//! Relative opens admit one observed document transition before their actual identity receipt is matched.

use super::Reason;
use plugin_protocol::api::DocumentVersion;

/// No target identity is inferred from a link path; only a Preview candidate and the Opened receipt can agree.
#[derive(Default)]
pub(super) struct Opening {
    candidate: Option<DocumentVersion>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Arrival {
    Ready,
    Waiting,
}

impl Opening {
    /// Source edits, closure and a second transition interrupt the intent, even if a later view revisits its target.
    pub(super) fn observe(
        &mut self,
        origin: &DocumentVersion,
        next: Option<&DocumentVersion>,
    ) -> bool {
        let Some(next) = next else { return false };
        if self.candidate.is_some() || next.id == origin.id || next.path == origin.path {
            return false;
        }
        self.candidate = Some(next.clone());
        true
    }

    /// Either arrival order is valid, but a different ID, path or revision never authorizes a follow-up reveal.
    pub(super) fn complete(
        &self,
        origin: &DocumentVersion,
        opened: &DocumentVersion,
        current: Option<&DocumentVersion>,
    ) -> Result<Arrival, Reason> {
        let current = current.ok_or(Reason::SourceChanged)?;
        if self
            .candidate
            .as_ref()
            .is_some_and(|candidate| candidate != opened)
        {
            return Err(Reason::SourceChanged);
        }
        if current == opened {
            Ok(Arrival::Ready)
        } else if current == origin && self.candidate.is_none() {
            Ok(Arrival::Waiting)
        } else {
            Err(Reason::SourceChanged)
        }
    }
}
