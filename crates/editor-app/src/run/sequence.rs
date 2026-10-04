//! Ordered preparation before a program starts: build actions, then pre-launch steps, then the run.
//!
//! The sequence owns no process. Every step is an ordinary execution session, so the provider keeps
//! its authority over the programs it started and the host never borrows a provider-private handle.
//! A step's completion condition is the provider's observation of an exit, never elapsed time and
//! never text the host happens to have seen.

use super::{RunPlan, RunSession, StepKind};
use std::collections::BTreeMap;

#[cfg(test)]
#[path = "sequence_tests.rs"]
mod tests;

/// What one step of a sequence is doing right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepState {
    /// Queued but not yet requested from the runtime.
    Waiting,
    /// Requested; the runtime has not published a session identity yet.
    Starting,
    /// The provider confirmed the program exists; its exit has not been observed.
    Running {
        session: u64,
        provider_session: Option<String>,
    },
    /// The program ended successfully, so the sequence may continue.
    Succeeded,
    /// The program ended unsuccessfully, or could not be started; the sequence is blocked.
    Failed {
        /// What was observed: an exit code, a provider failure, or an unreachable session.
        reason: String,
    },
    /// The user stopped this step; the sequence is blocked and does not resume on its own.
    Stopped,
}

/// How one step ended, as a provider reported it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepOutcome {
    /// The program ended with this status; zero is success.
    Exited { code: u32 },
    /// The program was terminated rather than ending on its own.
    Terminated,
    /// The provider could not report, or the session is gone without a status.
    Unknown,
}

impl StepState {
    /// Whether the sequence has to wait for this step before it may continue.
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Waiting | Self::Starting | Self::Running { .. })
    }

    /// Whether this step blocks every later step.
    pub fn is_blocking(&self) -> bool {
        matches!(self, Self::Failed { .. } | Self::Stopped)
    }
}

/// One step's identity plus its state, so output and status can name the step it belongs to.
#[derive(Clone, Debug)]
pub struct SequenceStep {
    pub kind: StepKind,
    pub name: String,
    /// The configuration whose stored definition produced this step.
    pub config: String,
    pub state: StepState,
}

/// A launch being prepared, or a build being run on its own.
#[derive(Clone, Debug)]
pub struct RunSequence {
    /// The configuration the user acted on; the program step belongs to it.
    pub config: String,
    /// Whether the program step runs after the preparation steps.
    pub launches_program: bool,
    steps: Vec<SequenceStep>,
    current: usize,
    /// Names the blocking step once the sequence has stopped for good.
    blocked: Option<String>,
    /// Set while a stop has been requested for the step now running.
    stopping: bool,
}

/// What the host should do next for a sequence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SequenceAction {
    /// Request this step, identified by the configuration and its index in the plan.
    Start { index: usize },
    /// Stop the session this step owns, because the user asked to stop the sequence.
    Stop { session: u64 },
    /// The sequence finished; the program is running or the build alone is complete.
    Done,
    /// The sequence is blocked; the reason names the step that blocked it.
    Blocked { reason: String },
    /// Nothing to do until the runtime publishes more state.
    Wait,
}

impl RunSequence {
    /// Begin a sequence from a plan, ready to request its first step.
    ///
    /// The plan already contains every step, so nothing about the sequence can change while it runs.
    pub fn new(config: &str, plan: &RunPlan) -> Self {
        Self {
            config: config.to_owned(),
            launches_program: plan.launches_program(),
            steps: plan
                .steps
                .iter()
                .map(|step| SequenceStep {
                    kind: step.kind,
                    name: step.name.clone(),
                    config: step.config.clone(),
                    state: StepState::Waiting,
                })
                .collect(),
            current: 0,
            blocked: None,
            stopping: false,
        }
    }

    /// A sequence over build actions only, for the Build control.
    pub fn build_only(config: &str, steps: &[super::PreparedStep]) -> Self {
        Self {
            config: config.to_owned(),
            launches_program: false,
            steps: steps
                .iter()
                .map(|step| SequenceStep {
                    kind: StepKind::Build,
                    name: step.name.clone(),
                    config: step.config.clone(),
                    state: StepState::Waiting,
                })
                .collect(),
            current: 0,
            blocked: None,
            stopping: false,
        }
    }

    /// Every step with its state, for status text and tests.
    pub fn steps(&self) -> &[SequenceStep] {
        &self.steps
    }

    /// The step now being prepared, when the sequence has one.
    pub fn current_step(&self) -> Option<&SequenceStep> {
        self.steps.get(self.current)
    }

    /// Whether the sequence is still working rather than finished or blocked.
    ///
    /// A stop in progress counts as working: the window must not close while a preparation program
    /// is still being terminated.
    pub fn is_active(&self) -> bool {
        self.blocked.is_none() && (self.stopping || self.current < self.steps.len())
    }

    /// Whether a stop has been requested but the sequence has not yet accepted one.
    pub fn is_stopping(&self) -> bool {
        self.stopping
    }

    /// The step that blocked this sequence, once it has stopped for good.
    pub fn blocked_by(&self) -> Option<&str> {
        self.blocked.as_deref()
    }

    /// The session this sequence currently owns, if its current step reached a provider.
    pub fn current_session(&self) -> Option<u64> {
        match self.current_step().map(|step| &step.state) {
            Some(StepState::Running { session, .. }) => Some(*session),
            _ => None,
        }
    }

    /// Record that a step's session was published by the runtime.
    pub fn started(&mut self, index: usize, session: u64, provider_session: Option<String>) {
        if let Some(step) = self.steps.get_mut(index) {
            step.state = StepState::Running {
                session,
                provider_session,
            };
        }
    }

