//! Nonblocking native process groups for trusted host features; plugins still use scoped APIs.
//!
//! This supervisor reuses the same pipes, PTYs, resize coalescing and process-tree retirement as
//! sandboxed callers. It owns native resources off the UI thread and never parses terminal output.

use crate::{faults::NativeDiagnostics, process::Processes};
use plugin_protocol::process::{Transport, Update};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

#[cfg(test)]
mod tests;

/// A literal launch requested by a trusted host interaction, with no shell interpolation.
#[derive(Clone, Debug)]
pub struct NativeLaunch {
    /// Program resolved by the common tool locator, never a shell command string.
    pub program: String,
    /// Literal arguments retain their individual boundaries on every platform.
    pub args: Vec<String>,
    /// Working directory supplied by the owning workspace interaction.
    pub cwd: String,
    /// Explicit environment entries; WASM itself still inherits no host environment.
    pub env: BTreeMap<String, String>,
    /// Pipes or PTY dimensions; only PTY consumers may resize or answer VT queries.
    pub transport: Transport,
}

/// A supervisor result retains the caller's stable session identity through startup and exit.
#[derive(Debug)]
pub enum NativeProcessEvent {
    Started {
        session: u64,
    },
    Update {
        session: u64,
        update: Update,
    },
    Failed {
        session: u64,
        message: String,
        launch: bool,
    },
}

enum Command {
    Launch(u64, u64, NativeLaunch),
    Write(u64, u64, Vec<u8>),
    Resize(u64, u64, u16, u16),
    Stop(u64, bool),
    Close(u64),
}

/// Authority changes cannot be dropped or revived by an older queued launch/input operation.
struct Authority {
    trusted: AtomicBool,
    revoked: AtomicBool,
    generation: AtomicU64,
}

/// A process retains the authority generation under which its native resources were created.
struct ProcessLease {
    id: u64,
    generation: u64,
}
impl Authority {
    fn check(&self, generation: u64) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.trusted.load(Ordering::Acquire)
                && self.generation.load(Ordering::Acquire) == generation,
            "Workspace authority was revoked"
        );
        Ok(())
    }
}

/// One bounded, revocable workspace supervisor. Dropping the transport closes its owned tree.
///
/// Host callers must supply the current workspace trust. This is not a plugin permission bypass:
/// guest launches continue through the runtime's negotiated process/service authority.
pub struct NativeProcessGroup {
    commands: mpsc::SyncSender<Command>,
    events: mpsc::Receiver<NativeProcessEvent>,
    authority: Arc<Authority>,
    /// One separate close gate stays available even when data/output queues apply backpressure.
    shutdown: mpsc::SyncSender<mpsc::Sender<()>>,
}

impl NativeProcessGroup {
    /// Start an idle supervisor; no child or PTY is created until an authorized launch arrives.
    pub fn new(trusted: bool) -> Self {
        let (commands, input) = mpsc::sync_channel(256);
        let (output, events) = mpsc::sync_channel(256);
        let authority = Arc::new(Authority {
            trusted: AtomicBool::new(trusted),
            revoked: AtomicBool::new(false),
            generation: AtomicU64::new(0),
        });
        let (shutdown, close_gate) = mpsc::sync_channel(1);
        let supervisor_authority = authority.clone();
        std::thread::spawn(move || supervise(input, output, close_gate, supervisor_authority));
        Self {
            commands,
            events,
            authority,
            shutdown,
        }
    }

    /// Revoke future launches synchronously, then retire already-owned processes on the supervisor.
    pub fn set_trusted(&self, trusted: bool) -> anyhow::Result<()> {
        self.authority.trusted.store(trusted, Ordering::Release);
        if !trusted {
            self.authority.generation.fetch_add(1, Ordering::AcqRel);
            self.authority.revoked.store(true, Ordering::Release);
        }
        Ok(())
    }

