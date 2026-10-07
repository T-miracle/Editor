//! Host protocols borrow approved native services; retiring the package revokes every borrowed process.
use crate::process::{Spawned, spawn_piped};
mod completion;
pub(crate) use completion::CompletionHook;
use plugin_protocol::language::Provider;
use std::{
    path::PathBuf,
    process::{ChildStdin, ChildStdout},
    sync::{Arc, Condvar, Mutex, Weak},
    time::Duration,
};

/// Validated immutable plan. Consumers cannot mutate the command behind an approved lease.
pub struct LanguageService {
    pub owner: String,
    pub provider: Provider,
    pub root: PathBuf,
    pub(crate) program: PathBuf,
    pub(crate) args: Vec<String>,
    state: Mutex<Lease>,
    /// The host receives the same process-local sink as the package's WASM and native diagnostics.
    runtime_logs: crate::RuntimeLogs,
    /// Active plans pin files even if another workspace removes the package's installation record.
    pub(crate) dependencies: Vec<Arc<std::fs::File>>,
    /// Optional stateless worker uses the same immutable provider lease as the native service.
    pub(crate) completion: Option<CompletionHook>,
}
#[derive(Default)]
struct Lease {
    retired: bool,
    children: Vec<LeasedChild>,
}

/// Each launch has its own reader cutoff; stopping one process cannot silence a later replacement.
struct LeasedChild {
    child: Weak<Mutex<Spawned>>,
    stderr: Arc<StderrPublication>,
}

/// A stopped provider cannot revive alerts, while a failed one can finish its final physical line.
#[derive(Default)]
struct StderrPublication {
    state: Mutex<StderrState>,
    drained: Condvar,
}

#[derive(Default)]
struct StderrState {
    accepting: bool,
    finished: bool,
}

impl StderrPublication {
    /// Hold the authority lock through append so a completed revocation cannot be followed by a record.
    fn publish(
        &self,
        logs: &crate::RuntimeLogs,
        owner: &str,
        level: crate::LogLevel,
        source: &str,
        message: String,
    ) {
        let state = self.state.lock().unwrap();
        if state.accepting {
            logs.append(owner, level, source, message);
        }
    }

    /// Explicit revocation takes precedence over any concurrent failure-drain wait.
    fn revoke(&self) {
        self.state.lock().unwrap().accepting = false;
        self.drained.notify_all();
    }

    /// Called after EOF has published an unterminated tail, not merely after the child exits.
    fn finish(&self) {
        let mut state = self.state.lock().unwrap();
        state.finished = true;
        state.accepting = false;
        self.drained.notify_all();
    }

    /// Wait only for this launch's reader; an inherited pipe in another process cannot stall teardown.
    fn finish_failure(&self, timeout: Duration) {
        let state = self.state.lock().unwrap();
        let (mut state, _) = self
            .drained
            .wait_timeout_while(state, timeout, |state| state.accepting && !state.finished)
            .unwrap();
        state.accepting = false;
    }
}

/// A transport may outlive a UI task, but it cannot outlive its package authority.
pub struct ServiceProcess {
    child: Arc<Mutex<Spawned>>,
    stderr: Arc<StderrPublication>,
}
impl ServiceProcess {
    /// Revoke output before stopping a deliberately cancelled or retired provider.
    /// Later buffered bytes are drained without producing records or new anomaly reminders.
    pub fn stop(&self) {
        // Serialize stopping against stderr publication before terminating the underlying process.
        self.stderr.revoke();
        self.terminate();
    }

    /// Preserve an actual failure's last stderr line before revoking its output authority.
    /// The reader has at most 200 ms after child reaping; an earlier user/package stop still wins.
    pub fn stop_after_failure(&self) {
        self.terminate();
        self.stderr.finish_failure(Duration::from_millis(200));
    }

    /// Both stop reasons terminate the same approved job and reap it before releasing the lease.
    fn terminate(&self) {
        let mut child = self.child.lock().unwrap();
        #[cfg(windows)]
        child.job.terminate();
        let _ = child.child.kill();
        let _ = child.child.wait();
    }
}
impl Drop for ServiceProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

