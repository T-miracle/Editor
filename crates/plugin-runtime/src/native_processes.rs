//! Nonblocking native process groups for trusted host features; plugins still use scoped APIs.
//!
//! This supervisor reuses the same pipes, PTYs, resize coalescing and process-tree retirement as
//! sandboxed callers. It owns native resources off the UI thread and never parses terminal output.

use crate::{faults::NativeDiagnostics, process::Processes};
use plugin_protocol::process::{Transport, Update};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

#[cfg(test)]
mod tests;

/// Locate a tool without launching or installing it; host discovery and native launches share this policy.
pub fn resolve_program(program: &str) -> anyhow::Result<std::path::PathBuf> {
    crate::toolchains::resolve(program)
}

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
    Launch(u64, u64, Arc<AtomicU64>, NativeLaunch),
    Write(u64, u64, Vec<u8>),
    Resize(u64, u64, u16, u16),
    Stop(u64),
}

/// Authority changes cannot be dropped or revived by an older queued launch/input operation.
struct Authority {
    trusted: AtomicBool,
    generation: AtomicU64,
}

/// A process retains the authority generation under which its native resources were created.
struct ProcessLease {
    id: u64,
    generation: u64,
    /// Each explicit retirement advances independently of the bounded data queue.
    retirement: Arc<AtomicU64>,
    /// Remember attempted termination so OS refusals do not create an unbounded diagnostic loop.
    retired_revision: u64,
    authority_retired: bool,
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
    /// Only admitted launches have entries; completed/failed launches release these bounded gates.
    retirements: Arc<Mutex<BTreeMap<u64, Arc<AtomicU64>>>>,
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
            generation: AtomicU64::new(0),
        });
        let (shutdown, close_gate) = mpsc::sync_channel(1);
        let supervisor_authority = authority.clone();
        let retirements = Arc::new(Mutex::new(BTreeMap::new()));
        let supervisor_retirements = retirements.clone();
        std::thread::spawn(move || {
            supervise(
                input,
                output,
                close_gate,
                supervisor_authority,
                supervisor_retirements,
            )
        });
        Self {
            commands,
            events,
            authority,
            retirements,
            shutdown,
        }
    }

    /// Revoke future launches synchronously, then retire already-owned processes on the supervisor.
    pub fn set_trusted(&self, trusted: bool) -> anyhow::Result<()> {
        self.authority.trusted.store(trusted, Ordering::Release);
        if !trusted {
            self.authority.generation.fetch_add(1, Ordering::AcqRel);
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
        let retirement = Arc::new(AtomicU64::new(0));
        {
            let mut gates = self.retirements.lock().unwrap();
            anyhow::ensure!(gates.len() < 512, "Native session quota exceeded");
            anyhow::ensure!(
                !gates.contains_key(&session),
                "Session already owns a process"
            );
            gates.insert(session, retirement.clone());
        }
        let result = self.send(Command::Launch(session, generation, retirement, launch));
        if result.is_err() {
            self.retirements.lock().unwrap().remove(&session);
        }
        result
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
        if force {
            self.close(session)
        } else {
            self.send(Command::Stop(session))
        }
    }

    /// Retire one session and its descendants without discarding other sessions' resources.
    pub fn close(&self, session: u64) -> anyhow::Result<()> {
        // Coalesce retirement in a per-launch gate, including launches that are still queued.
        // Unknown or already-ended sessions add no tombstone and cannot exhaust metadata.
        if let Some(retirement) = self.retirements.lock().unwrap().get(&session) {
            retirement.fetch_add(1, Ordering::AcqRel);
        }
        Ok(())
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
    retirements: Arc<Mutex<BTreeMap<u64, Arc<AtomicU64>>>>,
) {
    let diagnostics = NativeDiagnostics::default();
    let mut processes = Processes::default();
    let mut sessions = BTreeMap::<u64, ProcessLease>::new();
    // At most one bounded native poll plus queued diagnostics are retained. A full view queue
    // pauses further reads, but never stops us from revoking or joining native resources.
    let mut pending = VecDeque::new();
    'supervisor: loop {
        if let Ok(acknowledge) = close_gate.try_recv() {
            if processes.clear_checked().is_ok() {
                let _ = acknowledge.send(());
            }
            // Failure drops the acknowledgement; callers retain activity instead of claiming exit.
            return;
        }
        // Trust revocation and individual retirement bypass every data/output queue. A newer
        // authority grant only preserves leases admitted under that newer generation.
        let generation = authority.generation.load(Ordering::Acquire);
        for (&session, lease) in &mut sessions {
            let revision = lease.retirement.load(Ordering::Acquire);
            let obsolete = lease.generation != generation && !lease.authority_retired;
            if obsolete || revision != lease.retired_revision {
                lease.authority_retired |= obsolete;
                lease.retired_revision = revision;
                // Keep the original native handle until real exit and EOF, even on OS refusal.
                if let Err(error) = processes.terminate(lease.id) {
                    pending.push_back(NativeProcessEvent::Failed {
                        session,
                        message: format!("{error:#}"),
                        launch: false,
                    });
                }
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
                Command::Launch(session, generation, retirement, launch) => {
                    let result = (|| {
                        authority.check(generation)?;
                        anyhow::ensure!(
                            retirement.load(Ordering::Acquire) == 0,
                            "Native launch was retired"
                        );
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
                        sessions.insert(
                            session,
                            ProcessLease {
                                id,
                                generation,
                                retirement,
                                retired_revision: 0,
                                authority_retired: false,
                            },
                        );
                        pending.push_back(NativeProcessEvent::Started { session });
                        Ok(())
                    })();
                    if result.is_err() {
                        retirements.lock().unwrap().remove(&session);
                    }
                    (session, result)
                }
                Command::Write(session, generation, bytes) => (
                    session,
                    authority.check(generation).and_then(|_| {
                        sessions
                            .get(&session)
                            .filter(|lease| {
                                lease.generation == generation
                                    && lease.retirement.load(Ordering::Acquire) == 0
                            })
                            .ok_or_else(|| anyhow::anyhow!("Session has no process"))
                            .and_then(|lease| processes.write(lease.id, &bytes))
                    }),
                ),
                Command::Resize(session, generation, columns, rows) => (
                    session,
                    authority.check(generation).and_then(|_| {
                        sessions
                            .get(&session)
                            .filter(|lease| {
                                lease.generation == generation
                                    && lease.retirement.load(Ordering::Acquire) == 0
                            })
                            .ok_or_else(|| anyhow::anyhow!("Session has no process"))
                            .and_then(|lease| processes.resize(lease.id, columns, rows))
                    }),
                ),
                Command::Stop(session) => (
                    session,
                    sessions
                        .get(&session)
                        .ok_or_else(|| anyhow::anyhow!("Session has no process"))
                        .and_then(|lease| processes.request_exit(lease.id).map(|_| ())),
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
        for (id, result) in processes.poll_native_observed() {
            if let Some(session) = sessions
                .iter()
                .find_map(|(session, process)| (process.id == id).then_some(*session))
            {
                let update = match result {
                    Ok(update) => update,
                    Err(error) => {
                        pending.push_back(NativeProcessEvent::Failed {
                            session,
                            message: format!("{error:#}"),
                            launch: false,
                        });
                        continue;
                    }
                };
                if matches!(update, Update::Exited { .. } | Update::Terminated) {
                    sessions.remove(&session);
                    retirements.lock().unwrap().remove(&session);
                }
                pending.push_back(NativeProcessEvent::Update { session, update });
            }
        }
    }
    processes.clear();
}
