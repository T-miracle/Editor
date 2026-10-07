//! Nonblocking exit policy keeps ownership through normal shutdown, escalation and native completion.
use super::*;
use plugin_protocol::process::ExitMode;
use std::time::{Duration, Instant};

/// Three seconds allow a responsive console program to clean up without delaying an explicit rerun.
/// This is an implementation decision for this batch, not the interview's illustrative five seconds.
pub const DEFAULT_STOP_GRACE_MS: u32 = 3_000;

/// A caller may shorten or extend normal cleanup within the bounded control contract.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StopOptions {
    /// Graceful sends the supported exit request first; force skips that request and its wait.
    pub mode: ExitMode,
    /// Graceful wait in milliseconds, in 1..=60_000; ignored for an explicit force request.
    pub grace_ms: u32,
}
impl Default for StopOptions {
    fn default() -> Self {
        Self {
            mode: ExitMode::Graceful,
            grace_ms: DEFAULT_STOP_GRACE_MS,
        }
    }
}

/// Only one control request per phase; repeating Stop never restarts its deadline.
pub(super) struct StopState {
    mode: ExitMode,
    since: Instant,
    grace_ms: u32,
    sent: bool,
    pending: Option<Completion<Value>>,
}
impl StopState {
    /// A stop's UI phase is distinct from an observation that the program actually ended.
    pub(super) fn phase(&self) -> ExecutionState {
        match self.mode {
            ExitMode::Graceful => ExecutionState::Stopping,
            ExitMode::Force => ExecutionState::Terminating,
        }
    }
}

impl Manager {
    /// Request normal exit, then force the owned tree after the documented default grace period.
    pub fn stop_execution(&mut self, session: u64) -> anyhow::Result<()> {
        self.stop_execution_with(session, StopOptions::default())
    }

