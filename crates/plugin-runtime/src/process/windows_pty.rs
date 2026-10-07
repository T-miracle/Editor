//! Windows PTY creation joins a kill-on-close job atomically, before the first guest instruction.
use super::*;
use std::{
    cell::RefCell,
    ffi::OsStr,
    fs::File,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    ptr,
    sync::{Arc, Mutex},
};
use windows_sys::Win32::{
    Foundation::*,
    System::{Console::*, Pipes::CreatePipe, Threading::*},
};

/// Native handles stay owned through every partial initialization failure.
fn pipe() -> anyhow::Result<(File, File)> {
    let (mut read, mut write) = (ptr::null_mut(), ptr::null_mut());
    anyhow::ensure!(
        unsafe { CreatePipe(&mut read, &mut write, ptr::null(), 0) } != 0,
        "{}",
        std::io::Error::last_os_error()
    );
    Ok(unsafe { (File::from_raw_handle(read), File::from_raw_handle(write)) })
}

struct Console(HPCON);
// ConPTY handles are thread independent; ownership remains unique.
unsafe impl Send for Console {}
impl Drop for Console {
    fn drop(&mut self) {
        // A failed spawn can still leave an unanswered query. Close only after stack-owned pipes
        // can unwind, never synchronously on the instance worker that would close those pipes.
        let console = self.0;
        std::thread::spawn(move || unsafe { ClosePseudoConsole(console) });
    }
}

/// Attribute storage is pointer-aligned and lives until CreateProcessW finishes consuming it.
struct Attributes {
    storage: Vec<usize>,
    initialized: bool,
}
impl Attributes {
    fn new(console: HPCON, job: &Job) -> anyhow::Result<Self> {
        let mut size = 0;
        unsafe {
            InitializeProcThreadAttributeList(ptr::null_mut(), 2, 0, &mut size);
        }
        let mut value = Self {
            storage: vec![0; size.div_ceil(std::mem::size_of::<usize>())],
            initialized: false,
        };
        anyhow::ensure!(
            unsafe { InitializeProcThreadAttributeList(value.pointer(), 2, 0, &mut size) } != 0,
            "Cannot allocate process attributes"
        );
        value.initialized = true;
        for (key, data, size) in [
            (
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
                console as *const _,
                std::mem::size_of::<HPCON>(),
            ),
            (
                PROC_THREAD_ATTRIBUTE_JOB_LIST,
                &job.0 as *const _ as *const _,
                std::mem::size_of::<HANDLE>(),
            ),
        ] {
            anyhow::ensure!(
                unsafe {
                    UpdateProcThreadAttribute(
                        value.pointer(),
                        0,
                        key as usize,
                        data,
                        size,
                        ptr::null_mut(),
                        ptr::null(),
                    )
                } != 0,
                "Cannot bind process attributes: {}",
                std::io::Error::last_os_error()
            );
        }
        Ok(value)
    }
    fn pointer(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                DeleteProcThreadAttributeList(self.pointer());
            }
        }
    }
}

/// Encode each argv element using the Windows CRT rules, including trailing slashes and quotes.
fn quote(argument: &str) -> String {
    let mut value = String::from("\"");
    let mut slashes = 0;
    for ch in argument.chars() {
        if ch == '\\' {
            slashes += 1;
            continue;
        }
        value.extend(std::iter::repeat_n(
            '\\',
            if ch == '"' { slashes * 2 + 1 } else { slashes },
        ));
        value.push(ch);
        slashes = 0;
    }
    value.extend(std::iter::repeat_n('\\', slashes * 2));
    value.push('"');
    value
}
fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

/// Build the child environment: the host environment, then the caller's entries, then the terminal
/// identity. A caller's entry replaces an inherited one case-insensitively, which is how Windows
/// compares environment variable names.
fn environment(overrides: &std::collections::BTreeMap<String, String>) -> Vec<u16> {
    let mut entries = std::env::vars_os()
        .filter(|(key, _)| {
            !key.eq_ignore_ascii_case("TERM") && !key.eq_ignore_ascii_case("COLORTERM")
        })
        .map(|(key, value)| (key.to_string_lossy().to_uppercase(), key, value))
        .collect::<Vec<_>>();
    for (key, value) in overrides {
        let upper = key.to_uppercase();
        entries.retain(|(existing, _, _)| *existing != upper);
        entries.push((
            upper,
            std::ffi::OsString::from(key),
            std::ffi::OsString::from(value),
        ));
    }
    let mut entries = entries
        .into_iter()
        .map(|(_, key, value)| {
            let mut entry = key;
            entry.push("=");
            entry.push(value);
            entry
        })
        .collect::<Vec<_>>();
    // The terminal identity stays the host's, so a caller cannot misdescribe the transport.
    entries.extend(["TERM=xterm-256color".into(), "COLORTERM=truecolor".into()]);
    entries.sort_by_key(|entry| entry.to_string_lossy().to_uppercase());
    entries
        .into_iter()
        .flat_map(|entry| entry.encode_wide().chain(Some(0)).collect::<Vec<_>>())
        .chain(Some(0))
        .collect()
}

