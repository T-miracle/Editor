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

/// The debug actions a panel may offer right now, each with the reason it is unavailable.
///
/// A control that is unavailable always has a reason: "disabled" without one is the state this
/// module exists to prevent, because a user cannot tell a bug from an unmet precondition.
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
}

impl DebugControls {
    /// Derive the controls from the two facts that decide them.
    ///
    /// `availability` is the host's answer about whether a debug session could start at all;
    /// `state` is what the current session is. Availability decides starting, the session state
    /// decides the rest, and neither is inferred from the other.
    pub fn derive(availability: Result<&str, &str>, state: &DebugSessionState) -> Self {
        let start = match (availability, state) {
            // A session already being served is not started again: the panel stops it instead.
            (_, state) if state.is_connected() => {
                Err("已有调试会话；请先停止它再开始新的调试".into())
            }
            (Ok(_), _) => Ok(()),
            (Err(reason), _) => Err(reason.to_owned()),
        };
        // Resuming means continuing a target that is stopped; it is not a way to begin one.
        let resume = match state {
            DebugSessionState::Paused { .. } => Ok(()),
            DebugSessionState::Starting => Err("调试会话正在连接".into()),
            DebugSessionState::Running => Err("目标正在运行".into()),
            DebugSessionState::Disconnected => Err("没有调试会话".into()),
            DebugSessionState::Exited => Err("目标已退出".into()),
            DebugSessionState::Failed { reason } => Err(format!("调试会话失败：{reason}")),
        };
        // Pausing means asking a target that is running to stop where it is.
        let pause = match state {
            DebugSessionState::Running => Ok(()),
            DebugSessionState::Starting => Err("调试会话正在连接".into()),
            DebugSessionState::Paused { .. } => Err("目标已暂停".into()),
            DebugSessionState::Disconnected => Err("没有调试会话".into()),
            DebugSessionState::Exited => Err("目标已退出".into()),
            DebugSessionState::Failed { reason } => Err(format!("调试会话失败：{reason}")),
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
        Self {
            start,
            resume,
            pause,
            stop,
        }
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
