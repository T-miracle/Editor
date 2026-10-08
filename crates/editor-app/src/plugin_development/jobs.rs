//! Host jobs publish into the existing run session list while owning their complete child trees.
use crate::extensions::HostRunSnapshot;
use plugin_runtime::{
    ExecutionState,
    development::{HostProcess, Update},
};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

enum Control {
    Reload,
    Stop,
    Force,
}
struct Job {
    state: Arc<Mutex<Report>>,
    control: mpsc::Sender<Control>,
    thread: Option<std::thread::JoinHandle<()>>,
}
#[derive(Clone)]
pub(crate) struct Report {
    pub snapshot: HostRunSnapshot,
    pub output: String,
}
/// Every job has a unique native session identity; ended histories are bounded and independently owned.
#[derive(Default)]
pub(crate) struct Jobs {
    jobs: BTreeMap<String, Job>,
}
impl Jobs {
    /// Start only after configuration/trust/permission validation; arguments stay literal.
    pub(crate) fn start(
        &mut self,
        config: &str,
        request: u64,
        args: Vec<String>,
        cwd: &Path,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.active(config),
            "This configuration is already running"
        );
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1 << 63);
        let state = Arc::new(Mutex::new(Report {
            snapshot: HostRunSnapshot {
                id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                config: config.into(),
                request_id: request,
                plugin: super::configuration::PROVIDER.into(),
                state: ExecutionState::Starting,
                provider_session: None,
                failure: None,
            },
            output: String::new(),
        }));
        let output = state.clone();
        let (control, receiver) = mpsc::channel();
        let executable = std::env::current_exe()?;
        let cwd = cwd.to_owned();
        let thread = std::thread::Builder::new()
            .name("nanobug-plugin-development".into())
            .spawn(move || {
                let result = (|| -> anyhow::Result<()> {
                    let mut child = HostProcess::spawn(&executable, &args, &cwd, &BTreeMap::new())?;
                    output.lock().unwrap().snapshot.state = ExecutionState::Running;
                    let mut stopping = None::<Instant>;
                    let mut stop_requested = false;
                    loop {
                        for control in receiver.try_iter() {
                            match control {
                                Control::Reload => child.write(b"reload\n")?,
                                Control::Stop => {
                                    stop_requested = true;
                                    if stopping.is_some() {
                                        continue;
                                    }
                                    output.lock().unwrap().snapshot.state =
                                        ExecutionState::Stopping;
                                    let _ = child.write(b"stop\n");
                                    stopping = Some(Instant::now());
                                }
                                Control::Force => {
                                    stop_requested = true;
                                    output.lock().unwrap().snapshot.state =
                                        ExecutionState::Terminating;
                                    child.terminate()?;
                                    stopping = None;
                                }
                            }
                        }
                        if stopping.is_some_and(|time| time.elapsed() > Duration::from_secs(2)) {
                            output.lock().unwrap().snapshot.state = ExecutionState::Terminating;
                            child.terminate()?;
                            stopping = None;
                        }
                        for event in child.poll()? {
                            match event {
                                Update::Output { bytes, .. } => {
                                    append(&mut output.lock().unwrap().output, &bytes)
                                }
                                Update::Exited { code } => {
                                    if code != 0 && !stop_requested {
                                        anyhow::bail!("Plugin job exited with code {code}");
                                    }
                                    return Ok(());
                                }
                                Update::Terminated => return Ok(()),
                            }
                        }
                        std::thread::sleep(Duration::from_millis(15));
                    }
                })();
                let mut report = output.lock().unwrap();
                match result {
                    Ok(()) => report.snapshot.state = ExecutionState::Exited,
                    Err(error) => {
                        let message = format!("{error:#}");
                        append(&mut report.output, message.as_bytes());
                        report.snapshot.failure = Some(message);
                        report.snapshot.state = ExecutionState::Failed;
                    }
                }
            })?;
        self.jobs.insert(
            config.into(),
            Job {
                state,
                control,
                thread: Some(thread),
            },
        );
        // Joining already-ended workers is nonblocking; retain at most 64 histories.
        let ended = self
            .jobs
            .iter()
            .filter(|(_, job)| !job.state.lock().unwrap().snapshot.state.is_active())
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for key in ended.into_iter().take(self.jobs.len().saturating_sub(64)) {
            if let Some(mut job) = self.jobs.remove(&key) {
                if let Some(thread) = job.thread.take() {
                    let _ = thread.join();
                }
            }
        }
        Ok(())
    }
    /// Whether this configuration still owns a live process tree.
    pub(crate) fn active(&self, id: &str) -> bool {
        self.jobs
            .get(id)
            .is_some_and(|job| job.state.lock().unwrap().snapshot.state.is_active())
    }
    /// Join host jobs to the existing run-session publication without exposing mutable state.
    pub(crate) fn snapshots(&self) -> Vec<HostRunSnapshot> {
        self.jobs
            .values()
            .map(|job| job.state.lock().unwrap().snapshot.clone())
            .collect()
    }
    /// Copy the bounded output and latest native state for the selected configuration.
    pub(crate) fn report(&self, id: &str) -> Option<Report> {
        self.jobs
            .get(id)
            .map(|job| job.state.lock().unwrap().clone())
    }
    /// Ask the controller to prepare a replacement; failure leaves its current plugin alive.
    pub(crate) fn reload(&self, id: &str) {
        if let Some(job) = self.jobs.get(id) {
            let _ = job.control.send(Control::Reload);
        }
    }
    /// Request graceful cleanup or immediate termination of the complete owned tree.
    pub(crate) fn stop(&self, id: &str, force: bool) -> bool {
        if let Some(job) = self.jobs.get(id) {
            let _ = job
                .control
                .send(if force { Control::Force } else { Control::Stop });
            return true;
        }
        false
    }
    /// Revoke every native job when the workspace loses trust or its window retires.
    pub(crate) fn stop_all(&self) {
        for job in self.jobs.values() {
            let _ = job.control.send(Control::Stop);
        }
    }
}
impl Drop for Jobs {
    fn drop(&mut self) {
        self.stop_all();
        for job in self.jobs.values_mut() {
            if let Some(thread) = job.thread.take() {
                let _ = thread.join();
            }
        }
    }
}
/// Retain recent complete UTF-8 output under a fixed byte budget, independent of process volume.
fn append(output: &mut String, bytes: &[u8]) {
    output.push_str(&String::from_utf8_lossy(bytes));
    if output.len() > 512 * 1024 {
        let mut start = output.len() - 512 * 1024;
        while !output.is_char_boundary(start) {
            start += 1;
        }
        output.drain(..start);
    }
}
