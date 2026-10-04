//! What the debug controls may offer, given what the host has confirmed and reported.
//!
//! Two different facts decide whether a debug control is enabled, and keeping them apart is the
//! point of this module: whether a debug session *could* start (a provider is installed and a target
//! is usable), and what the session *is* right now (starting, running, paused at a location, gone).
//!
//! Nothing here starts anything. It answers what a control would mean at this moment, so the panel
//! and the launch path cannot disagree — and so a button is never enabled for an action that has
//! nowhere to go.

/// What one debug session currently is, as its provider last reported it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum DebugSessionState {
    /// Nothing is being debugged; nothing is connected.
    #[default]
    Disconnected,
    /// A session was asked for and has not answered yet.
    Starting,
    /// Connected and running, with no location to show.
    Running,
    /// Stopped at a location, ready to be resumed.
    Paused {
        source: String,
        line: u32,
        /// Why the target stopped, when the provider said.
        reason: Option<String>,
    },
    /// The provider reported the target is gone.
    Exited,
    /// The provider refused, failed, or retired before answering.
    Failed { reason: String },
}

impl DebugSessionState {
    /// Whether this state means a provider is serving the session.
    ///
    /// `Starting` counts as connected because something has been asked for and an answer is coming;
    /// `Failed` and `Exited` do not, because nothing is being served any more.
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Starting | Self::Running | Self::Paused { .. })
    }

    /// Where the session is stopped, when it is stopped somewhere.
    pub fn paused_at(&self) -> Option<(&str, u32)> {
        match self {
            Self::Paused { source, line, .. } => Some((source.as_str(), *line)),
            _ => None,
        }
    }
}

/// How one step is asked for, in the provider's own vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugStep {
    /// Enter the call at the current location.
    Into,
    /// Run to the next line of this frame.
    Over,
    /// Run until this frame returns.
    Out,
}

impl DebugStep {
    /// The word the provider is sent for this step.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Into => "into",
            Self::Over => "over",
            Self::Out => "out",
        }
    }
}

/// What the selected provider offers, as its own declaration says.
///
/// Defaulting to nothing is deliberate: a capability the host has not been told about is not a
/// capability it may offer, so an unknown provider leaves every ability disabled with a reason.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DebugCapabilities {
    pub breakpoints: bool,
    pub resume_pause: bool,
    pub step: bool,
    /// Whether the provider can describe a paused target: its frames and their variables.
    ///
    /// One ability rather than two, because the panel needs both to show anything: a frame list with
    /// no variables is not an inspection view. The host derives it from the provider's own declaration
    /// the same way, so an editor that dropped it would render a view nothing could ever fill.
    pub inspect: bool,
}

/// The debug actions a panel may offer right now, each with the reason it is unavailable.
///
/// A control that is unavailable always has a reason: "disabled" without one is the state this
/// module exists to prevent, because a user cannot tell a bug from an unmet precondition. Two kinds
/// of reason are kept distinct — the provider does not offer the ability, or the session is in a
/// state where it means nothing — because they call for different actions from the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugControls {
    /// Begin debugging the selected configuration.
    pub start: Result<(), String>,
    /// Resume a paused target.
    pub resume: Result<(), String>,
    /// Ask a running target to stop where it is.
    pub pause: Result<(), String>,
    /// End the session and the target it owns.
    pub stop: Result<(), String>,
    /// Step the paused target, in the direction the caller asks for.
    pub step: Vec<(DebugStep, Result<(), String>)>,
}

impl DebugControls {
    /// Derive the controls from the two facts that decide them.
    ///
    /// `availability` is the host's answer about whether a debug session could start at all;
    /// `state` is what the current session is, and `capabilities` is what the provider said it can
    /// do. Availability decides starting, the session state decides the rest, and a capability the
    /// provider never declared disables the control that needs it rather than being assumed.
    pub fn derive(
        availability: Result<&str, &str>,
        state: &DebugSessionState,
        capabilities: DebugCapabilities,
    ) -> Self {
        let start = match (availability, state) {
            // A session already being served is not started again: the panel stops it instead.
            (_, state) if state.is_connected() => {
                Err("已有调试会话；请先停止它再开始新的调试".into())
            }
            (Ok(_), _) => Ok(()),
            (Err(reason), _) => Err(reason.to_owned()),
        };
        // A control is unavailable for one of two reasons, and they are reported in that order: the
        // provider never offered the ability, or the session is in a state where it means nothing.
        // Offering a control the provider cannot serve would only move the failure to the click.
        let state_reason = |state: &DebugSessionState, meaning: &str| match state {
            DebugSessionState::Starting => "调试会话正在连接".to_owned(),
            DebugSessionState::Disconnected => "没有调试会话".to_owned(),
            DebugSessionState::Exited => "目标已退出".to_owned(),
            DebugSessionState::Failed { reason } => format!("调试会话失败：{reason}"),
            _ => meaning.to_owned(),
        };
        let resume = if !capabilities.resume_pause {
            Err("该调试提供者未声明继续与暂停能力".into())
        } else {
            match state {
                DebugSessionState::Paused { .. } => Ok(()),
                other => Err(state_reason(other, "目标正在运行")),
            }
        };
        let pause = if !capabilities.resume_pause {
            Err("该调试提供者未声明继续与暂停能力".into())
        } else {
            match state {
                DebugSessionState::Running => Ok(()),
                other => Err(state_reason(other, "目标已暂停")),
            }
        };
        // Stopping is offered whenever a provider is serving the session, including while it is
        // connecting: that is exactly when a user needs a way out of a session that will not answer.
        let stop = if state.is_connected() {
            Ok(())
        } else {
            match state {
                DebugSessionState::Failed { reason } => Err(format!("调试会话失败：{reason}")),
                DebugSessionState::Exited => Err("目标已退出".into()),
                _ => Err("没有调试会话".into()),
            }
        };
        // Stepping means moving a target that is already stopped at a location. It is offered only
        // while paused, and only when the provider said it can step at all.
        let step = [DebugStep::Into, DebugStep::Over, DebugStep::Out]
            .into_iter()
            .map(|kind| {
                let outcome = if !capabilities.step {
                    Err("该调试提供者未声明单步能力".into())
                } else {
                    match state {
                        DebugSessionState::Paused { .. } => Ok(()),
                        other => Err(state_reason(other, "目标正在运行；请先暂停再单步")),
                    }
                };
                (kind, outcome)
            })
            .collect();
        Self {
            start,
            resume,
            pause,
            stop,
            step,
        }
    }

    /// Whether stepping in one direction is offered.
    pub fn can_step(&self, kind: DebugStep) -> bool {
        self.step
            .iter()
            .any(|(candidate, outcome)| *candidate == kind && outcome.is_ok())
    }

    /// Whether starting is offered.
    pub fn can_start(&self) -> bool {
        self.start.is_ok()
    }

    /// Whether stopping is offered.
    pub fn can_stop(&self) -> bool {
        self.stop.is_ok()
    }
}

#[cfg(test)]
#[path = "debug_state_tests.rs"]
mod tests;