impl LanguageService {
    pub(crate) fn new(
        owner: String,
        provider: Provider,
        root: PathBuf,
        program: PathBuf,
        args: Vec<String>,
        runtime_logs: crate::RuntimeLogs,
    ) -> Self {
        Self {
            owner,
            provider,
            root,
            program,
            args,
            state: Mutex::new(Lease::default()),
            runtime_logs,
            dependencies: vec![],
            completion: None,
        }
    }
    /// Borrow the shared run history without transferring authority to launch or modify this plan.
    pub fn runtime_logs(&self) -> crate::RuntimeLogs {
        self.runtime_logs.clone()
    }
    /// Publish a host-owned transport message only while both package and view authority are live.
    /// The lease lock serializes this append with retirement; rejected late callbacks return `None`.
    pub fn append_runtime_log(
        &self,
        retired: &std::sync::atomic::AtomicBool,
        level: crate::LogLevel,
        source: &str,
        message: impl Into<String>,
    ) -> Option<u64> {
        let state = self.state.lock().unwrap();
        if state.retired || retired.load(std::sync::atomic::Ordering::Acquire) {
            return None;
        }
        Some(
            self.runtime_logs
                .append(&self.owner, level, source, message),
        )
    }
    /// This lock serializes launch against revocation; no child can escape the retirement boundary.
    pub fn spawn(&self) -> anyhow::Result<(ServiceProcess, ChildStdin, ChildStdout)> {
        self.spawn_for_owner(&std::sync::atomic::AtomicBool::new(false))
    }
    /// A view's retirement flag shares this launch lock with stop_processes, preventing late starts.
    pub fn spawn_for_owner(
        &self,
        retired: &std::sync::atomic::AtomicBool,
    ) -> anyhow::Result<(ServiceProcess, ChildStdin, ChildStdout)> {
        let mut state = self.state.lock().unwrap();
        anyhow::ensure!(
            !state.retired && !retired.load(std::sync::atomic::Ordering::Acquire),
            "LSP provider has been retired"
        );
        state
            .children
            .retain(|lease| lease.child.strong_count() > 0);
        anyhow::ensure!(
            state.children.is_empty(),
            "LSP provider already has a live process"
        );
        let mut child = spawn_piped(&self.program, &self.args, &self.root, &Default::default())?;
        let input = child.child.stdin.take().expect("piped stdin");
        let output = child.child.stdout.take().expect("piped stdout");
        let mut errors = child.child.stderr.take().expect("piped stderr");
        // stderr has no JSON-RPC frames: capture bounded lines without ever treating stdout as log data.
        let logs = self.runtime_logs.clone();
        let owner = self.owner.clone();
        let source = format!("lsp/{}/stderr", self.provider.id);
        let stderr = Arc::new(StderrPublication {
            state: Mutex::new(StderrState {
                accepting: true,
                finished: false,
            }),
            drained: Condvar::new(),
        });
        let reader_stderr = stderr.clone();
        std::thread::spawn(move || {
            crate::logs::drain(
                &mut errors,
                |level, source, message| {
                    // Continue draining after stop, but old buffered bytes cannot revive an anomaly reminder.
                    reader_stderr.publish(&logs, &owner, level, source, message);
                },
                crate::LogLevel::Warning,
                &source,
            );
            // stdout EOF can reach the transport first; completion includes stderr's own final flush.
            reader_stderr.finish();
        });
        let child = Arc::new(Mutex::new(child));
        state.children.push(LeasedChild {
            child: Arc::downgrade(&child),
            stderr: stderr.clone(),
        });
        Ok((ServiceProcess { child, stderr }, input, output))
    }
    pub fn is_active(&self) -> bool {
        !self.state.lock().unwrap().retired
    }
    /// Consent and lifecycle diagnostics include host-owned transports as well as guest-created processes.
    pub fn process_count(&self) -> usize {
        self.state
            .lock()
            .unwrap()
            .children
            .iter()
            .filter(|lease| lease.child.strong_count() > 0)
            .count()
    }
    pub(crate) fn same_plan(&self, other: &Self) -> bool {
        self.owner == other.owner
            && self.provider == other.provider
            && self.root == other.root
            && self.program == other.program
            && self.args == other.args
            && self.completion.as_ref().map(|hook| &hook.settings)
                == other.completion.as_ref().map(|hook| &hook.settings)
    }
    pub(crate) fn retire(&self) {
        let mut state = self.state.lock().unwrap();
        state.retired = true;
        for lease in state.children.drain(..) {
            if let Some(child) = lease.child.upgrade() {
                ServiceProcess {
                    child,
                    stderr: lease.stderr,
                }
                .stop();
            }
        }
        // Mark retired before waiting for a bounded pure call; its eventual result is rejected.
        drop(state);
        if let Some(hook) = &self.completion
            && let Some(mut instance) = hook.instance.lock().unwrap().take()
        {
            instance.stop();
        }
    }
    /// Selection changes stop the current transport while keeping the installed startup plan reusable.
    pub fn stop_processes(&self) {
        let mut state = self.state.lock().unwrap();
        for lease in state.children.drain(..) {
            if let Some(child) = lease.child.upgrade() {
                ServiceProcess {
                    child,
                    stderr: lease.stderr,
                }
                .stop();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LogLevel, RuntimeLogs};

    /// A fault teardown waits for the reader's EOF flush, then rejects further buffered callbacks.
    #[test]
    fn failure_wait_preserves_stderr_tail_then_revokes_publication() {
        let logs = RuntimeLogs::default();
        let publication = Arc::new(StderrPublication {
            state: Mutex::new(StderrState {
                accepting: true,
                finished: false,
            }),
            drained: Condvar::new(),
        });
        let waiting = publication.clone();
        let (started, begin) = std::sync::mpsc::channel();
        let (finished, done) = std::sync::mpsc::channel();
        let stop = std::thread::spawn(move || {
            started.send(()).unwrap();
            waiting.finish_failure(Duration::from_secs(2));
            finished.send(()).unwrap();
        });
        begin.recv().unwrap();
        // The unterminated line is published before the reader reports completion.
        publication.publish(
            &logs,
            "owner",
            LogLevel::Warning,
            "lsp/analysis/stderr",
            "crash tail".into(),
        );
        publication.finish();
        done.recv_timeout(Duration::from_secs(1)).unwrap();
        stop.join().unwrap();
        publication.publish(
            &logs,
            "owner",
            LogLevel::Warning,
            "lsp/analysis/stderr",
            "late tail".into(),
        );
        assert_eq!(logs.records("owner").len(), 1);
        assert_eq!(logs.records("owner")[0].message, "crash tail");
    }

    /// A user/package revocation wakes failure teardown and wins over a pending final line.
    #[test]
    fn controlled_revocation_wakes_failure_wait_without_reviving_a_reminder() {
        let logs = RuntimeLogs::default();
        let publication = Arc::new(StderrPublication {
            state: Mutex::new(StderrState {
                accepting: true,
                finished: false,
            }),
            drained: Condvar::new(),
        });
        let waiting = publication.clone();
        let (started, begin) = std::sync::mpsc::channel();
        let (finished, done) = std::sync::mpsc::channel();
        let stop = std::thread::spawn(move || {
            started.send(()).unwrap();
            waiting.finish_failure(Duration::from_secs(10));
            finished.send(()).unwrap();
        });
        begin.recv().unwrap();
        publication.revoke();
        done.recv_timeout(Duration::from_secs(1)).unwrap();
        stop.join().unwrap();
        publication.publish(
            &logs,
            "owner",
            LogLevel::Warning,
            "lsp/analysis/stderr",
            "old buffered tail".into(),
        );
        publication.finish();
        assert!(logs.records("owner").is_empty());
        assert!(logs.pending_reminders().is_empty());
    }

    /// Descendants retaining a pipe cannot keep a failed reader authorized after its finite drain budget.
    #[test]
    fn failure_timeout_revokes_an_unfinished_reader() {
        let publication = StderrPublication {
            state: Mutex::new(StderrState {
                accepting: true,
                finished: false,
            }),
            drained: Condvar::new(),
        };
        publication.finish_failure(Duration::ZERO);
        let logs = RuntimeLogs::default();
        publication.publish(
            &logs,
            "owner",
            LogLevel::Warning,
            "lsp/analysis/stderr",
            "outlived failure drain".into(),
        );
        assert!(logs.records("owner").is_empty());
    }

    /// Both host-view retirement and package revocation reject late transport messages in the owner sink.
    #[test]
    fn runtime_log_publication_respects_view_and_package_retirement() {
        let logs = RuntimeLogs::default();
        let provider = serde_json::from_value(serde_json::json!({
            "id":"analysis", "language":"fixture-language", "service":"analysis"
        }))
        .unwrap();
        let plan = LanguageService::new(
            "owner".into(),
            provider,
            PathBuf::new(),
            PathBuf::new(),
            vec![],
            logs.clone(),
        );
        let retired = std::sync::atomic::AtomicBool::new(false);
        let id = plan
            .append_runtime_log(&retired, LogLevel::Info, "lsp/analysis", "ready")
            .unwrap();
        assert_eq!(plan.runtime_logs().records("owner")[0].id, id);
        retired.store(true, std::sync::atomic::Ordering::Release);
        assert!(
            plan.append_runtime_log(
                &retired,
                LogLevel::Error,
                "lsp/analysis",
                "late host result"
            )
            .is_none()
        );
        retired.store(false, std::sync::atomic::Ordering::Release);
        plan.retire();
        assert!(
            plan.append_runtime_log(
                &retired,
                LogLevel::Error,
                "lsp/analysis",
                "late package result"
            )
            .is_none()
        );
        assert_eq!(logs.records("owner").len(), 1);
        assert!(logs.pending_reminders().is_empty());
    }
}