/// The shared child adapter reports status without holding a thread handle or inheritable authority.
#[derive(Debug, Clone)]
struct NativeChild(Arc<OwnedHandle>);
impl portable_pty::ChildKiller for NativeChild {
    fn kill(&mut self) -> std::io::Result<()> {
        if unsafe { TerminateProcess(self.0.as_raw_handle(), 1) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
    fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
        Box::new(self.clone())
    }
}
impl Child for NativeChild {
    fn try_wait(&mut self) -> std::io::Result<Option<portable_pty::ExitStatus>> {
        let state = unsafe { WaitForSingleObject(self.0.as_raw_handle(), 0) };
        if state == WAIT_TIMEOUT {
            return Ok(None);
        }
        if state == WAIT_FAILED {
            return Err(std::io::Error::last_os_error());
        }
        let mut code = 0;
        if unsafe { GetExitCodeProcess(self.0.as_raw_handle(), &mut code) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Some(portable_pty::ExitStatus::with_exit_code(code)))
    }
    fn wait(&mut self) -> std::io::Result<portable_pty::ExitStatus> {
        unsafe {
            WaitForSingleObject(self.0.as_raw_handle(), INFINITE);
        }
        self.try_wait()?
            .ok_or_else(|| std::io::Error::other("Process did not exit"))
    }
    fn process_id(&self) -> Option<u32> {
        Some(unsafe { GetProcessId(self.0.as_raw_handle()) })
    }
    fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        Some(self.0.as_raw_handle())
    }
}

/// Resize and byte streams implement only the generic PTY contract, without terminal state or UI.
struct NativePty {
    _shutdown: Shutdown,
    resize: ResizeQueue,
    reader: File,
    writer: RefCell<Option<File>>,
    size: RefCell<PtySize>,
}
impl MasterPty for NativePty {
    fn resize(&self, size: PtySize) -> anyhow::Result<()> {
        self.resize.request(size)?;
        *self.size.borrow_mut() = size;
        Ok(())
    }
    fn get_size(&self) -> anyhow::Result<PtySize> {
        self.resize.check()?;
        Ok(*self.size.borrow())
    }
    fn try_clone_reader(&self) -> anyhow::Result<Box<dyn Read + Send>> {
        Ok(Box::new(self.reader.try_clone()?))
    }
    fn take_writer(&self) -> anyhow::Result<Box<dyn Write + Send>> {
        Ok(Box::new(self.writer.borrow_mut().take().ok_or_else(
            || anyhow::anyhow!("PTY writer already taken"),
        )?))
    }
}

/// One native resize may wait for a guest cursor reply; coalesce later sizes without blocking
/// process input, output, or retirement. Closing the owned pipes lets the worker release ConPTY.
struct ResizeQueue {
    pending: Arc<Mutex<Option<PtySize>>>,
    failure: Arc<Mutex<Option<String>>>,
    wake: mpsc::SyncSender<()>,
}
impl ResizeQueue {
    fn new(console: Arc<Console>) -> Self {
        let pending = Arc::new(Mutex::new(None::<PtySize>));
        let failure = Arc::new(Mutex::new(None));
        let (wake, events) = mpsc::sync_channel(1);
        let queued = pending.clone();
        let error = failure.clone();
        std::thread::spawn(move || {
            while events.recv().is_ok() {
                let Some(size) = queued.lock().unwrap().take() else {
                    continue;
                };
                let result = unsafe {
                    ResizePseudoConsole(
                        console.0,
                        COORD {
                            X: size.cols as i16,
                            Y: size.rows as i16,
                        },
                    )
                };
                if result < 0 {
                    *error.lock().unwrap() =
                        Some(format!("Cannot resize console: HRESULT {result:#x}"));
                    break;
                }
            }
            // Drop on channel closure releases the console after queued native work completes.
            drop(console);
        });
        Self {
            pending,
            failure,
            wake,
        }
    }

    /// Native failures become ordinary process errors on the next poll, even without another drag.
    fn check(&self) -> anyhow::Result<()> {
        if let Some(error) = self.failure.lock().unwrap().as_ref() {
            anyhow::bail!("{error}");
        }
        Ok(())
    }

    /// A full wake queue already covers the latest-size slot; no resize threads accumulate.
    fn request(&self, size: PtySize) -> anyhow::Result<()> {
        self.check()?;
        *self.pending.lock().unwrap() = Some(size);
        match self.wake.try_send(()) {
            Ok(()) | Err(mpsc::TrySendError::Full(())) => Ok(()),
            Err(mpsc::TrySendError::Disconnected(())) => {
                anyhow::bail!("Console resize worker stopped")
            }
        }
    }
}