    /// Request bounded graceful cleanup or immediate termination of this session's original provider.
    ///
    /// Acceptance does not claim completion. A starting request remains owned: if no provider identity
    /// arrives before the stop deadline, its individual delegation lifetime is revoked and cleaned up.
    pub fn stop_execution_with(
        &mut self,
        session: u64,
        options: StopOptions,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            options.mode == ExitMode::Force || (1..=60_000).contains(&options.grace_ms),
            "Graceful wait must be in 1..=60000 milliseconds"
        );
        let execution = self
            .execution(session)
            .ok_or_else(|| anyhow::anyhow!("Unknown execution session {session}"))?;
        if execution.state() == ExecutionState::Exited {
            return Ok(());
        }
        anyhow::ensure!(
            execution.stoppable(),
            "Execution session {session} has no available owner"
        );
        {
            let mut stop = execution.stop.lock().unwrap();
            if stop.is_none()
                || (options.mode == ExitMode::Force
                    && stop
                        .as_ref()
                        .is_some_and(|state| state.mode != ExitMode::Force))
            {
                *stop = Some(StopState {
                    mode: options.mode,
                    since: Instant::now(),
                    grace_ms: options.grace_ms,
                    sent: false,
                    pending: None,
                });
            }
        }
        self.drive_execution_stop(&execution);
        Ok(())
    }

    /// Drive all outstanding controls once per poll; no UI frame waits for a shutdown timeout.
    pub(super) fn poll_execution_stops(&mut self) {
        for execution in self.executions() {
            if execution.snapshot().state == ExecutionState::Failed {
                // Failed/timed-out starts can have created side effects before their reply failed.
                execution.execution_alive.store(false, Ordering::Release);
            } else if execution.stop.lock().unwrap().is_some() {
                self.drive_execution_stop(&execution);
            }
        }
    }

    /// Escalate once, then retain the forced phase until real exit or revocation is observed.
    fn drive_execution_stop(&mut self, execution: &HostExecution) {
        if !execution.snapshot().state.is_active() {
            return;
        }
        let Some((mode, since, grace_ms, sent, failed)) =
            execution.stop.lock().unwrap().as_ref().map(|state| {
                let failed = state.pending.as_ref().is_some_and(|pending| {
                    matches!(
                        pending.status(),
                        RequestUpdate::Completed { result: Err(_) }
                            | RequestUpdate::Cancelled { .. }
                    )
                });
                (state.mode, state.since, state.grace_ms, state.sent, failed)
            })
        else {
            return;
        };
        let has_identity = execution.snapshot().provider_session.is_some();
        if !has_identity {
            let limit = if mode == ExitMode::Force {
                Duration::ZERO
            } else {
                Duration::from_millis(grace_ms.into())
            };
            if since.elapsed() >= limit {
                self.fail_execution_stop(
                    execution,
                    "Stopped before the provider reported its session identity",
                );
            }
            return;
        }
        if mode == ExitMode::Graceful
            && (failed || since.elapsed() >= Duration::from_millis(grace_ms.into()))
        {
            // An unsupported or refused normal exit goes directly to the explicitly visible force phase.
            *execution.stop.lock().unwrap() = Some(StopState {
                mode: ExitMode::Force,
                since: Instant::now(),
                grace_ms,
                sent: false,
                pending: None,
            });
            self.drive_execution_stop(execution);
            return;
        }
        if mode == ExitMode::Force
            && (failed
                || since.elapsed() >= Duration::from_millis(EXECUTION_STOP_TIMEOUT_MS.into()))
        {
            self.fail_execution_stop(
                execution,
                "Provider could not confirm forceful termination; session resources revoked",
            );
            return;
        }
        if !sent {
            match self.enqueue_execution_stop(execution, mode) {
                Ok(pending) => {
                    if let Some(state) = execution.stop.lock().unwrap().as_mut() {
                        state.sent = true;
                        state.pending = Some(pending);
                    }
                }
                Err(_) if mode == ExitMode::Graceful => {
                    *execution.stop.lock().unwrap() = Some(StopState {
                        mode: ExitMode::Force,
                        since: Instant::now(),
                        grace_ms,
                        sent: false,
                        pending: None,
                    });
                    self.drive_execution_stop(execution);
                }
                Err(error) => self.fail_execution_stop(
                    execution,
                    &format!("Cannot terminate session: {error:#}"),
                ),
            }
        }
    }

    /// Revoking the session's own token kills its delegated resources without retiring other sessions.
    fn fail_execution_stop(&self, execution: &HostExecution, reason: &str) {
        *execution.observation_failure.lock().unwrap() = Some(ExecutionFailure {
            code: ErrorCode::Cancelled,
            message: reason.into(),
        });
        execution.execution_alive.store(false, Ordering::Release);
    }

    /// Control authority and target incarnation are taken from the original launch, never defaults.
    fn enqueue_execution_stop(
        &mut self,
        execution: &HostExecution,
        mode: ExitMode,
    ) -> anyhow::Result<Completion<Value>> {
        let provider_session = execution
            .snapshot()
            .provider_session
            .ok_or_else(|| anyhow::anyhow!("Provider reported no session identity"))?;
        let dependency = execution_dependency().map_err(start_failure)?;
        self.refresh_services();
        let caller = &execution.origin.caller;
        let reference = self
            .plugin_services
            .lock()
            .unwrap()
            .resolve_pinned(
                caller,
                EXECUTION_CONTRACT,
                &dependency,
                execution.provider_instance(),
            )
            .map_err(start_failure)?;
        let mut completion = Completion::new(EXECUTION_STOP_TIMEOUT_MS);
        completion.lifetimes = execution.origin.lifetimes.clone();
        let mut call = host_method_call(
            caller,
            reference,
            "stop",
            serde_json::json!({ "session": provider_session, "mode": mode }),
            &dependency,
            completion.clone(),
            self.host_alive.clone(),
        )
        .map_err(start_failure)?;
        call.context = execution
            .origin
            .delegate(&call.reference.provider, &call.signature)
            .map_err(start_failure)?;
        call.completion.lifetimes = call.context.lifetimes.clone();
        self.plugin_services
            .lock()
            .unwrap()
            .enqueue(call)
            .map_err(start_failure)?;
        Ok(completion)
    }
}
