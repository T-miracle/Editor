//! Owned native pipes and PTYs with bounded I/O; no escape parsing or terminal state lives here.
use plugin_protocol::Event;
use plugin_protocol::process::{Stream, Update};
use portable_pty::{Child, MasterPty, PtySize};
#[cfg(not(windows))]
use portable_pty::{CommandBuilder, native_pty_system};
#[cfg(windows)]
mod windows_pty;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

/// Native applications redraw after PTY resize; wait until an interactive drag settles.
const RESIZE_SETTLE: Duration = Duration::from_millis(150);

struct Process {
    // Drop the job and output receiver before ConPTY: otherwise a full reader queue
    // can block ClosePseudoConsole while it tries to flush the dying process output.
    #[cfg(windows)]
    _job: Job,
    output: Receiver<(Stream, Vec<u8>)>,
    input: mpsc::SyncSender<Vec<u8>>,
    child: Box<dyn Child + Send + Sync>,
    master: Option<Box<dyn MasterPty + Send>>,
    applied_size: (u16, u16),
    pending_resize: Option<(u16, u16, Instant)>,
    exit_code: Option<u32>,
}
impl Process {
    /// Apply the latest requested size after the quiet period expires.
    fn flush_resize(&mut self) -> anyhow::Result<()> {
        if let Some((cols, rows, _)) = self.pending_resize {
            self.master.as_ref().expect("PTY resize").resize(PtySize {
                rows,
                cols,
                ..PtySize::default()
            })?;
            self.applied_size = (cols, rows);
            self.pending_resize = None;
        }
        Ok(())
    }
}
impl Drop for Process {
    /// Dropping a resource terminates its whole process tree and releases its PTY.
    fn drop(&mut self) {
        let _ = self.child.kill();
        // ClosePseudoConsole may flush pending output; teardown must not block the lifecycle worker.
        if let Some(master) = self.master.take() {
            std::thread::spawn(move || drop(master));
        }
    }
}
#[cfg(windows)]
struct Job(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
unsafe impl Send for Job {}
#[cfg(windows)]
impl Job {
    /// Windows job ownership closes descendants even when the shell has exited first.
    fn new(child: &dyn Child) -> anyhow::Result<Self> {
        use windows_sys::Win32::System::JobObjects::*;
        let job = Self::empty()?;
        anyhow::ensure!(
            unsafe {
                AssignProcessToJobObject(
                    job.0,
                    child
                        .as_raw_handle()
                        .ok_or_else(|| anyhow::anyhow!("Missing process handle"))?
                        as _,
                )
            } != 0,
            "Cannot own process tree"
        );
        Ok(job)
    }
    /// Create before launch so suspended stdio or atomic PTY creation cannot leak early descendants.
    fn empty() -> anyhow::Result<Self> {
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            let job = Self(CreateJobObjectW(std::ptr::null(), std::ptr::null()));
            anyhow::ensure!(!job.0.is_null(), "Cannot create process job");
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            anyhow::ensure!(
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const _,
                    std::mem::size_of_val(&limits) as u32
                ) != 0,
                "Cannot restrict process lifetime"
            );
            Ok(job)
        }
    }
    fn terminate(&self) {
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0, 1);
        }
    }
}
#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[derive(Default)]
pub(crate) struct Processes {
    next: u64,
    items: BTreeMap<u64, Process>,
}
impl Processes {
    /// Bound live processes and output buffers per plugin, with backpressure on noisy children.
    pub fn spawn(
        &mut self,
        program: String,
        args: Vec<String>,
        cwd: String,
        cols: u16,
        rows: u16,
        inherit_cursor: bool,
    ) -> anyhow::Result<u64> {
        anyhow::ensure!(self.items.len() < 32, "Plugin process quota exceeded");
        let rows = rows.clamp(1, 500);
        let cols = cols.clamp(2, 1000);
        let size = PtySize {
            rows,
            cols,
            ..PtySize::default()
        };
        // Strip Windows device-path prefixes so PowerShell presents an ordinary filesystem prompt.
        let cwd = if let Some(p) = cwd.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{p}")
        } else {
            cwd.strip_prefix(r"\\?\").unwrap_or(&cwd).to_owned()
        };
        #[cfg(windows)]
        let (child, master, job) = windows_pty::spawn(
            &crate::toolchains::resolve(&program)?.display().to_string(),
            &args,
            &cwd,
            size,
            inherit_cursor,
        )?;
        #[cfg(not(windows))]
        let (child, master) = {
            let pair = native_pty_system().openpty(size)?;
            let mut command = CommandBuilder::new(program);
            command.args(args);
            command.cwd(cwd);
            command.env("TERM", "xterm-256color");
            command.env("COLORTERM", "truecolor");
            let child = pair.slave.spawn_command(command)?;
            drop(pair.slave);
            (child, pair.master)
        };
        let reader = master.try_clone_reader()?;
        let mut writer = master.take_writer()?;
        let (input, writes) = mpsc::sync_channel::<Vec<u8>>(8);
        // A child that stops reading must not block the plugin lifecycle worker.
        std::thread::spawn(move || {
            while let Ok(bytes) = writes.recv() {
                if writer.write_all(&bytes).is_err() {
                    break;
                }
            }
        });
        let (tx, output) = mpsc::sync_channel(64);
        read_output(reader, tx, Stream::Pty);
        self.next += 1;
        self.items.insert(
            self.next,
            Process {
                master: Some(master),
                input,
                child,
                output,
                applied_size: (cols, rows),
                pending_resize: None,
                exit_code: None,
                #[cfg(windows)]
                _job: job,
            },
        );
        Ok(self.next)
    }
    pub fn write(&mut self, id: u64, bytes: &[u8]) -> anyhow::Result<()> {
        anyhow::ensure!(bytes.len() <= 1024 * 1024, "Input quota exceeded");
        self.items
            .get_mut(&id)
            .ok_or_else(|| anyhow::anyhow!("Unknown process handle"))?
            .input
            .try_send(bytes.to_vec())
            .map_err(|e| anyhow::anyhow!("Process input is unavailable: {e}"))?;
        Ok(())
    }
    pub fn resize(&mut self, id: u64, cols: u16, rows: u16) -> anyhow::Result<()> {
        let process = self
            .items
            .get_mut(&id)
            .ok_or_else(|| anyhow::anyhow!("Unknown process handle"))?;
        let size = (cols.clamp(2, 1000), rows.clamp(1, 500));
        anyhow::ensure!(process.master.is_some(), "Cannot resize a stdio process");
        // A drag returning to the applied size needs no ConPTY redraw at all.
        process.pending_resize =
            (size != process.applied_size).then_some((size.0, size.1, Instant::now()));
        Ok(())
    }
    pub fn close(&mut self, id: u64) -> anyhow::Result<()> {
        self.items
            .remove(&id)
            .ok_or_else(|| anyhow::anyhow!("Unknown process handle"))?;
        Ok(())
    }
    pub fn clear(&mut self) {
        self.items.clear();
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    /// Diagnostics expose process identifiers, never native handles or cross-plugin access.
    pub fn ids(&self) -> Vec<u32> {
        self.items
            .values()
            .filter_map(|p| p.child.process_id())
            .collect()
    }
    /// Drain a bounded batch so one noisy process cannot starve the UI event queue.
    pub fn poll(&mut self) -> anyhow::Result<Vec<Event>> {
        Ok(self
            .poll_native()?
            .into_iter()
            .map(|(handle, update)| match update {
                Update::Output { bytes, .. } => Event::ProcessOutput { handle, bytes },
                Update::Exited { .. } | Update::Terminated => Event::ProcessExit { handle },
            })
            .collect())
    }
    /// EOF, not a temporarily empty queue, is the barrier that makes exit follow every output byte.
    pub fn poll_native(&mut self) -> anyhow::Result<Vec<(u64, Update)>> {
        let mut events = vec![];
        let mut exited = vec![];
        for (&handle, process) in &mut self.items {
            if process.exit_code.is_none()
                && let Some(status) = process.child.try_wait()?
            {
                process.exit_code = Some(status.exit_code());
                // Descendants share the owner's lifetime, even if they inherited its output pipes.
                #[cfg(windows)]
                process._job.terminate();
                if let Some(master) = process.master.take() {
                    std::thread::spawn(move || drop(master));
                }
                process.pending_resize = None;
            }
            if process
                .pending_resize
                .is_some_and(|(_, _, at)| at.elapsed() >= RESIZE_SETTLE)
            {
                process.flush_resize()?;
            }
            let mut eof = false;
            for _ in 0..8 {
                match process.output.try_recv() {
                    Ok((stream, bytes)) => events.push((handle, Update::Output { stream, bytes })),
                    Err(mpsc::TryRecvError::Disconnected) => {
                        eof = true;
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                }
            }
            if eof && let Some(code) = process.exit_code {
                events.push((handle, Update::Exited { code }));
                exited.push(handle);
            }
        }
        for id in exited {
            self.items.remove(&id);
        }
        Ok(events)
    }
}

/// Bounded queues apply backpressure; dropping the receiver releases a reader blocked on delivery.
fn read_output(
    mut reader: impl Read + Send + 'static,
    tx: mpsc::SyncSender<(Stream, Vec<u8>)>,
    stream: Stream,
) {
    std::thread::spawn(move || {
        let mut buffer = [0; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send((stream, buffer[..n].to_vec())).is_err() {
                        break;
                    }
                }
            }
        }
    });
}

