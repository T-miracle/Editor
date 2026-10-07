//! Controlled inline drafts and explicit decisions before changing persisted shortcut bindings.

use super::{
    ShortcutPanel,
    capture::{self, Capture},
    catalog::Operation,
    config::{InvalidBinding, Sequence},
    conflicts::Conflict,
    engine::{BindingEngine, BindingError},
};
use crate::*;
use gpui_kit::{AnyElement, BorrowAppContext as _, Keystroke};

mod view;

/// A draft owns no live binding; its original list protects against concurrent changes.
pub(super) struct Draft {
    id: String,
    index: Option<usize>,
    original: Vec<Sequence>,
    capture: Capture,
    save_focus: FocusHandle,
}

/// Navigation and immediate mutations share the same unsaved-draft guard.
#[derive(Clone)]
pub(super) enum Intent {
    Close,
    Tab(usize),
    ToggleCapture,
    Edit {
        id: String,
        index: Option<usize>,
    },
    Remove {
        id: String,
        index: usize,
    },
    Restore(String),
    /// Re-apply retained custom bindings after lifecycle suspension, never reset to defaults.
    Resolve(String),
}

/// The reviewed candidate is retained until an explicit replacement decision.
#[derive(Clone)]
pub(super) enum Mutation {
    Save {
        id: String,
        bindings: Vec<Sequence>,
        original: Vec<Sequence>,
    },
    Restore(String),
}

/// Leaving never implies saving; replacing authorizes only the displayed revision.
pub(super) enum Confirmation {
    Leave(Intent),
    Replace {
        mutation: Mutation,
        conflicts: Vec<Conflict>,
        revision: u64,
    },
}

impl ShortcutPanel {
    /// Keep the current draft visible even when its description no longer matches the query.
    pub(super) fn is_editing(&self, id: &str) -> bool {
        self.draft.as_ref().is_some_and(|draft| draft.id == id)
    }

