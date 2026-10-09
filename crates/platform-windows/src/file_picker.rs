//! Instance-owned native file selection; cancellation closes the dialog without returning authority.
use std::{
    io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering},
        mpsc,
    },
};

#[cfg(windows)]
mod native;

/// Dialog threads are bounded even when multiple guests request selection concurrently.
#[cfg(windows)]
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
#[cfg(windows)]
static NEXT_OWNER: AtomicUsize = AtomicUsize::new(1);

/// Open and directory results are read intents; save results are exact-target write intents only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilePickerKind {
    OpenFile,
    Directory,
    Save,
}

/// Trusted host options. Guest requests never supply an HWND or receive an absolute path directly.
#[derive(Clone, Debug)]
pub struct FilePickerOptions {
    /// Native parent window; zero uses a private hidden owner on the dialog's STA.
    pub owner_hwnd: isize,
    /// Plugin-supplied display title already admitted by the public interaction contract.
    pub title: String,
    /// Selection intent; this API performs no file reads or writes.
    pub kind: FilePickerKind,
    /// Multiple selection is valid only for open-file and directory modes.
    pub multiple: bool,
    /// Optional initial basename. This never becomes a path authority by itself.
    pub suggested_name: Option<String>,
}

/// A native terminal selection; None is cancellation, never an empty successful grant.
pub type FilePickerResult = io::Result<Option<Vec<PathBuf>>>;

/// The host retains this controller for the original request/window lifetime.
pub struct FilePickerControl {
    state: Arc<Control>,
}

/// Only the native thread dereferences COM objects; cross-thread cancellation posts an owned nonce.
struct Control {
    cancelled: AtomicBool,
    /// True only after IOleWindow exposes the visible modal shell window.
    ready: AtomicBool,
    window: AtomicIsize,
    nonce: usize,
}

impl FilePickerControl {
    /// Cancel the owning dialog. Late selection must still pass the runtime completion gate.
    pub fn cancel(&self) {
        self.state.cancelled.store(true, Ordering::Release);
        #[cfg(windows)]
        native::cancel(&self.state);
    }
}

impl Drop for FilePickerControl {
    /// Dropping the original UI owner also closes a pending system dialog.
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Result transport and cancellation are separate so an async host can observe both lifetimes.
pub struct FilePicker {
    /// Keep this owner until the request is terminal; cancellation is safe from any host thread.
    pub control: FilePickerControl,
    /// Blocking receiver intended for a background host task, never the GPUI render thread.
    pub result: mpsc::Receiver<FilePickerResult>,
}

/// Start an owned native selection without blocking the caller; no permissions are granted here.
/// Options control display and intent; platform errors arrive on result, or on spawn failure.
pub fn pick_files(options: FilePickerOptions) -> io::Result<FilePicker> {
    validate(&options)?;
    #[cfg(not(windows))]
    return Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Native file selection is supported on Windows",
    ));
    #[cfg(windows)]
    start(options)
}

/// Defend the platform boundary as well as the plugin contract; strings become native UTF-16.
fn validate(options: &FilePickerOptions) -> io::Result<()> {
    if options.title.is_empty()
        || options.title.len() > 512
        || options.title.contains('\0')
        || (options.kind == FilePickerKind::Save && options.multiple)
        || options.suggested_name.as_ref().is_some_and(|name| {
            name.is_empty()
                || name.len() > 255
                || name.contains(['/', '\\', ':', '\0'])
                || name == "."
                || name == ".."
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid native file selection options",
        ));
    }
    Ok(())
}