impl Processes {
    /// Pipes never emulate a terminal: stdout/stderr remain byte-exact independent streams.
    pub fn spawn_stdio(
        &mut self,
        program: &std::path::Path,
        args: &[String],
        cwd: &std::path::Path,
    ) -> anyhow::Result<u64> {
        use std::process::{Command, Stdio};
        anyhow::ensure!(self.items.len() < 32, "Plugin process quota exceeded");
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // The child cannot execute any code until it belongs to the host's kill-on-close job.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(
                windows_sys::Win32::System::Threading::CREATE_SUSPENDED
                    | windows_sys::Win32::System::Threading::CREATE_NO_WINDOW,
            );
        }
        let mut child = command.spawn()?;
        #[cfg(windows)]
        let job = match Job::new(&child).and_then(|job| {
            resume(&child)?;
            Ok(job)
        }) {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let reader = child.stdout.take().expect("piped stdout");
        let errors = child.stderr.take().expect("piped stderr");
        let mut writer = child.stdin.take().expect("piped stdin");
        let (input, writes) = mpsc::sync_channel::<Vec<u8>>(8);
        std::thread::spawn(move || {
            while let Ok(bytes) = writes.recv() {
                if writer.write_all(&bytes).is_err() {
                    break;
                }
            }
        });
        let (tx, output) = mpsc::sync_channel(64);
        read_output(reader, tx.clone(), Stream::Stdout);
        read_output(errors, tx, Stream::Stderr);
        self.next += 1;
        self.items.insert(
            self.next,
            Process {
                output,
                input,
                child: Box::new(child),
                master: None,
                applied_size: (0, 0),
                pending_resize: None,
                exit_code: None,
                #[cfg(windows)]
                _job: job,
            },
        );
        Ok(self.next)
    }
}

/// Resume only after successful job assignment; failure leaves the child suspended for cleanup.
#[cfg(windows)]
fn resume(child: &std::process::Child) -> anyhow::Result<()> {
    use std::os::windows::io::AsRawHandle;
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtResumeProcess(process: windows_sys::Win32::Foundation::HANDLE) -> i32;
    }
    anyhow::ensure!(
        unsafe { NtResumeProcess(AsRawHandle::as_raw_handle(child)) } >= 0,
        "Cannot resume owned process"
    );
    Ok(())
}
