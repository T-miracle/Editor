//! A private STA owns both IFileDialog and its cancel window; COM never crosses host threads.
use super::*;
use std::{cell::Cell, ffi::OsString, os::windows::ffi::OsStringExt, sync::OnceLock};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
                CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
            },
            LibraryLoader::GetModuleHandleW,
            Ole::IOleWindow,
        },
        UI::{
            Shell::{
                FOS_ALLOWMULTISELECT, FOS_DONTADDTORECENT, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM,
                FOS_NOCHANGEDIR, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS,
                FileOpenDialog, FileSaveDialog, IFileDialog, IFileOpenDialog, IFileSaveDialog,
                IShellItem, SIGDN_FILESYSPATH,
            },
            WindowsAndMessaging::{
                CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA,
                GetWindowLongPtrW, IsWindow, IsWindowVisible, KillTimer, PostMessageW,
                RegisterClassW, SetTimer, SetWindowLongPtrW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
                WM_CLOSE, WM_NCCREATE, WM_NCDESTROY, WM_TIMER, WNDCLASSW,
            },
        },
    },
    core::{HRESULT, Interface, PCWSTR, PWSTR, w},
};

const CANCEL: u32 = WM_APP + 73;
const CANCELLED: HRESULT = HRESULT(0x800704c7u32 as i32);
const CLASS: PCWSTR = w!("Nanobug.OwnedFilePicker.Cancel");

/// Only an owned nonce is posted; a recycled HWND cannot cancel a later request's dialog.
pub(super) fn cancel(state: &Control) {
    let window = state.window.load(Ordering::Acquire);
    if window != 0 {
        // SAFETY: this carries no COM pointer; the receiving thread checks its private nonce.
        let _ = unsafe {
            PostMessageW(
                Some(HWND(window as *mut _)),
                CANCEL,
                WPARAM(state.nonce),
                LPARAM(0),
            )
        };
    }
}

/// Every successful initialization is balanced after all dialogs and native windows are dropped.
struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

struct DialogContext {
    dialog: IFileDialog,
    state: Arc<Control>,
    /// Native modal loops may reenter this window procedure while Close is running.
    closing: Cell<bool>,
}

/// A stable boxed context remains valid until DestroyWindow finishes its final callbacks.
struct CancelWindow {
    window: HWND,
    _context: Box<DialogContext>,
    state: Arc<Control>,
}
impl Drop for CancelWindow {
    fn drop(&mut self) {
        self.state.window.store(0, Ordering::Release);
        // SAFETY: only this STA owns and destroys this window and its timer.
        unsafe {
            let _ = KillTimer(Some(self.window), 1);
            let _ = DestroyWindow(self.window);
        }
    }
}

/// The dialog's modal loop dispatches this private window on the same STA as its COM interface.
unsafe extern "system" fn procedure(window: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if message == WM_NCCREATE {
        // SAFETY: CreateWindowEx supplies CREATESTRUCTW and the stable Box pointer we passed.
        let create = unsafe { &*(lp.0 as *const CREATESTRUCTW) };
        unsafe {
            SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
        }
    }
    let pointer = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *const DialogContext;
    if !pointer.is_null() && (message == CANCEL || message == WM_TIMER) {
        // SAFETY: CancelWindow retains the Box through WM_NCDESTROY; this runs only on its STA.
        let context = unsafe { &*pointer };
        if message == WM_TIMER || wp.0 == context.state.nonce {
            // Folder callbacks precede display. Readiness comes from the actual modal HWND;
            // the owned timer also retries a cancellation that races shell initialization.
            let dialog_window = context
                .dialog
                .cast::<IOleWindow>()
                .ok()
                .and_then(|ole| unsafe { ole.GetWindow() }.ok());
            let visible =
                dialog_window.is_some_and(|handle| unsafe { IsWindowVisible(handle).as_bool() });
            context.state.ready.store(visible, Ordering::Release);
            if visible
                && context.state.cancelled.load(Ordering::Acquire)
                && !context.closing.replace(true)
            {
                // Preserve the HRESULT: the generated wrapper maps S_FALSE to Ok, while
                // only S_OK means this close was accepted. The guard survives reentrancy.
                let closed =
                    unsafe { (context.dialog.vtable().Close)(context.dialog.as_raw(), CANCELLED) };
                if closed == HRESULT(0) {
                    let _ = unsafe { KillTimer(Some(window), 1) };
                    // Defer the native dismiss to the shell window's own modal dispatch,
                    // rather than closing reentrantly while it is initializing.
                    if let Some(dialog_window) = context
                        .dialog
                        .cast::<IOleWindow>()
                        .ok()
                        .and_then(|ole| unsafe { ole.GetWindow() }.ok())
                        .filter(|handle| unsafe { IsWindow(Some(*handle)).as_bool() })
                    {
                        let _ = unsafe {
                            PostMessageW(Some(dialog_window), WM_CLOSE, WPARAM(0), LPARAM(0))
                        };
                    }
                } else {
                    context.closing.set(false);
                }
            }
        }
        return LRESULT(0);
    }
    if message == WM_NCDESTROY {
        unsafe {
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
        }
    }
    unsafe { DefWindowProcW(window, message, wp, lp) }
}

