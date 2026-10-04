//! Pause data that belongs to one pause, and the rules that stop a late answer from outliving it.
//!
//! A debug view shows stack frames and variables that describe one moment: the pause a session is
//! sitting at. Every step, resume or restart ends that moment, and any answer still on its way about
//! it is then about something the user is no longer looking at. Applying such an answer would show a
//! frame the target has already left, next to a location it is no longer at — so the data is scoped
//! to the pause it describes and refused once that pause is over.
//!
//! Nothing here reads a program's output: a stack frame and a variable are the provider's reports
//! about a stopped target, which is a different thing from the text a program printed.
use std::collections::BTreeMap;

/// Which stopped moment a piece of inspection data describes.
///
/// Generated rather than derived from a location, so two pauses at the same line are still two
/// different moments and an answer about the first cannot be mistaken for one about the second.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PauseScope(u64);

impl PauseScope {
    /// A scope for a request that is about no pause yet: starting a session.
    ///
    /// It is deliberately not a pause identity, so an answer carrying it can never be accepted as
    /// describing a pause, and a pause can never be mistaken for a session that has not begun.
    pub fn starting() -> Self {
        Self(0)
    }
}

/// Why inspection data was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InspectionError {
    /// No session is being inspected.
    NoSession,
    /// The session is not stopped, so there is nothing to inspect.
    NotPaused,
    /// The answer describes a pause that is over: a step, resume or restart has happened since.
    StalePause,
    /// The answer names a frame this pause does not have.
    NoSuchFrame { frame: u32 },
    /// The answer describes a pause belonging to a different session.
    WrongSession,
    /// The provider itself reported a failure, with its own account of what went wrong.
    ///
    /// Kept apart from the host's own reasons: a provider that could not report frames has said
    /// nothing about the target, and showing that as an empty stack would describe one it invented.
    Provider(String),
}

impl std::fmt::Display for InspectionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSession => write!(formatter, "没有正在检查的调试会话"),
            Self::NotPaused => write!(formatter, "目标未暂停，暂无暂停数据"),
            Self::StalePause => write!(formatter, "该暂停已结束，迟到的结果不会覆盖当前视图"),
            Self::NoSuchFrame { frame } => write!(formatter, "该暂停没有第 {frame} 个栈帧"),
            Self::WrongSession => write!(formatter, "该结果属于另一个调试会话"),
            Self::Provider(message) => write!(formatter, "调试提供者报告失败：{message}"),
        }
    }
}

impl std::error::Error for InspectionError {}

/// One frame of a stopped target, as its provider reported it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackFrame {
    /// Provider-assigned frame identity, kept opaque.
    pub id: u32,
    /// What the frame is called, which is the provider's word and not the host's.
    pub name: String,
    pub source: String,
    pub line: u32,
}

/// One variable in one frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugVariable {
    pub name: String,
    /// The provider's rendering of the value, shown as given.
    pub value: String,
}

/// The inspection data of one pause: its frames, and the variables of whichever frame is selected.
///
/// Both arrive from the provider and neither is synthesised: an absent frame list is an empty list,
/// not a fabricated one, and selecting a frame the pause does not have is refused by name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PauseData {
    scope: Option<PauseScope>,
    /// Counts pauses for this view, never reset.
    ///
    /// It is not derived from the current scope, because clearing ends a pause without starting one:
    /// a later pause must not reuse an ended pause's identity, or an answer about the ended one would
    /// be accepted as an answer about the new one.
    pauses: u64,
    /// Whether the provider has described this pause yet.
    ///
    /// Kept apart from "the frame list is empty", because a provider that reports no frames has
    /// answered and a provider that has not answered yet has not: only the first may be replaced by a
    /// later answer, and a second frame report for one pause is how a late one is refused.
    described: bool,
    frames: Vec<StackFrame>,
    /// Variables by frame identity, so switching frames does not discard what was already read.
    variables: BTreeMap<u32, Vec<DebugVariable>>,
    selected: Option<u32>,
}