/// ConPTY requires a cursor reply even after failed creation or a retired consumer. Keep that
/// cancellation reply separate from live guest bytes, and send it only after its whole job exits.
struct Shutdown {
    console: Arc<Console>,
    job: Arc<Job>,
    input: Option<File>,
    inherit_cursor: bool,
    process_id: Option<u32>,
    diagnostics: Option<crate::faults::NativeReporter>,
}
impl Drop for Shutdown {
    fn drop(&mut self) {
        let console = self.console.clone();
        let job = self.job.clone();
        let mut input = self.input.take().expect("owned cancellation pipe");
        let inherit_cursor = self.inherit_cursor;
        let process_id = self.process_id;
        let diagnostics = self.diagnostics.take().expect("owned native diagnostics");
        // No join on the lifecycle worker: pending native resize and write calls remain isolated.
        std::thread::spawn(move || {
            match job.terminate_and_wait() {
                Err(error) => diagnostics.record(
                    "pty.retire.wait",
                    format!("process={process_id:?}: {error:#}"),
                ),
                Ok(()) if inherit_cursor => {
                    // No application survives to interpret this neutral response as user input.
                    // A closed pipe already completed this channel's teardown; other errors stay diagnosable.
                    if let Err(error) = input.write_all(b"\x1b[1;1R")
                        && error.kind() != std::io::ErrorKind::BrokenPipe
                    {
                        diagnostics.record(
                            "pty.retire.cursor",
                            format!("process={process_id:?}: {error}"),
                        );
                    }
                }
                Ok(()) => {}
            }
            drop(input);
            drop(console);
        });
    }
}

/// Creation attributes bind the process to both ConPTY and its owning job in a single OS operation.
pub(super) fn spawn(
    program: &str,
    args: &[String],
    cwd: &str,
    size: PtySize,
    inherit_cursor: bool,
    env: &std::collections::BTreeMap<String, String>,
    diagnostics: crate::faults::NativeReporter,
) -> anyhow::Result<(
    Box<dyn Child + Send + Sync>,
    Box<dyn MasterPty + Send>,
    Arc<Job>,
)> {
    let job = Arc::new(Job::empty()?);
    let (input_read, input_write) = pipe()?;
    let (output_read, output_write) = pipe()?;
    let mut console = 0;
    anyhow::ensure!(
        unsafe {
            CreatePseudoConsole(
                COORD {
                    X: size.cols as i16,
                    Y: size.rows as i16,
                },
                input_read.as_raw_handle(),
                output_write.as_raw_handle(),
                // Only opted-in VT consumers receive the OS cursor query; the host never parses it.
                if inherit_cursor {
                    PSEUDOCONSOLE_INHERIT_CURSOR
                } else {
                    0
                },
                &mut console,
            )
        } >= 0,
        "Cannot create console"
    );
    let console = Arc::new(Console(console));
    // Establish cancellation ownership before any remaining fallible allocation or process call.
    let mut shutdown = Shutdown {
        console: console.clone(),
        job: job.clone(),
        input: Some(input_write),
        inherit_cursor,
        process_id: None,
        diagnostics: Some(diagnostics),
    };
    let input_write = shutdown.input.as_ref().unwrap().try_clone()?;
    drop(input_read);
    drop(output_write);
    let mut attributes = Attributes::new(console.0, &job)?;
    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    // Do not inherit the editor/test runner's redirected stdio; ConPTY supplies the console streams.
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
    startup.lpAttributeList = attributes.pointer();
    let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let exe = wide(program);
    let cwd = wide(cwd);
    let environment = environment(env);
    let mut command = wide(
        &std::iter::once(program)
            .chain(args.iter().map(String::as_str))
            .map(quote)
            .collect::<Vec<_>>()
            .join(" "),
    );
    anyhow::ensure!(
        unsafe {
            CreateProcessW(
                exe.as_ptr(),
                command.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
                environment.as_ptr().cast(),
                cwd.as_ptr(),
                &startup.StartupInfo,
                &mut info,
            )
        } != 0,
        "{}",
        std::io::Error::last_os_error()
    );
    let child = NativeChild(Arc::new(unsafe {
        OwnedHandle::from_raw_handle(info.hProcess)
    }));
    shutdown.process_id = Some(info.dwProcessId);
    drop(unsafe { OwnedHandle::from_raw_handle(info.hThread) });
    Ok((
        Box::new(child),
        Box::new(NativePty {
            _shutdown: shutdown,
            resize: ResizeQueue::new(console),
            reader: output_read,
            writer: RefCell::new(Some(input_write)),
            size: RefCell::new(size),
        }),
        job,
    ))
}