impl CancelWindow {
    fn new(dialog: &IFileDialog, state: Arc<Control>) -> windows::core::Result<Self> {
        static REGISTERED: OnceLock<Result<(), HRESULT>> = OnceLock::new();
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
        let registered = REGISTERED.get_or_init(|| {
            let class = WNDCLASSW {
                hInstance: instance,
                lpszClassName: CLASS,
                lpfnWndProc: Some(procedure),
                ..Default::default()
            };
            if unsafe { RegisterClassW(&class) } != 0 {
                Ok(())
            } else {
                Err(windows::core::Error::from_thread().code())
            }
        });
        if let Err(code) = registered {
            return Err(windows::core::Error::from_hresult(*code));
        }
        let context = Box::new(DialogContext {
            dialog: dialog.clone(),
            state: state.clone(),
            closing: Cell::new(false),
        });
        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                CLASS,
                w!(""),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                Some((&*context as *const DialogContext).cast()),
            )?
        };
        let owner = Self {
            window,
            _context: context,
            state,
        };
        // The nonce-bearing PostMessage is the fast path; a short timer closes startup race gaps.
        if unsafe { SetTimer(Some(window), 1, 50, None) } == 0 {
            return Err(windows::core::Error::from_thread());
        }
        owner
            .state
            .window
            .store(window.0 as isize, Ordering::Release);
        Ok(owner)
    }
}

/// Native shell names are allocated by COM; no selected path is read or written by this module.
fn selected_path(item: &IShellItem) -> windows::core::Result<PathBuf> {
    struct Name(PWSTR);
    impl Drop for Name {
        fn drop(&mut self) {
            unsafe {
                CoTaskMemFree(Some(self.0.0.cast()));
            }
        }
    }
    let name = Name(unsafe { item.GetDisplayName(SIGDN_FILESYSPATH)? });
    if name.0.0.is_null() {
        return Err(windows::core::Error::from_hresult(HRESULT(
            0x8007000du32 as i32,
        )));
    }
    // SAFETY: GetDisplayName returns a NUL-terminated COM allocation, retained until conversion ends.
    let units = unsafe { name.0.as_wide() };
    if units.is_empty() || units.len() > 32767 {
        return Err(windows::core::Error::from_hresult(HRESULT(
            0x8007000du32 as i32,
        )));
    }
    Ok(PathBuf::from(OsString::from_wide(units)))
}

/// Configure only filesystem selections; this does not turn a displayed basename into authority.
fn create_dialog(options: &FilePickerOptions) -> windows::core::Result<IFileDialog> {
    let dialog: IFileDialog = if options.kind == FilePickerKind::Save {
        let save: IFileSaveDialog =
            unsafe { CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)? };
        save.cast()?
    } else {
        let open: IFileOpenDialog =
            unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)? };
        open.cast()?
    };
    let mut flags = unsafe { dialog.GetOptions()? }
        | FOS_FORCEFILESYSTEM
        | FOS_PATHMUSTEXIST
        | FOS_NOCHANGEDIR
        | FOS_DONTADDTORECENT;
    match options.kind {
        FilePickerKind::OpenFile => flags |= FOS_FILEMUSTEXIST,
        FilePickerKind::Directory => flags |= FOS_PICKFOLDERS,
        FilePickerKind::Save => flags |= FOS_OVERWRITEPROMPT,
    }
    if options.multiple {
        flags |= FOS_ALLOWMULTISELECT;
    }
    unsafe {
        dialog.SetOptions(flags)?;
    }
    let title = options
        .title
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
    unsafe {
        dialog.SetTitle(PCWSTR(title.as_ptr()))?;
    }
    if let Some(name) = &options.suggested_name {
        let name = name.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        unsafe {
            dialog.SetFileName(PCWSTR(name.as_ptr()))?;
        }
    }
    Ok(dialog)
}

/// Collect names only after a successful Show, with a finite public multi-selection limit.
fn selected_paths(
    dialog: &IFileDialog,
    kind: FilePickerKind,
) -> windows::core::Result<Vec<PathBuf>> {
    if kind == FilePickerKind::Save {
        return Ok(vec![selected_path(&unsafe { dialog.GetResult()? })?]);
    }
    let open: IFileOpenDialog = dialog.cast()?;
    let results = unsafe { open.GetResults()? };
    let count = unsafe { results.GetCount()? };
    if count == 0 || count > 64 {
        return Err(windows::core::Error::new(
            HRESULT(0x800700dfu32 as i32),
            "Select between 1 and 64 filesystem objects",
        ));
    }
    (0..count)
        .map(|index| selected_path(&unsafe { results.GetItemAt(index)? }))
        .collect()
}

/// Run Show and Close on the same STA, including user cancel, host cancel and owner drop.
pub(super) fn choose(options: FilePickerOptions, state: Arc<Control>) -> FilePickerResult {
    let result = (|| -> windows::core::Result<Option<Vec<PathBuf>>> {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).ok()?;
        }
        let _apartment = Apartment;
        let parent = (options.owner_hwnd != 0).then_some(HWND(options.owner_hwnd as *mut _));
        if parent.is_some_and(|window| !unsafe { IsWindow(Some(window)).as_bool() }) {
            return Err(windows::core::Error::from_hresult(HRESULT(
                0x80070006u32 as i32,
            )));
        }
        let dialog = create_dialog(&options)?;
        let _window = CancelWindow::new(&dialog, state.clone())?;
        if state.cancelled.load(Ordering::Acquire) {
            return Ok(None);
        }
        let shown = unsafe { dialog.Show(parent.or(Some(_window.window))) };
        match shown {
            Err(error) if error.code() == CANCELLED => return Ok(None),
            other => other?,
        }
        if state.cancelled.load(Ordering::Acquire) {
            return Ok(None);
        }
        selected_paths(&dialog, options.kind).map(Some)
    })();
    result.map_err(|error| io::Error::other(format!("Native file selection failed: {error}")))
}