    /// Record that a step could not be started at all.
    pub fn start_failed(&mut self, index: usize, reason: &str) {
        if let Some(step) = self.steps.get_mut(index) {
            step.state = StepState::Failed {
                reason: reason.to_owned(),
            };
        }
        self.block_here(index, reason);
    }

    /// Request a stop of everything this sequence has started.
    ///
    /// A step still running is stopped first, so it gets the chance to clean up before the sequence
    /// gives up on it. A step that was only queued is abandoned immediately: there is nothing to
    /// stop, and the sequence must not wait for a program the user no longer wants.
    pub fn request_stop(&mut self) {
        self.stopping = true;
        let running = matches!(
            self.current_step().map(|step| &step.state),
            Some(StepState::Running { .. })
        );
        if !running {
            if let Some(step) = self.steps.get_mut(self.current) {
                step.state = StepState::Stopped;
            }
            self.block_here(self.current, "已停止准备步骤");
        }
    }

    /// Record that the session of the step now being stopped has ended.
    ///
    /// The sequence was already blocked by the stop; this only closes the step that owned the
    /// session, so nothing can report a program as still preparing after it was terminated.
    pub fn stopped(&mut self, session: u64) {
        if self.session_for(self.current) != Some(session) {
            return;
        }
        if let Some(step) = self.steps.get_mut(self.current) {
            step.state = StepState::Stopped;
        }
        // The sequence is now finished rather than merely stopping: the window may close, and no
        // later step can start.
        self.stopping = false;
        self.block_here(self.current, "已停止准备步骤");
    }

    /// Feed one observed outcome for the step the sequence is waiting on.
    ///
    /// Returns whether the outcome advanced or blocked the sequence, so a caller can tell an
    /// observation that mattered from one that arrived late.
    pub fn observe(&mut self, index: usize, outcome: StepOutcome) -> bool {
        if index != self.current || self.blocked.is_some() {
            // A late answer for a step the sequence has already left cannot resurrect it.
            return false;
        }
        if self.stopping {
            // A stop already decided the launch. A program that exits successfully while the stop is
            // on its way must not advance a sequence the user asked to abandon.
            return false;
        }
        let Some(step) = self.steps.get(index) else {
            return false;
        };
        if !matches!(step.state, StepState::Running { .. }) {
            return false;
        }
        let name = self.steps[index].name.clone();
        match outcome {
            StepOutcome::Exited { code: 0 } => {
                self.steps[index].state = StepState::Succeeded;
                self.current += 1;
                true
            }
            StepOutcome::Exited { code } => {
                let reason = format!("步骤 {name} 退出码 {code}");
                self.steps[index].state = StepState::Failed {
                    reason: reason.clone(),
                };
                self.block_here(index, &reason);
                true
            }
            StepOutcome::Terminated => {
                self.steps[index].state = StepState::Stopped;
                // A termination the user did not ask for still blocks: the remaining steps would
                // run against a preparation that never finished.
                self.block_here(index, &format!("步骤 {name} 已终止"));
                true
            }
            StepOutcome::Unknown => {
                // An unanswerable status is not evidence of success. The provider may have retired,
                // so the sequence stops rather than continuing on a guess.
                let reason = format!("步骤 {name} 的结束状态无法确认");
                self.steps[index].state = StepState::Failed {
                    reason: reason.clone(),
                };
                self.block_here(index, &reason);
                true
            }
        }
    }

    /// Forget the session of the finished step, so a later step cannot adopt it.
    pub fn session_for(&self, index: usize) -> Option<u64> {
        match self.steps.get(index).map(|step| &step.state) {
            Some(StepState::Running { session, .. }) => Some(*session),
            _ => None,
        }
    }

    /// What the host should do next, given the sessions the runtime has published.
    ///
    /// This is the single decision point, so the order of preparation is checked in one place rather
    /// than spread across the UI and the worker.
    pub fn next_action(&self, sessions: &BTreeMap<u64, RunSession>) -> SequenceAction {
        if let Some(reason) = &self.blocked {
            return SequenceAction::Blocked {
                reason: reason.clone(),
            };
        }
        let Some(step) = self.steps.get(self.current) else {
            return SequenceAction::Done;
        };
        match &step.state {
            StepState::Waiting => SequenceAction::Start {
                index: self.current,
            },
            StepState::Starting => SequenceAction::Wait,
            StepState::Running { session, .. } => {
                if self.stopping {
                    return SequenceAction::Stop { session: *session };
                }
                // A session the runtime no longer reports cannot complete this step.
                if sessions
                    .get(session)
                    .is_some_and(|known| !known.is_active())
                {
                    return SequenceAction::Blocked {
                        reason: format!("步骤 {} 的会话已结束但未报告结果", step.name),
                    };
                }
                SequenceAction::Wait
            }
            StepState::Succeeded => SequenceAction::Wait,
            StepState::Failed { reason } => SequenceAction::Blocked {
                reason: reason.clone(),
            },
            StepState::Stopped => SequenceAction::Blocked {
                reason: "已停止准备步骤".to_owned(),
            },
        }
    }

    /// Mark the sequence blocked at one index, keeping the first cause rather than the last.
    fn block_here(&mut self, index: usize, reason: &str) {
        if self.blocked.is_none() {
            self.blocked = Some(match self.steps.get(index) {
                Some(step) => format!("{}步骤 {}：{}", step.kind.label(), step.name, reason),
                None => reason.to_owned(),
            });
        }
    }
}