    /// Queue a bounded literal launch. Restricted callers are refused before any native side effect.
    pub fn launch(&self, session: u64, launch: NativeLaunch) -> anyhow::Result<()> {
        let generation = self.authority.generation.load(Ordering::Acquire);
        self.authority.check(generation)?;
        anyhow::ensure!(
            session > 0
                && !launch.program.is_empty()
                && launch.program.len() <= 4096
                && !launch.program.contains('\0'),
            "Invalid native program"
        );
        anyhow::ensure!(
            launch.args.len() <= 128
                && launch
                    .args
                    .iter()
                    .all(|arg| arg.len() <= 32768 && !arg.contains('\0'))
                && launch.args.iter().map(String::len).sum::<usize>() <= 65536,
            "Invalid native arguments"
        );
        anyhow::ensure!(
            launch.cwd.len() <= 4096
                && !launch.cwd.contains('\0')
                && launch.env.len() <= 64
                && launch.env.iter().all(|(name, value)| !name.is_empty()
                    && name.len() <= 128
                    && !name.contains('=')
                    && !name.chars().any(char::is_control)
                    && value.len() <= 32768
                    && !value.contains('\0')),
            "Invalid native environment or directory"
        );
        self.authority.check(generation)?;
        self.send(Command::Launch(session, generation, launch))
    }

    /// Queue literal stdin bytes with bounded backpressure; never block the UI on a slow child.
    pub fn write(&self, session: u64, bytes: Vec<u8>) -> anyhow::Result<()> {
        anyhow::ensure!(bytes.len() <= 65536, "Native input exceeds quota");
        let generation = self.authority.generation.load(Ordering::Acquire);
        self.authority.check(generation)?;
        self.send(Command::Write(session, generation, bytes))
    }

    /// Queue grid dimensions; the shared process implementation coalesces native drag bursts.
    pub fn resize(&self, session: u64, columns: u16, rows: u16) -> anyhow::Result<()> {
        let generation = self.authority.generation.load(Ordering::Acquire);
        self.authority.check(generation)?;
        self.send(Command::Resize(session, generation, columns, rows))
    }

    /// Request graceful exit or immediate tree termination; completion is a later process event.
    pub fn stop(&self, session: u64, force: bool) -> anyhow::Result<()> {
        self.send(Command::Stop(session, force))
    }

    /// Retire one session and its descendants without discarding other sessions' resources.
    pub fn close(&self, session: u64) -> anyhow::Result<()> {
        self.send(Command::Close(session))
    }

    /// Drain a bounded amount of output so a noisy child cannot monopolize one render frame.
    pub fn poll(&self) -> Vec<NativeProcessEvent> {
        self.events.try_iter().take(256).collect()
    }

    /// Revoke launch authority and acknowledge only after native tree/reader cleanup completes.
    /// The returned receiver may be waited on a background executor, never the UI thread.
    pub fn shutdown(&self) -> anyhow::Result<mpsc::Receiver<()>> {
        self.set_trusted(false)?;
        let (tx, rx) = mpsc::channel();
        self.shutdown
            .try_send(tx)
            .map_err(|_| anyhow::anyhow!("Native cleanup is already pending or closed"))?;
        Ok(rx)
    }

    fn send(&self, command: Command) -> anyhow::Result<()> {
        self.commands
            .try_send(command)
            .map_err(|_| anyhow::anyhow!("Native process supervisor is busy or closed"))
    }
}

