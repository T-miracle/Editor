//! Resolves managed two-step bindings before GPUI dispatch without replaying consumed prefixes.

use super::{catalog::Target, config::Sequence, conflicts::Conflict, engine::BindingEngine};
use gpui_kit::{AnyWindowHandle, FocusHandle, KeyContext, Keystroke};
use std::time::{Duration, Instant};

/// One next-step candidate supplies translated operation text and canonical keycaps to the UI.
#[derive(Clone)]
pub(super) struct NextStep {
    pub(super) operation: String,
    pub(super) title: String,
    pub(super) sequence: Sequence,
}

/// The prompt uses the same fixed first-step deadline as dispatch, not a repaint-relative timer.
#[derive(Clone)]
pub(super) struct PendingHint {
    pub(super) first: String,
    pub(super) next: Vec<NextStep>,
    pub(super) deadline: Instant,
}

/// Pending, Dispatch and Conflict consume the key. Pass preserves the same original event.
pub(super) enum Resolution {
    Pass,
    Pending(PendingHint),
    Dispatch {
        operation: String,
        target: Target,
    },
    /// Runtime ambiguity is reported instead of selecting a command by registration order.
    Conflict(Vec<Conflict>),
}

struct Pending {
    window: AnyWindowHandle,
    focus: Option<FocusHandle>,
    contexts: Vec<KeyContext>,
    revision: u64,
    hint: PendingHint,
}

/// Keep one resolver per application window; there is no process-global pending sequence.
#[derive(Default)]
pub(super) struct Resolver {
    pending: Option<Pending>,
}

impl Resolver {
    /// The caller schedules a timer for this deadline and redraws when expire returns true.
    pub(super) fn pending(&self) -> Option<&PendingHint> {
        self.pending.as_ref().map(|pending| &pending.hint)
    }

    /// Capture, focus loss, modal entry and plugin retirement all cancel the same pending state.
    pub(super) fn cancel(&mut self) -> bool {
        self.pending.take().is_some()
    }

    /// Ignore an obsolete timer by checking its deadline against the current first step.
    pub(super) fn expire(&mut self, now: Instant) -> bool {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| now >= pending.hint.deadline)
        {
            return self.cancel();
        }
        false
    }

    /// Clear a hint whose captured target path is no longer current, without notifying or dispatching.
    /// Root preparation can call this before rendering; the returned flag only reports a change.
    pub(super) fn invalidate_if_changed(
        &mut self,
        engine_revision: u64,
        window: AnyWindowHandle,
        focus: Option<FocusHandle>,
        contexts: &[KeyContext],
        now: Instant,
    ) -> bool {
        if self.pending.as_ref().is_some_and(|pending| {
            now >= pending.hint.deadline
                || pending.window != window
                || pending.focus != focus
                || pending.contexts != contexts
                || pending.revision != engine_revision
        }) {
            return self.cancel();
        }
        false
    }

    /// Resolve against live focus, contexts, engine revision and target availability.
    /// `available` must verify native handlers and current plugin trust/readiness/incarnation.
    /// Dispatch uses the returned exact target; callers never synthesize an action by its name.
    pub(super) fn resolve(
        &mut self,
        engine: &BindingEngine,
        stroke: &Keystroke,
        window: AnyWindowHandle,
        focus: Option<FocusHandle>,
        contexts: &[KeyContext],
        now: Instant,
        mut available: impl FnMut(&Target) -> bool,
    ) -> Resolution {
        if modifier_only(stroke) {
            return Resolution::Pass;
        }
        if self.invalidate_if_changed(engine.revision(), window, focus.clone(), contexts, now) {
            return Resolution::Pass;
        }
        let key = stroke.unparse();
        if let Some(pending) = self.pending.take() {
            // A failed continuation is not reinterpreted as another first step. The very
            // same event continues through native keymap/text handling, as the spec requires.
            let mut matches = Vec::new();
            for candidate in pending.hint.next {
                if candidate.sequence.get(1) != Some(&key) {
                    continue;
                }
                let Some(operation) = engine.operation(&candidate.operation) else {
                    continue;
                };
                if !engine
                    .effective(&operation.id)
                    .contains(&candidate.sequence)
                    || !in_context(&operation.target, contexts)
                    || !available(&operation.target)
                {
                    continue;
                }
                matches.push((candidate, operation.target.clone()));
            }
            return dispatch_unique(matches);
        }

        let mut next = Vec::new();
        let mut singles = Vec::new();
        for operation in engine.active_operations() {
            if !in_context(&operation.target, contexts) || !available(&operation.target) {
                continue;
            }
            for sequence in engine.effective(&operation.id) {
                if sequence.first() != Some(&key) {
                    continue;
                }
                let candidate = NextStep {
                    operation: operation.id.clone(),
                    title: operation.title.clone(),
                    sequence,
                };
                if candidate.sequence.len() == 2 {
                    next.push(candidate);
                } else if candidate.sequence.len() == 1 {
                    singles.push((candidate, operation.target.clone()));
                }
            }
        }
        if !next.is_empty() {
            if !singles.is_empty() {
                let conflicts = singles
                    .iter()
                    .map(|(candidate, _)| conflict(candidate))
                    .chain(next.iter().map(conflict))
                    .collect();
                return Resolution::Conflict(conflicts);
            }
            let hint = PendingHint {
                first: key,
                next,
                deadline: now + Duration::from_secs(2),
            };
            self.pending = Some(Pending {
                window,
                focus,
                contexts: contexts.to_vec(),
                revision: engine.revision(),
                hint: hint.clone(),
            });
            return Resolution::Pending(hint);
        }
        // Native single-step precedence and propagation remain with GPUI. Manifest commands
        // use this route after removal of the old raw-key shortcut loop.
        if singles
            .iter()
            .any(|(_, target)| matches!(target, Target::Plugin { .. }))
        {
            return dispatch_unique(singles);
        }
        Resolution::Pass
    }
}

/// Single-step native defaults may have their own GPUI precedence; managed candidates do not.
fn dispatch_unique(mut matches: Vec<(NextStep, Target)>) -> Resolution {
    if matches.len() > 1 {
        return Resolution::Conflict(
            matches
                .iter()
                .map(|(candidate, _)| conflict(candidate))
                .collect(),
        );
    }
    if let Some((candidate, target)) = matches.pop() {
        Resolution::Dispatch {
            operation: candidate.operation,
            target,
        }
    } else {
        Resolution::Pass
    }
}

fn conflict(candidate: &NextStep) -> Conflict {
    Conflict {
        operation: candidate.operation.clone(),
        title: candidate.title.clone(),
        binding: candidate.sequence.clone(),
    }
}

/// Matching at any stack depth keeps ancestor restrictions and whole-stack negation intact.
fn in_context(target: &Target, contexts: &[KeyContext]) -> bool {
    match target {
        Target::Native { predicate, .. } => predicate
            .as_ref()
            .is_none_or(|predicate| predicate.depth_of(contexts).is_some()),
        Target::Plugin { .. } => contexts
            .iter()
            .any(|context| context.contains("EditorShell")),
    }
}

/// Modifier releases emitted by GPUI neither finish a chord nor reset its two-second deadline.
fn modifier_only(stroke: &Keystroke) -> bool {
    stroke.key.is_empty()
        || matches!(
            stroke.key.as_str(),
            "shift" | "control" | "ctrl" | "alt" | "cmd" | "platform" | "super" | "fn" | "function"
        )
}
