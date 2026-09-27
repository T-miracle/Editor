//! Generic PTY resource ownership. No escape parsing or terminal state lives here.
use plugin_protocol::Event;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
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
    output: Receiver<Vec<u8>>,
    input: mpsc::SyncSender<Vec<u8>>,
    child: Box<dyn Child + Send + Sync>,
    master: Box<dyn MasterPty + Send>,
    applied_size: (u16, u16),
    pending_resize: Option<(u16, u16, Instant)>,
}
impl Process {
    /// Apply the latest requested size after the quiet period expires.
    fn flush_resize(&mut self) -> anyhow::Result<()> {
        if let Some((cols, rows, _)) = self.pending_resize {
            self.master.resize(PtySize {
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
            anyhow::ensure!(
                AssignProcessToJobObject(
                    job.0,
                    child
                        .as_raw_handle()
                        .ok_or_else(|| anyhow::anyhow!("Missing process handle"))?
                        as _
                ) != 0,
                "Cannot own process tree"
            );
            Ok(job)
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
    ) -> anyhow::Result<u64> {
        anyhow::ensure!(self.items.len() < 32, "Plugin process quota exceeded");
        let rows = rows.clamp(1, 500);
        let cols = cols.clamp(2, 1000);
        let pair = native_pty_system().openpty(PtySize {
            rows,
            cols,
            ..PtySize::default()
        })?;
        let mut command = CommandBuilder::new(program);
        command.args(args);
        // Strip Windows device-path prefixes so PowerShell presents an ordinary filesystem prompt.
        let cwd = if let Some(p) = cwd.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{p}")
        } else {
            cwd.strip_prefix(r"\\?\").unwrap_or(&cwd).to_owned()
        };
        command.cwd(cwd);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        let mut child = pair.slave.spawn_command(command)?;
        #[cfg(windows)]
        let job = match Job::new(&*child) {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill();
                return Err(error);
            }
        };
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let mut writer = pair.master.take_writer()?;
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
        std::thread::spawn(move || {
            let mut buffer = [0; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(buffer[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        self.next += 1;
        self.items.insert(
            self.next,
            Process {
                master: pair.master,
                input,
                child,
                output,
                applied_size: (cols, rows),
                pending_resize: None,
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
        let mut events = vec![];
        let mut exited = vec![];
        for (&handle, process) in &mut self.items {
            if process
                .pending_resize
                .is_some_and(|(_, _, at)| at.elapsed() >= RESIZE_SETTLE)
            {
                process.flush_resize()?;
            }
            let mut bytes = Vec::new();
            for _ in 0..8 {
                match process.output.try_recv() {
                    Ok(chunk) => bytes.extend(chunk),
                    Err(_) => break,
                }
            }
            if !bytes.is_empty() {
                events.push(Event::ProcessOutput { handle, bytes });
            } else if process.child.try_wait().ok().flatten().is_some() {
                events.push(Event::ProcessExit { handle });
                exited.push(handle);
            }
        }
        for id in exited {
            self.items.remove(&id);
        }
        Ok(events)
    }
}
