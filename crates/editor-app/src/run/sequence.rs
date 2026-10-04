//! Ordered preparation before a program starts: build actions, then pre-launch steps, then the run.
//!
//! The sequence owns no process. Every step is an ordinary execution session, so the provider keeps
//! its authority over the programs it started and the host never borrows a provider-private handle.
//! A step's completion condition is the provider's observation of an exit, never elapsed time and
//! never text the host happens to have seen.

use super::{RunPlan, StepKind};

#[cfg(test)]
#[path = "sequence_tests.rs"]
mod tests;

/// What one step of a sequence is doing right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepState {
    /// Queued but not yet requested from the runtime.
    Waiting,
    /// Requested, but the runtime has not published this step's session identity yet.
    ///
    /// This is what stops a second click from requesting the same preparation twice: the step is
    /// owned from the moment it is asked for, not from the moment its session appears.
    Starting {
        /// The launch identity of the request that is preparing this step.
        request: u64,
    },
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

/// One step's identity plus its state, so output and status can name the step it belongs to.
#[derive(Clone, Debug)]
pub struct SequenceStep {
    pub kind: StepKind,
    pub name: String,
    /// The configuration whose stored definition produced this step.
    ///
    /// A step can reference a build owned by another configuration, so this is what says whose
    /// definition a step came from. Nothing reads it today — the step is always reached through the
    /// sequence that holds it — and it is kept because a step without its origin cannot be explained
    /// when a launch is assembled from more than one configuration.
    #[allow(dead_code)]
    pub config: String,
    pub state: StepState,
}

/// A launch being prepared, or a build being run on its own.
#[derive(Clone, Debug)]
pub struct RunSequence {
    /// The configuration the user acted on; the program step belongs to it.
    ///
    /// Like a step's own `config`, this is identity rather than state: a sequence is reached through
    /// the map keyed by the configuration, so the field is not consulted. It is kept for the same
    /// reason — diagnostics about a launch should be able to say which configuration it was.
    #[allow(dead_code)]
    pub config: String,
    steps: Vec<SequenceStep>,
    /// The requests behind each step, fixed when the sequence began so a later edit cannot change
    /// what a launch already started doing.
    requests: Vec<plugin_runtime::RunRequest>,
    /// The launch identity of each step's start request, once it has been requested.
    requested: Vec<Option<u64>>,
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
            requests: plan.steps.iter().map(|step| step.request.clone()).collect(),
            requested: vec![None; plan.steps.len()],
            current: 0,
            blocked: None,
            stopping: false,
        }
    }

    /// A sequence over build actions only, for the Build control.
    pub fn build_only(config: &str, steps: &[super::PreparedStep]) -> Self {
        Self {
            config: config.to_owned(),
            steps: steps
                .iter()
                .map(|step| SequenceStep {
                    kind: StepKind::Build,
                    name: step.name.clone(),
                    config: step.config.clone(),
                    state: StepState::Waiting,
                })
                .collect(),
            requests: steps.iter().map(|step| step.request.clone()).collect(),
            requested: vec![None; steps.len()],
            current: 0,
            blocked: None,
            stopping: false,
        }
    }

    /// The request for one step, so a caller stages exactly what the plan computed.
    pub fn planned_request(&self, index: usize) -> Option<&plugin_runtime::RunRequest> {
        self.requests.get(index)
    }

    /// The position of the step now being prepared.
    pub fn current_index(&self) -> usize {
        self.current
    }

    /// Replace one entry of a step's request for this launch only.
    ///
    /// A launch-time entry never discards the configuration's own entries: it replaces the entry of
    /// the same name, so a tool directory or a stored variable cannot be lost by an override.
    pub fn override_environment(&mut self, index: usize, entry: &plugin_runtime::RunEnvEntry) {
        if let Some(request) = self.requests.get_mut(index) {
            request
                .env
                .retain(|existing| !existing.name.eq_ignore_ascii_case(&entry.name));
            request.env.push(entry.clone());
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
    /// A stop in progress counts as working, so the window cannot close while a preparation program
    /// is still being terminated. The program step counts as working while it runs: the launch is
    /// owned by this sequence, so a repeated click locates that session instead of starting another.
    pub fn is_active(&self) -> bool {
        if self.blocked.is_some() {
            return false;
        }
        if self.stopping || self.current < self.steps.len() {
            return true;
        }
        self.steps
            .last()
            .is_some_and(|step| matches!(step.state, StepState::Running { .. }))
    }

    /// Whether a stop has been requested but the sequence has not yet accepted one.
    ///
    /// Only the checks ask this, through `RunControls::is_stopping`, which is itself only asked by a
    /// check: the application reads the sequence's state through `is_active`, which already counts a
    /// stop in progress as working. Compiled for tests so the library does not carry an accessor
    /// nothing consults.
    #[cfg(test)]
    pub fn is_stopping(&self) -> bool {
        self.stopping
    }

    /// The step that blocked this sequence, once it has stopped for good.
    ///
    /// Like `is_stopping`, this is reached only from the checks' own entry points; the application
    /// reads a blocked preparation through `preparation_blocked`, which is itself a check's question.
    #[cfg(test)]
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

    /// Record that a step has been asked for, before its session exists.
    ///
    /// A step that has been requested is no longer waiting, so no second request can be made for it.
    pub fn requested(&mut self, index: usize, request: u64) {
        if let Some(step) = self.steps.get_mut(index) {
            step.state = StepState::Starting { request };
        }
        if let Some(slot) = self.requested.get_mut(index) {
            *slot = Some(request);
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

    /// What the host should do next, given how the runtime currently sees a session.
    ///
    /// This is the single decision point, so the order of preparation is checked in one place rather
    /// than spread across the UI and the worker. `known` and `active` are asked separately because a
    /// session the runtime has not published yet is not the same as one it has already dropped.
    pub fn next_action(
        &self,
        known: impl Fn(u64) -> bool,
        active: impl Fn(u64) -> bool,
    ) -> SequenceAction {
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
            StepState::Starting { .. } => SequenceAction::Wait,
            StepState::Running { session, .. } => {
                if self.stopping {
                    return SequenceAction::Stop { session: *session };
                }
                // A session the runtime reports as finished cannot complete this step: the sequence
                // waits for a provider's observation, not for a session to disappear.
                if known(*session) && !active(*session) {
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
