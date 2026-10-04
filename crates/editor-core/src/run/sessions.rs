//! Several debug sessions at once, each with its own state and its own pause.
//!
//! Two configurations can be debugged together, so every fact here is per session: a pause in one
//! never describes another, resuming one never moves another, and the panel acts on whichever session
//! is selected. A single shared "current state" would make a stop in one session look like a stop in
//! the other, which is exactly the confusion this module exists to prevent.
use super::{DebugSessionState, PauseData, PauseScope};
use std::collections::BTreeMap;

/// One configuration's debug session.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DebugSession {
    state: DebugSessionState,
    /// The provider's own session identity, which every later call names.
    ///
    /// Kept exactly as the provider reported it: the host never derives one, and a session without
    /// one cannot be asked anything, because there would be nothing to address the question to.
    provider_session: Option<String>,
    pause: PauseData,
}

impl DebugSession {
    pub fn state(&self) -> &DebugSessionState {
        &self.state
    }

    /// The provider's own identity for this session, once it has answered.
    pub fn provider_session(&self) -> Option<&str> {
        self.provider_session.as_deref()
    }

    /// Record the provider's identity for this session.
    ///
    /// A different identity means a different session, so whatever described the previous one is
    /// dropped: frames and variables belong to the session that reported them.
    pub fn note_provider_session(&mut self, session: &str) {
        if self.provider_session.as_deref() != Some(session) {
            self.pause.clear();
        }
        self.provider_session = Some(session.to_owned());
    }

    pub fn pause(&self) -> &PauseData {
        &self.pause
    }

    /// Replace the reported state, ending the pause whenever the target is no longer stopped.
    ///
    /// This is what makes a resume, a step or an exit invalidate the frames and variables that
    /// described the previous moment: the state change is the one place that decides it, so no
    /// caller can forget to.
    pub fn note_state(&mut self, state: DebugSessionState) {
        if !matches!(state, DebugSessionState::Paused { .. }) {
            self.pause.clear();
        }
        self.state = state;
    }

    /// Begin a pause at a new location, ending whatever the previous one held.
    pub fn begin_pause(&mut self) -> PauseScope {
        self.pause.begin()
    }

    /// Record the frames of one pause, refusing an answer about any other.
    pub fn set_frames(
        &mut self,
        scope: PauseScope,
        frames: Vec<super::StackFrame>,
    ) -> Result<(), super::InspectionError> {
        self.pause.set_frames(scope, frames)
    }

    /// Select a frame of the current pause, which is what locates the source.
    pub fn select_frame(&mut self, frame: u32) -> Result<(), super::InspectionError> {
        self.pause.select_frame(frame).map(|_| ())
    }

    /// Record one frame's variables, refusing an answer about a pause that is over.
    pub fn set_variables(
        &mut self,
        scope: PauseScope,
        frame: u32,
        variables: Vec<super::DebugVariable>,
    ) -> Result<(), super::InspectionError> {
        self.pause.set_variables(scope, frame, variables)
    }
}

/// Every debug session this editor is running, keyed by the configuration that started it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DebugSessions {
    sessions: BTreeMap<String, DebugSession>,
    selected: Option<String>,
}

impl DebugSessions {
    /// The configuration whose session the panel acts on, if any.
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// Select the session the panel acts on, refusing a configuration with no session.
    ///
    /// Selecting is the only way the panel's target changes, so an action can never be delivered to
    /// a session the user is not looking at.
    pub fn select(&mut self, config: &str) -> bool {
        if !self.sessions.contains_key(config) {
            return false;
        }
        self.selected = Some(config.to_owned());
        true
    }

    /// Add or replace one configuration's session, selecting it if nothing else is selected.
    pub fn insert(&mut self, config: &str, session: DebugSession) {
        if self.sessions.is_empty() {
            self.selected = Some(config.to_owned());
        }
        self.sessions.insert(config.to_owned(), session);
    }

    /// Remove one configuration's session, moving the selection to another if there is one.
    pub fn remove(&mut self, config: &str) -> Option<DebugSession> {
        let removed = self.sessions.remove(config);
        if self.selected.as_deref() == Some(config) {
            // Another live session takes over rather than leaving the panel acting on nothing.
            self.selected = self.sessions.keys().next().cloned();
        }
        removed
    }

    /// The session one configuration is running, if any.
    pub fn session(&self, config: &str) -> Option<&DebugSession> {
        self.sessions.get(config)
    }

    /// The session one configuration is running, for changing it.
    pub fn session_mut(&mut self, config: &str) -> Option<&mut DebugSession> {
        self.sessions.get_mut(config)
    }

    /// The selected session, if one is selected and still present.
    pub fn current(&self) -> Option<(&str, &DebugSession)> {
        let selected = self.selected.as_deref()?;
        self.sessions
            .get_key_value(selected)
            .map(|(config, session)| (config.as_str(), session))
    }

    /// Every session, in configuration order, so a panel can list them without guessing.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &DebugSession)> {
        self.sessions
            .iter()
            .map(|(config, session)| (config.as_str(), session))
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// Whether any session other than this one is stopped, which is what a panel asks before moving
    /// the view to a new breakpoint: another paused session means the user is reading something else.
    pub fn another_is_paused(&self, config: &str) -> bool {
        self.sessions.iter().any(|(candidate, session)| {
            candidate != config && matches!(session.state, DebugSessionState::Paused { .. })
        })
    }
}

#[cfg(test)]
#[path = "sessions_tests.rs"]
mod tests;