/// Reserve a finite worker before displaying anything; each worker releases its permit before reply.
#[cfg(windows)]
fn start(options: FilePickerOptions) -> io::Result<FilePicker> {
    IN_FLIGHT
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            (count < 4).then_some(count + 1)
        })
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "Native selection workers are busy",
            )
        })?;
    struct Permit;
    impl Drop for Permit {
        fn drop(&mut self) {
            IN_FLIGHT.fetch_sub(1, Ordering::AcqRel);
        }
    }
    let permit = Permit;
    let nonce = NEXT_OWNER
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
            value.checked_add(1)
        })
        .map_err(|_| io::Error::other("Native selection identities exhausted"))?;
    let state = Arc::new(Control {
        cancelled: AtomicBool::new(false),
        ready: AtomicBool::new(false),
        window: AtomicIsize::new(0),
        nonce,
    });
    let native_state = state.clone();
    let (send, result) = mpsc::channel();
    std::thread::Builder::new()
        .name("nanobug-file-selection".into())
        .spawn(move || {
            let selection = if native_state.cancelled.load(Ordering::Acquire) {
                Ok(None)
            } else {
                native::choose(options, native_state.clone())
            };
            let selection = if native_state.cancelled.load(Ordering::Acquire) {
                Ok(None)
            } else {
                selection
            };
            drop(permit);
            // A disconnected async owner already cancelled; no path is saved or published elsewhere.
            let _ = send.send(selection);
        })?;
    Ok(FilePicker {
        control: FilePickerControl { state },
        result,
    })
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Cancellation before display must terminate without an OS selection or an empty grant.
    #[test]
    fn cancelled_native_picker_returns_no_selection() {
        let picker = pick_files(FilePickerOptions {
            owner_hwnd: 0,
            title: "Nanobug cancellation test".into(),
            kind: FilePickerKind::OpenFile,
            multiple: false,
            suggested_name: None,
        })
        .unwrap();
        picker.control.cancel();
        let result = picker.result.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            result.unwrap().is_none(),
            "cancellation must not select any file"
        );
    }

    /// Malformed transport cannot display a dialog or reserve a worker, regardless of the caller.
    #[test]
    fn invalid_native_picker_options_are_rejected_before_display() {
        for (title, kind, multiple, name) in [
            ("bad\0title", FilePickerKind::OpenFile, false, None),
            ("Save", FilePickerKind::Save, true, None),
            ("Save", FilePickerKind::Save, false, Some("../outside")),
        ] {
            let result = pick_files(FilePickerOptions {
                owner_hwnd: 0,
                title: title.into(),
                kind,
                multiple,
                suggested_name: name.map(str::to_owned),
            });
            assert!(matches!(result, Err(error) if error.kind() == io::ErrorKind::InvalidInput));
        }
    }

    /// A close while Show pumps native messages must return cancellation and release the control HWND.
    #[test]
    #[ignore = "opens a real Windows file dialog; run explicitly during native interaction verification"]
    fn visible_native_picker_cancellation_closes_its_owner() {
        for kind in [
            FilePickerKind::OpenFile,
            FilePickerKind::Directory,
            FilePickerKind::Save,
        ] {
            let picker = pick_files(FilePickerOptions {
                owner_hwnd: 0,
                title: format!("Nanobug native picker lifecycle test {kind:?}"),
                kind,
                multiple: false,
                suggested_name: None,
            })
            .unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !picker.control.state.ready.load(Ordering::Acquire)
                && std::time::Instant::now() < deadline
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_ne!(
                picker.control.state.ready.load(Ordering::Acquire),
                false,
                "native dialog should become visible"
            );
            picker.control.cancel();
            assert!(
                picker
                    .result
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap_or_else(|error| panic!(
                        "Native {kind:?} cancellation timed out: {error}"
                    ))
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                picker.control.state.window.load(Ordering::Acquire),
                0,
                "native window must be destroyed before terminal reply"
            );
        }
    }

    /// Losing the native UI owner closes Show even without a separate explicit cancellation call.
    #[test]
    #[ignore = "opens a real Windows file dialog; run explicitly during native interaction verification"]
    fn dropping_native_picker_owner_closes_pending_selection() {
        let FilePicker { control, result } = pick_files(FilePickerOptions {
            owner_hwnd: 0,
            title: "Nanobug picker owner-drop test".into(),
            kind: FilePickerKind::OpenFile,
            multiple: false,
            suggested_name: None,
        })
        .unwrap();
        let state = control.state.clone();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !state.ready.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            state.ready.load(Ordering::Acquire),
            "native dialog should become visible"
        );
        drop(control);
        assert!(
            result
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap()
                .is_none()
        );
        assert_eq!(state.window.load(Ordering::Acquire), 0);
    }
}