/// Native resources stay on this thread; control gates are independent from data backpressure.
fn supervise(
    input: mpsc::Receiver<Command>,
    output: mpsc::SyncSender<NativeProcessEvent>,
    close_gate: mpsc::Receiver<mpsc::Sender<()>>,
    authority: Arc<Authority>,
) {
    let diagnostics = NativeDiagnostics::default();
    let mut processes = Processes::default();
    let mut sessions = BTreeMap::<u64, ProcessLease>::new();
    // At most one bounded native poll plus queued diagnostics are retained. A full view queue
    // pauses further reads, but never stops us from revoking or joining native resources.
    let mut pending = VecDeque::new();
    'supervisor: loop {
        if let Ok(acknowledge) = close_gate.try_recv() {
            processes.clear();
            let _ = acknowledge.send(());
            return;
        }
        if authority.revoked.swap(false, Ordering::AcqRel) {
            // A newer grant may already have launched a child while this thread was waiting
            // for input. Retire only obsolete leases, not resources of that newer authority.
            let generation = authority.generation.load(Ordering::Acquire);
            let obsolete: Vec<_> = sessions
                .iter()
                .filter_map(|(session, lease)| (lease.generation != generation).then_some(*session))
                .collect();
            for session in obsolete {
                let lease = sessions.remove(&session).expect("obsolete lease exists");
                let _ = processes.close(lease.id);
                pending.push_back(NativeProcessEvent::Update {
                    session,
                    update: Update::Terminated,
                });
            }
        }
        while let Some(event) = pending.pop_front() {
            match output.try_send(event) {
                Ok(()) => {}
                Err(mpsc::TrySendError::Full(event)) => {
                    pending.push_front(event);
                    break;
                }
                Err(mpsc::TrySendError::Disconnected(_)) => break 'supervisor,
            }
        }
        let command = if pending.len() < 512 {
            match input.recv_timeout(Duration::from_millis(10)) {
                Ok(command) => Some(command),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        } else {
            std::thread::park_timeout(Duration::from_millis(10));
            None
        };
        if let Some(command) = command {
            let launch_failed = matches!(command, Command::Launch(..));
            let (session, result) = match command {
                Command::Launch(session, generation, launch) => {
                    let result = (|| {
                        authority.check(generation)?;
                        anyhow::ensure!(
                            !sessions.contains_key(&session),
                            "Session already owns a process"
                        );
                        let reporter = diagnostics.reporter("host", "native-processes");
                        let id = match launch.transport {
                            Transport::Pty {
                                columns,
                                rows,
                                inherit_cursor,
                            } => processes.spawn(
                                launch.program,
                                launch.args,
                                launch.cwd,
                                columns,
                                rows,
                                inherit_cursor,
                                launch.env,
                                reporter,
                            )?,
                            Transport::Stdio => processes.spawn_stdio(
                                &crate::toolchains::resolve(&launch.program)?,
                                &launch.args,
                                std::path::Path::new(&launch.cwd),
                                &launch.env,
                            )?,
                        };
                        sessions.insert(session, ProcessLease { id, generation });
                        pending.push_back(NativeProcessEvent::Started { session });
                        Ok(())
                    })();
                    (session, result)
                }
                Command::Write(session, generation, bytes) => (
                    session,
                    authority.check(generation).and_then(|_| {
                        sessions
                            .get(&session)
                            .filter(|lease| lease.generation == generation)
                            .ok_or_else(|| anyhow::anyhow!("Session has no process"))
                            .and_then(|lease| processes.write(lease.id, &bytes))
                    }),
                ),
                Command::Resize(session, generation, columns, rows) => (
                    session,
                    authority.check(generation).and_then(|_| {
                        sessions
                            .get(&session)
                            .filter(|lease| lease.generation == generation)
                            .ok_or_else(|| anyhow::anyhow!("Session has no process"))
                            .and_then(|lease| processes.resize(lease.id, columns, rows))
                    }),
                ),
                Command::Stop(session, force) => (
                    session,
                    sessions
                        .get(&session)
                        .ok_or_else(|| anyhow::anyhow!("Session has no process"))
                        .and_then(|lease| {
                            if force {
                                processes.terminate(lease.id)
                            } else {
                                processes.request_exit(lease.id).map(|_| ())
                            }
                        }),
                ),
                Command::Close(session) => (
                    session,
                    sessions
                        .remove(&session)
                        .map_or(Ok(()), |lease| processes.close(lease.id)),
                ),
            };
            if let Err(error) = result {
                pending.push_back(NativeProcessEvent::Failed {
                    session,
                    message: format!("{error:#}"),
                    launch: launch_failed,
                });
            }
        }
        if !pending.is_empty() {
            continue;
        }
        match processes.poll_native() {
            Ok(updates) => {
                for (id, update) in updates {
                    if let Some(session) = sessions
                        .iter()
                        .find_map(|(session, process)| (process.id == id).then_some(*session))
                    {
                        if matches!(update, Update::Exited { .. } | Update::Terminated) {
                            sessions.remove(&session);
                        }
                        pending.push_back(NativeProcessEvent::Update { session, update });
                    }
                }
            }
            Err(error) => pending.push_back(NativeProcessEvent::Failed {
                session: 0,
                message: format!("{error:#}"),
                launch: false,
            }),
        }
    }
    processes.clear();
}
