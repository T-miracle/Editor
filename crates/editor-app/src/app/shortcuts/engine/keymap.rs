//! Rebuilds the native keymap while preserving exact actions and late control defaults.

use super::*;
use gpui_kit::{DummyKeyboardMapper, KeyBindingContextPredicate as Predicate, Unbind};
use std::rc::Rc;

impl BindingEngine {
    /// Build a complete native keymap without changing unrelated defaults or their priority.
    pub(super) fn native_bindings(
        &self,
        config: &UserBindings,
        activate: Option<&str>,
    ) -> Result<Vec<KeyBinding>, BindingError> {
        let suspended = |id: &str| self.suspended.contains_key(id) && activate != Some(id);
        let mut result = Vec::new();
        let mut removed = Vec::new();
        for binding in &self.defaults {
            if self
                .retired_native
                .iter()
                .any(|target| native_target_matches(target, binding))
            {
                removed.push(binding);
                continue;
            }
            let managed = self
                .operations
                .values()
                .find(|operation| native_target_matches(&operation.target, binding));
            if managed.is_some_and(|operation| {
                config.overrides.contains_key(&operation.id)
                    || binding.keystrokes().len() > 1
                    || suspended(&operation.id)
            }) {
                removed.push(binding);
                continue;
            }
            result.push(binding.clone());
        }
        self.block_ancestor_fallbacks(&mut result, &removed, config)?;
        for operation in self.operations.values() {
            if suspended(&operation.id) {
                continue;
            }
            let Target::Native {
                action,
                predicate,
                action_input,
            } = &operation.target
            else {
                continue;
            };
            let Some(sequences) = config.overrides.get(&operation.id) else {
                continue;
            };
            for sequence in sequences.iter().filter(|sequence| sequence.len() == 1) {
                let binding = KeyBinding::load(
                    &sequence[0],
                    action.boxed_clone(),
                    predicate.clone(),
                    false,
                    action_input.clone(),
                    &DummyKeyboardMapper,
                )
                .map_err(|error| BindingError::NativeBinding(error.to_string()))?;
                result.push(binding);
            }
        }
        Ok(result)
    }

    /// GPUI routes an ancestor's action to the deepest matching handler. Deleting Input Copy
    /// alone therefore leaves Root Copy able to copy from Input with the same old keystroke.
    /// Add a scoped Unbind after defaults, before overrides, only when that exact action still
    /// has another matching scope. Unlike NoAction this preserves unrelated actions on the key.
    fn block_ancestor_fallbacks(
        &self,
        bindings: &mut Vec<KeyBinding>,
        removed: &[&KeyBinding],
        config: &UserBindings,
    ) -> Result<(), BindingError> {
        let mut blockers = Vec::new();
        for removed in removed
            .iter()
            .filter(|binding| binding.keystrokes().len() == 1)
        {
            let removed_target = binding_target(removed);
            let fallback_exists = bindings.iter().any(|binding| {
                binding.keystrokes() == removed.keystrokes()
                    && binding.action().partial_eq(removed.action())
                    && self
                        .profiles
                        .overlap(&removed_target, &binding_target(binding))
            });
            if !fallback_exists {
                continue;
            }
            let key = removed.keystrokes()[0].unparse();
            let has_other_payload = bindings.iter().any(|binding| {
                binding.keystrokes() == removed.keystrokes()
                    && binding.action().name() == removed.action().name()
                    && !binding.action().partial_eq(removed.action())
            }) || self.operations.values().any(|operation| {
                matches!(&operation.target, Target::Native { action, .. }
                    if action.name() == removed.action().name()
                        && !action.partial_eq(removed.action()))
                    && config
                        .overrides
                        .get(&operation.id)
                        .is_some_and(|sequences| {
                            sequences
                                .iter()
                                .any(|sequence| sequence.len() == 1 && sequence[0] == key)
                        })
            });
            if has_other_payload {
                // Unbind identifies an action by name, so it must not consume an opaque sibling
                // such as Enter { secondary: true }. Restrict only the exact fallback variant.
                for binding in bindings.iter_mut().filter(|binding| {
                    binding.keystrokes() == removed.keystrokes()
                        && binding.action().partial_eq(removed.action())
                }) {
                    exclude_removed_scope(binding, removed)?;
                }
            } else {
                blockers.push(
                    KeyBinding::load(
                        &removed.keystrokes()[0].unparse(),
                        Box::new(Unbind(removed.action().name().into())),
                        removed.predicate(),
                        false,
                        None,
                        &DummyKeyboardMapper,
                    )
                    .map_err(|error| BindingError::NativeBinding(error.to_string()))?,
                );
            }
        }
        bindings.extend(blockers);
        Ok(())
    }