impl PauseData {
    /// Begin a new pause, ending whatever the previous one held.
    ///
    /// This is the only way a scope comes into being, so there is no constructor that could hand out
    /// one without also invalidating the data that described the moment before it.
    pub fn begin(&mut self) -> PauseScope {
        // Strictly increasing across clears, so an identity is never reused by a later pause.
        self.pauses += 1;
        let scope = PauseScope(self.pauses);
        self.scope = Some(scope);
        self.described = false;
        self.frames.clear();
        self.variables.clear();
        self.selected = None;
        scope
    }

    /// End the current pause, so every answer about it is refused from now on.
    pub fn clear(&mut self) {
        self.scope = None;
        self.described = false;
        self.frames.clear();
        self.variables.clear();
        self.selected = None;
    }

    /// The pause being inspected, if any.
    pub fn scope(&self) -> Option<PauseScope> {
        self.scope
    }

    pub fn frames(&self) -> &[StackFrame] {
        &self.frames
    }

    /// The frame whose variables are shown, if one is selected.
    pub fn selected_frame(&self) -> Option<u32> {
        self.selected
    }

    /// The variables of one frame, or an empty slice when none were read.
    pub fn variables_of(&self, frame: u32) -> &[DebugVariable] {
        self.variables
            .get(&frame)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// Whether this scope is the pause currently being inspected.
    ///
    /// A caller holding a scope from before a step asks this before applying an answer, which is what
    /// keeps a late result from replacing a newer view. Nothing else is decided here: whether a
    /// particular answer makes sense also depends on the frames and the selected frame.
    pub fn accepts(&self, scope: PauseScope) -> Result<(), InspectionError> {
        if self.scope != Some(scope) {
            return Err(InspectionError::StalePause);
        }
        Ok(())
    }

    /// Record the frames of one pause, refusing an answer about any other.
    pub fn set_frames(
        &mut self,
        scope: PauseScope,
        frames: Vec<StackFrame>,
    ) -> Result<(), InspectionError> {
        self.accepts(scope)?;
        if self.described {
            // One pause is described once: a second frame report for it is a late answer about a
            // moment that has already been shown, and applying it would move the view backwards.
            return Err(InspectionError::StalePause);
        }
        self.described = true;
        // The first frame is where the target stopped, which is the one a user expects to see first.
        self.selected = frames.first().map(|frame| frame.id);
        self.frames = frames;
        self.variables.clear();
        Ok(())
    }

    /// Select a frame of the current pause, which is what locates the source.
    pub fn select_frame(&mut self, frame: u32) -> Result<&StackFrame, InspectionError> {
        if self.frames.is_empty() {
            return Err(InspectionError::NotPaused);
        }
        if !self.frames.iter().any(|candidate| candidate.id == frame) {
            return Err(InspectionError::NoSuchFrame { frame });
        }
        self.selected = Some(frame);
        Ok(self
            .frames
            .iter()
            .find(|candidate| candidate.id == frame)
            .expect("the frame was just found"))
    }

    /// Record one frame's variables, refusing an answer about a pause that is over.
    pub fn set_variables(
        &mut self,
        scope: PauseScope,
        frame: u32,
        variables: Vec<DebugVariable>,
    ) -> Result<(), InspectionError> {
        self.accepts(scope)?;
        if !self.frames.iter().any(|candidate| candidate.id == frame) {
            return Err(InspectionError::NoSuchFrame { frame });
        }
        self.variables.insert(frame, variables);
        Ok(())
    }

    /// Where the selected frame is, which is the location to reveal in the source.
    pub fn selected_location(&self) -> Option<(&str, u32)> {
        let selected = self.selected?;
        self.frames
            .iter()
            .find(|frame| frame.id == selected)
            .map(|frame| (frame.source.as_str(), frame.line))
    }
}

#[cfg(test)]
#[path = "inspection_tests.rs"]
mod tests;