    /// An unavailable plugin can no longer own an editable row or a replacement decision.
    /// Keep the user's stored override intact and explain why the transient draft disappeared.
    pub(super) fn cancel_unavailable_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let missing = |id: &str| !self.operations.iter().any(|operation| operation.id == id);
        let unavailable = self.draft.as_ref().is_some_and(|draft| missing(&draft.id))
            || self
                .confirm
                .as_ref()
                .is_some_and(|confirmation| match confirmation {
                    Confirmation::Replace {
                        mutation: Mutation::Save { id, .. } | Mutation::Restore(id),
                        ..
                    } => missing(id),
                    _ => false,
                });
        if unavailable {
            self.draft = None;
            self.confirm = None;
            // Lifecycle cancellation retains the user's lookup mode instead of resetting it.
            if self.key_search {
                self.focus.focus(window, cx);
            } else {
                self.search
                    .update(cx, |search, cx| search.focus(window, cx));
            }
            self.edit_error = Some(t!("shortcuts.edit.plugin_unavailable").to_string());
            cx.notify();
        }
    }

    /// Recording owns Alt/navigation keys only while the capture surface itself has focus.
    pub(super) fn is_recording_binding(&self, window: &Window) -> bool {
        self.draft.is_some() && self.confirm.is_none() && self.focus.is_focused(window)
    }

    /// A damaged configuration leaves query available without enabling unsafe writes.
    fn editing_available(&mut self, cx: &mut Context<Self>) -> bool {
        if cx.has_global::<BindingEngine>() {
            return true;
        }
        if self.edit_error.is_none() {
            self.edit_error = Some(t!("shortcuts.edit.unavailable").to_string());
        }
        cx.notify();
        false
    }

    /// Guard a tab, row, restore, delete or backdrop action before discarding an inline draft.
    pub(super) fn request_edit_intent(
        &mut self,
        intent: Intent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.confirm.is_some() {
            return;
        }
        if self.draft.is_some() {
            self.confirm = Some(Confirmation::Leave(intent));
            self.focus.focus(window, cx);
        } else {
            self.perform_edit_intent(intent, window, cx);
        }
        cx.notify();
    }

    /// Execute only after the owner has settled any pending draft.
    fn perform_edit_intent(&mut self, intent: Intent, window: &mut Window, cx: &mut Context<Self>) {
        let target = match &intent {
            Intent::Edit { id, .. }
            | Intent::Remove { id, .. }
            | Intent::Restore(id)
            | Intent::Resolve(id) => Some(id),
            _ => None,
        };
        if target.is_some_and(|id| !self.operations.iter().any(|operation| operation.id == *id)) {
            // A leave decision can outlive its destination row in this workspace, even while
            // another trusted window keeps the same operation in the shared engine.
            self.edit_error = Some(t!("shortcuts.edit.plugin_unavailable").to_string());
            cx.notify();
            return;
        }
        // Preserve startup/load diagnostics while the panel is restricted to read-only lookup.
        if cx.has_global::<BindingEngine>() {
            self.edit_error = None;
        }
        match intent {
            Intent::Close => self.request_close(window, cx),
            Intent::Tab(tab) => self.select_tab(tab, cx),
            Intent::ToggleCapture => self.toggle_capture(window, cx),
            Intent::Edit { id, index } => {
                if !self.editing_available(cx) {
                    return;
                }
                let original = cx.global::<BindingEngine>().configured(&id);
                // Keep the lookup that located this row while the draft owns key recording.
                // Clearing key search here would expand the list and move Save off screen.
                self.draft = Some(Draft {
                    id,
                    index,
                    original,
                    capture: Capture::default(),
                    save_focus: cx.focus_handle(),
                });
                self.focus.focus(window, cx);
            }
            Intent::Remove { id, index } => {
                if !self.editing_available(cx) {
                    return;
                }
                let result = cx
                    .update_global::<BindingEngine, _>(|engine, cx| engine.remove(&id, index, cx));
                self.finish_edit(result, window, cx);
            }
            Intent::Restore(id) => self.preview_mutation(Mutation::Restore(id), window, cx),
            Intent::Resolve(id) => {
                if !self.editing_available(cx) {
                    return;
                }
                let bindings = cx.global::<BindingEngine>().configured(&id);
                self.preview_mutation(
                    Mutation::Save {
                        id,
                        original: bindings.clone(),
                        bindings,
                    },
                    window,
                    cx,
                );
            }
        }
    }

    /// Escape abandons the draft directly; ordinary modal Escape is handled by the caller.
    pub(super) fn cancel_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.draft.take().is_some() {
            // A completed or abandoned draft returns to ordinary lookup, with a visible input.
            self.key_search = false;
            self.capture.clear();
        }
        self.confirm = None;
        self.edit_error = None;
        if self.key_search {
            self.focus.focus(window, cx);
        } else {
            self.search
                .update(cx, |search, cx| search.focus(window, cx));
        }
        cx.notify();
    }

    /// Return true only for consumed draft input; focused buttons retain native activation.
    pub(super) fn edit_keystroke(
        &mut self,
        key: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if key.key == "escape" && key.modifiers == Modifiers::default() {
            if self.confirm.is_some() {
                self.confirm = None;
                self.resume_draft(window, cx);
                return true;
            }
            if self.draft.is_some() {
                self.cancel_draft(window, cx);
                return true;
            }
        }
        if self.confirm.is_some() || !self.focus.is_focused(window) {
            return false;
        }
        let Some(draft) = self.draft.as_mut() else {
            return false;
        };
        if !draft.capture.record(key, cx.background_executor().now()) {
            return true;
        }
        self.edit_error = None;
        let generation = draft.capture.generation;
        let save_focus = draft.save_focus.clone();
        if !draft.capture.waiting {
            save_focus.focus(window, cx);
        } else {
            // A fresh focus handle identifies the draft incarnation; old timers cannot finish a
            // replacement draft even if its first capture happens to have the same generation.
            cx.spawn_in(window, async move |panel, cx| {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                let _ = panel.update_in(cx, |panel, window, cx| {
                    if let Some(draft) = panel.draft.as_mut()
                        && draft.save_focus == save_focus
                        && draft.capture.generation == generation
                    {
                        draft.capture.waiting = false;
                        if panel.confirm.is_none() && panel.focus.is_focused(window) {
                            draft.save_focus.focus(window, cx);
                        }
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.notify();
        true
    }

    /// Resume the staged capture without erasing it after declining a leave/replacement request.
    fn resume_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(draft) = &self.draft {
            if draft.capture.waiting || draft.capture.strokes.is_empty() {
                self.focus.focus(window, cx);
            } else {
                draft.save_focus.focus(window, cx);
            }
        } else {
            self.search
                .update(cx, |search, cx| search.focus(window, cx));
        }
        cx.notify();
    }

    /// Stage a complete binding list while preserving every untouched sibling binding.
    fn save_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = &self.draft else {
            return;
        };
        let mut bindings = draft.original.clone();
        if let Some(index) = draft.index {
            let Some(binding) = bindings.get_mut(index) else {
                self.edit_error = Some(error_text(BindingError::InvalidIndex));
                cx.notify();
                return;
            };
            *binding = draft.capture.strokes.clone();
        } else {
            bindings.push(draft.capture.strokes.clone());
        }
        self.preview_mutation(
            Mutation::Save {
                id: draft.id.clone(),
                bindings,
                original: draft.original.clone(),
            },
            window,
            cx,
        );
    }

    /// Preview against one current revision; unseen changes never inherit replacement consent.
    fn preview_mutation(
        &mut self,
        mutation: Mutation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.editing_available(cx) {
            return;
        }
        let engine = cx.global::<BindingEngine>();
        let revision = engine.revision();
        let preview = match &mutation {
            Mutation::Save {
                id,
                bindings,
                original,
            } => {
                if engine.configured(id) != *original {
                    Err(BindingError::StaleRevision)
                } else {
                    engine.validate(id, bindings)
                }
            }
            Mutation::Restore(id) => engine.validate_restore(id),
        };
        match preview {
            Ok(conflicts) if !conflicts.is_empty() => {
                self.confirm = Some(Confirmation::Replace {
                    mutation,
                    conflicts,
                    revision,
                });
                self.focus.focus(window, cx);
            }
            Ok(_) => self.commit_mutation(mutation, false, revision, window, cx),
            Err(error) => self.edit_error = Some(error_text(error)),
        }
        cx.notify();
    }

    /// The engine persists before publishing, and removes only the explicitly reviewed collisions.
    fn commit_mutation(
        &mut self,
        mutation: Mutation,
        replace: bool,
        revision: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.editing_available(cx) {
            return;
        }
        let result = cx.update_global::<BindingEngine, _>(|engine, cx| match &mutation {
            Mutation::Save { id, bindings, .. } => {
                engine.save_at_revision(id, bindings, replace, revision, cx)
            }
            Mutation::Restore(id) => engine.restore_at_revision(id, replace, revision, cx),
        });
        if matches!(result, Err(BindingError::StaleRevision)) {
            // Refresh the preview, requiring another click if the conflict set changed.
            self.preview_mutation(mutation, window, cx);
        } else {
            self.finish_edit(result, window, cx);
        }
    }

    /// Keep failed edits visible; update displayed effective bindings only after a committed save.
    fn finish_edit(
        &mut self,
        result: Result<(), BindingError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(()) => {
                // Dispatch sequences selected before a save must not finish under the new map.
                super::runtime::invalidate_all(cx);
                self.sync_effective(cx);
                self.cancel_draft(window, cx);
            }
            Err(error) => self.edit_error = Some(error_text(error)),
        }
        cx.notify();
    }
}

/// Localized summaries expose typed validation categories and preserve storage diagnostics.
fn error_text(error: BindingError) -> String {
    match error {
        BindingError::Invalid(InvalidBinding::StepCount) => {
            t!("shortcuts.edit.step_count").to_string()
        }
        BindingError::Invalid(
            InvalidBinding::Keystroke(detail) | InvalidBinding::ModifierOnly(detail),
        ) => t!("shortcuts.edit.invalid_key", detail = detail).to_string(),
        BindingError::Invalid(InvalidBinding::TextNeedsModifier(_)) => {
            t!("shortcuts.edit.text_modifier").to_string()
        }
        BindingError::Invalid(InvalidBinding::PrefixOverlap) => {
            t!("shortcuts.edit.prefix_overlap").to_string()
        }
        BindingError::Invalid(InvalidBinding::ReservedEscape) => {
            t!("shortcuts.edit.reserved_escape").to_string()
        }
        BindingError::StaleRevision => t!("shortcuts.edit.stale").to_string(),
        BindingError::UnknownOperation(_) | BindingError::InvalidIndex => {
            t!("shortcuts.edit.unavailable").to_string()
        }
        BindingError::Conflicts(_) => t!("shortcuts.edit.conflict").to_string(),
        BindingError::Storage(detail) | BindingError::NativeBinding(detail) => {
            t!("shortcuts.edit.storage", detail = detail).to_string()
        }
    }
}