    /// Native controls can register after the initial snapshot. Preserve external additions,
    /// but never reclassify our previous user overrides as new defaults after a rebuild.
    pub(super) fn absorb_late_defaults(&mut self, cx: &App) {
        let bindings = cx.key_bindings();
        let bindings = bindings.borrow();
        let mut changed = false;
        for binding in bindings.bindings() {
            if !self
                .last_applied
                .iter()
                .any(|old| same_binding(old, binding))
                && !self.defaults.iter().any(|old| same_binding(old, binding))
            {
                self.defaults.push(binding.clone());
                changed = true;
            }
        }
        if changed {
            self.defaults = normalize_defaults(std::mem::take(&mut self.defaults));
            self.invalidate_pending();
        }
    }

    /// Rebinding the complete collection avoids accumulating stale Unbind/NoAction layers.
    pub(super) fn install(&mut self, bindings: Vec<KeyBinding>, cx: &mut App) {
        self.last_applied = bindings.clone();
        cx.clear_key_bindings();
        cx.bind_keys(bindings);
    }
}

/// Keep scope-overlap decisions on the same audited topology as editing conflict validation.
fn binding_target(binding: &KeyBinding) -> Target {
    Target::Native {
        action: binding.action().boxed_clone(),
        predicate: binding.predicate(),
        action_input: binding.action_input(),
    }
}

/// Exclude only occurrences at or above the removed handler's context. A deeper binding of
/// the same action remains available; different action payloads are never touched at all.
fn exclude_removed_scope(
    binding: &mut KeyBinding,
    removed: &KeyBinding,
) -> Result<(), BindingError> {
    let predicate = match (binding.predicate(), removed.predicate()) {
        (Some(original), Some(removed)) => {
            let same_depth =
                Predicate::And(Box::new((*original).clone()), Box::new((*removed).clone()));
            let ancestor =
                Predicate::Descendant(Box::new((*original).clone()), Box::new((*removed).clone()));
            Some(Rc::new(Predicate::And(
                Box::new((*original).clone()),
                Box::new(Predicate::Not(Box::new(Predicate::Or(
                    Box::new(same_depth),
                    Box::new(ancestor),
                )))),
            )))
        }
        (None, Some(removed)) => Some(Rc::new(Predicate::Not(Box::new((*removed).clone())))),
        (_, None) => {
            // An unscoped removed handler applies everywhere. A false predicate keeps the
            // original action metadata intact while removing only this fallback candidate.
            Some(Rc::new(Predicate::And(
                Box::new(Predicate::Identifier("Root".into())),
                Box::new(Predicate::Not(Box::new(Predicate::Identifier(
                    "Root".into(),
                )))),
            )))
        }
    };
    let previous_meta = binding.meta();
    let mut replacement = KeyBinding::load(
        &binding.keystrokes()[0].unparse(),
        binding.action().boxed_clone(),
        predicate,
        false,
        binding.action_input(),
        &DummyKeyboardMapper,
    )
    .map_err(|error| BindingError::NativeBinding(error.to_string()))?;
    if let Some(meta) = previous_meta {
        replacement.set_meta(meta);
    }
    *binding = replacement;
    Ok(())
}

/// A changed operation suppresses every old binding for this exact payload and original context.
fn native_target_matches(target: &Target, binding: &KeyBinding) -> bool {
    matches!(target, Target::Native { action, predicate, .. }
        if action.partial_eq(binding.action()) && *predicate == binding.predicate())
}

fn same_binding(left: &KeyBinding, right: &KeyBinding) -> bool {
    left.action().partial_eq(right.action())
        && left.keystrokes() == right.keystrokes()
        && left.predicate() == right.predicate()
        && left.action_input() == right.action_input()
        && left.meta() == right.meta()
}

/// Permanently drop superseded same-key/same-predicate defaults. Otherwise removing
/// Ctrl+Backspace's word deletion would unexpectedly revive its earlier generic Backspace rule.
pub(super) fn normalize_defaults(bindings: Vec<KeyBinding>) -> Vec<KeyBinding> {
    bindings
        .iter()
        .enumerate()
        .filter(|(index, binding)| {
            !bindings[index + 1..].iter().any(|later| {
                later.keystrokes() == binding.keystrokes()
                    && later.predicate() == binding.predicate()
            })
        })
        .map(|(_, binding)| binding.clone())
        .collect()
}
