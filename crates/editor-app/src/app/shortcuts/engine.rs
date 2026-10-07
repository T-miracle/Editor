//! Applies one shared user configuration to real native and plugin command targets.

use super::{
    catalog::{Operation, Target},
    config::{InvalidBinding, Sequence, UserBindings, normalize, prefixes_overlap},
    conflicts::{Conflict, ContextProfiles},
};
use gpui_kit::{App, Global, KeyBinding, KeyContext};

mod keymap;
#[cfg(test)]
mod tests;
use keymap::normalize_defaults;
use std::{collections::BTreeMap, path::PathBuf};

/// Validation and persistence errors are returned to the controlled editor row.
#[derive(Debug)]
pub(super) enum BindingError {
    UnknownOperation(String),
    Invalid(InvalidBinding),
    InvalidIndex,
    /// A lifecycle or another window changed the previewed conflict set before confirmation.
    StaleRevision,
    Conflicts(Vec<Conflict>),
    Storage(String),
    NativeBinding(String),
}

/// One App global serves every workspace window; per-window pending input lives in Resolver.
/// Defaults include unrelated and NoAction bindings so a rebuild cannot erase native controls.
pub(super) struct BindingEngine {
    path: PathBuf,
    config: UserBindings,
    defaults: Vec<KeyBinding>,
    last_applied: Vec<KeyBinding>,
    operations: BTreeMap<String, Operation>,
    /// Retiring a native target must not reveal its original baseline bindings again.
    retired_native: Vec<Target>,
    suspended: BTreeMap<String, Vec<Conflict>>,
    profiles: ContextProfiles,
    revision: u64,
}

impl Global for BindingEngine {}

impl BindingEngine {
    /// Load before applying overrides. Malformed profiles are reported, never overwritten.
    /// `defaults` must be captured after upstream and local controls register their keymaps.
    pub(super) fn load(path: PathBuf, defaults: Vec<KeyBinding>) -> Result<Self, BindingError> {
        let config = UserBindings::load(&path).map_err(BindingError::Storage)?;
        let last_applied = defaults.clone();
        let defaults = normalize_defaults(defaults);
        Ok(Self {
            path,
            config,
            last_applied,
            defaults,
            operations: BTreeMap::new(),
            retired_native: Vec::new(),
            suspended: BTreeMap::new(),
            profiles: ContextProfiles::default(),
            revision: 1,
        })
    }

    /// Register complete operations, not only rows visible in the current focus snapshot.
    /// Targets retain their exact action payload and predicate through every replacement.
    pub(super) fn register(&mut self, operations: impl IntoIterator<Item = Operation>) -> bool {
        let mut changed = false;
        for operation in operations {
            let differs = self
                .operations
                .get(&operation.id)
                .is_none_or(|old| !same_operation(old, &operation));
            if differs {
                self.retired_native
                    .retain(|target| !same_target(target, &operation.target));
                self.operations.insert(operation.id.clone(), operation);
                changed = true;
            }
        }
        if changed {
            self.invalidate_pending();
        }
        changed
    }

    /// Remove an unavailable target while retaining its user override for later recovery.
    pub(super) fn unregister(&mut self, id: &str) -> bool {
        let removed = self.operations.remove(id);
        if let Some(operation) = &removed
            && matches!(operation.target, Target::Native { .. })
        {
            self.retired_native.push(operation.target.clone());
        }
        self.suspended.remove(id);
        if removed.is_some() {
            self.invalidate_pending();
        }
        removed.is_some()
    }

    /// Reconcile an authoritative set of ready plugin operations without choosing a winner.
    /// A newly restored conflicting plugin is suspended; existing active bindings remain intact.
    /// If two returning plugins conflict with each other, both are suspended and reported.
    pub(super) fn sync_plugin_operations(&mut self, operations: Vec<Operation>) -> Vec<Conflict> {
        let incoming: BTreeMap<_, _> = operations
            .into_iter()
            .filter(|operation| matches!(operation.target, Target::Plugin { .. }))
            .map(|operation| (operation.id.clone(), operation))
            .collect();
        let removed = self
            .operations
            .iter()
            .filter(|(id, operation)| {
                matches!(operation.target, Target::Plugin { .. }) && !incoming.contains_key(*id)
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in removed {
            self.unregister(&id);
        }
        let changed = incoming
            .iter()
            .filter(|(id, operation)| {
                self.operations
                    .get(*id)
                    .is_none_or(|old| !same_operation(old, operation))
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        self.register(incoming.into_values());
        // Compute every result before suspending anyone; ordering must not select a provider.
        let results = changed
            .iter()
            .filter_map(|id| {
                let conflicts = self.conflicts(id, &self.configured(id), true);
                (!conflicts.is_empty()).then(|| (id.clone(), conflicts))
            })
            .collect::<Vec<_>>();
        for id in &changed {
            self.suspended.remove(id);
        }
        let mut conflicts = Vec::new();
        for (id, found) in results {
            conflicts.extend(found.clone());
            self.suspended.insert(id, found);
        }
        conflicts
    }

    /// Stable revision invalidates candidates after configuration or plugin incarnation changes.
    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    /// The lifecycle owner also calls this when a same-digest plugin instance is replaced.
    pub(super) fn invalidate_pending(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Add a source-observed focus path without making unknown predicate names permissive.
    pub(super) fn observe_context(&mut self, contexts: &[KeyContext]) {
        if self.profiles.observe(contexts) {
            self.invalidate_pending();
        }
    }

    /// All original bindings remain available to construct catalogs for other focus contexts.
    pub(super) fn defaults(&self) -> &[KeyBinding] {
        &self.defaults
    }

    /// Absorb late native registrations before rebuilding the complete operation catalog.
    /// Returns whether defaults changed, so bootstrap need not reinstall an unchanged keymap.
    pub(super) fn synchronize_defaults(&mut self, cx: &App) -> bool {
        let revision = self.revision;
        self.absorb_late_defaults(cx);
        self.revision != revision
    }

    /// Return a registered operation, including one awaiting explicit restoration conflict handling.
    pub(super) fn operation(&self, id: &str) -> Option<&Operation> {
        self.operations.get(id)
    }

    /// Active operations are still subject to current context, handler, trust and instance checks.
    pub(super) fn active_operations(&self) -> impl Iterator<Item = &Operation> {
        self.operations
            .values()
            .filter(|operation| !self.suspended.contains_key(&operation.id))
    }

    /// Effective sequences contain no bindings for an unavailable or suspended operation.
    pub(super) fn effective(&self, id: &str) -> Vec<Sequence> {
        if self.suspended.contains_key(id) {
            Vec::new()
        } else {
            self.configured(id)
        }
    }

    /// The editor may show retained bindings even while a restored plugin awaits conflict resolution.
    pub(super) fn configured(&self, id: &str) -> Vec<Sequence> {
        self.config.overrides.get(id).cloned().unwrap_or_else(|| {
            self.operations
                .get(id)
                .map(|operation| operation.defaults.clone())
                .unwrap_or_default()
        })
    }

    /// Return the conflicts that prevent a restored plugin from becoming active.
    pub(super) fn suspended_conflicts(&self, id: &str) -> &[Conflict] {
        self.suspended
            .get(id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// Inspect a draft without changing persisted configuration, active bindings or pending input.
    pub(super) fn validate(
        &self,
        id: &str,
        bindings: &[Sequence],
    ) -> Result<Vec<Conflict>, BindingError> {
        self.require_operation(id)?;
        let bindings = self.normalize_draft(id, bindings)?;
        Ok(self.conflicts(id, &bindings, false))
    }

    /// Save a complete operation binding list; replacement removes only the reported sequences.
    /// Persistence failure leaves both the old in-memory configuration and native keymap unchanged.
    pub(super) fn save(
        &mut self,
        id: &str,
        bindings: &[Sequence],
        replace_conflicts: bool,
        cx: &mut App,
    ) -> Result<(), BindingError> {
        self.require_operation(id)?;
        let bindings = self.normalize_draft(id, bindings)?;
        let conflicts = self.conflicts(id, &bindings, false);
        let mut config = self.replacement_config(&conflicts, replace_conflicts)?;
        config.overrides.insert(id.to_owned(), bindings);
        self.commit(config, Some(id), cx)
    }

    /// A conflict confirmation authorizes only the revision that the user actually reviewed.
    /// The UI re-renders a fresh preview on StaleRevision rather than replacing unseen bindings.
    pub(super) fn save_at_revision(
        &mut self,
        id: &str,
        bindings: &[Sequence],
        replace_conflicts: bool,
        expected_revision: u64,
        cx: &mut App,
    ) -> Result<(), BindingError> {
        if self.revision != expected_revision {
            return Err(BindingError::StaleRevision);
        }
        self.save(id, bindings, replace_conflicts, cx)
    }

    /// Remove one displayed sequence without deleting the operation's other shortcuts.
    pub(super) fn remove(
        &mut self,
        id: &str,
        index: usize,
        cx: &mut App,
    ) -> Result<(), BindingError> {
        self.require_operation(id)?;
        let mut bindings = self.configured(id);
        if index >= bindings.len() {
            return Err(BindingError::InvalidIndex);
        }
        bindings.remove(index);
        let mut config = self.config.clone();
        config.overrides.insert(id.to_owned(), bindings);
        // Removing cannot introduce a conflict; remaining conflicts keep a suspended target paused.
        let conflicts = self.conflicts(id, config.overrides.get(id).unwrap(), false);
        self.commit(config, conflicts.is_empty().then_some(id), cx)?;
        if self.suspended.contains_key(id) {
            self.suspended.insert(id.to_owned(), conflicts);
        }
        Ok(())
    }

    /// Preview restoration through the same collision detector without mutating the draft.
    pub(super) fn validate_restore(&self, id: &str) -> Result<Vec<Conflict>, BindingError> {
        let operation = self.require_operation(id)?;
        Ok(self.conflicts(id, &operation.defaults, false))
    }

    /// Restore inherited defaults after the same explicit conflict decision used by normal saves.
    pub(super) fn restore(
        &mut self,
        id: &str,
        replace_conflicts: bool,
        cx: &mut App,
    ) -> Result<(), BindingError> {
        let operation = self.require_operation(id)?;
        let conflicts = self.conflicts(id, &operation.defaults, false);
        let mut config = self.replacement_config(&conflicts, replace_conflicts)?;
        config.overrides.remove(id);
        self.commit(config, Some(id), cx)
    }

    /// Restoring defaults uses the same reviewed-revision guarantee as editing a sequence.
    pub(super) fn restore_at_revision(
        &mut self,
        id: &str,
        replace_conflicts: bool,
        expected_revision: u64,
        cx: &mut App,
    ) -> Result<(), BindingError> {
        if self.revision != expected_revision {
            return Err(BindingError::StaleRevision);
        }
        self.restore(id, replace_conflicts, cx)
    }

    /// Install current native single-step bindings after startup or catalog synchronization.
    /// Plugin actions and two-step sequences are resolved through the predispatch Resolver.
    pub(super) fn apply(&mut self, cx: &mut App) -> Result<(), BindingError> {
        self.absorb_late_defaults(cx);
        self.validate_configuration(&self.config)?;
        let bindings = self.native_bindings(&self.config, None)?;
        self.install(bindings, cx);
        Ok(())
    }

    /// Reload only after the file has parsed; invalid data leaves the running profile intact.
    pub(super) fn reload(&mut self, cx: &mut App) -> Result<(), BindingError> {
        let config = UserBindings::load(&self.path).map_err(BindingError::Storage)?;
        self.absorb_late_defaults(cx);
        self.validate_configuration(&config)?;
        let bindings = self.native_bindings(&config, None)?;
        self.config = config;
        self.invalidate_pending();
        self.install(bindings, cx);
        Ok(())
    }

    /// Validate only changed sequences against new-binding restrictions. Native defaults such
    /// as Tree's bare j and Input's Escape must remain editable alongside a new sibling binding.
    /// Matching an individual step is insufficient: only an entire unchanged sequence is exempt.
    fn normalize_draft(
        &self,
        id: &str,
        bindings: &[Sequence],
    ) -> Result<Vec<Sequence>, BindingError> {
        let bindings = normalize(bindings, false).map_err(BindingError::Invalid)?;
        let existing = self.configured(id);
        for sequence in &bindings {
            if !existing.contains(sequence) {
                normalize(std::slice::from_ref(sequence), true).map_err(BindingError::Invalid)?;
            }
        }
        Ok(bindings)
    }

    /// Compare every active operation across all registered scopes, not the currently visible tab.
    fn conflicts(
        &self,
        id: &str,
        candidates: &[Sequence],
        include_suspended: bool,
    ) -> Vec<Conflict> {
        let Some(operation) = self.operations.get(id) else {
            return Vec::new();
        };
        let mut result = Vec::new();
        for other in self.operations.values() {
            if other.id == id
                || (!include_suspended && self.suspended.contains_key(&other.id))
                || !self.profiles.overlap(&operation.target, &other.target)
            {
                continue;
            }
            for binding in self.configured(&other.id) {
                if candidates
                    .iter()
                    .any(|candidate| prefixes_overlap(candidate, &binding))
                {
                    result.push(Conflict {
                        operation: other.id.clone(),
                        title: other.title.clone(),
                        binding,
                    });
                }
            }
        }
        result
    }

    /// Materialize all explicit removals before touching the file or native keymap.
    fn replacement_config(
        &self,
        conflicts: &[Conflict],
        replace: bool,
    ) -> Result<UserBindings, BindingError> {
        if !replace && !conflicts.is_empty() {
            return Err(BindingError::Conflicts(conflicts.to_vec()));
        }
        let mut config = self.config.clone();
        for conflict in conflicts {
            let bindings = config
                .overrides
                .entry(conflict.operation.clone())
                .or_insert_with(|| self.configured(&conflict.operation));
            bindings.retain(|binding| binding != &conflict.binding);
        }
        Ok(config)
    }

    /// Hand-edited or restored profiles may conflict with newly added defaults. Validate
    /// all active overrides before publishing them, without rejecting untouched legacy defaults.
    fn validate_configuration(&self, config: &UserBindings) -> Result<(), BindingError> {
        let mut conflicts = Vec::new();
        for operation in self
            .active_operations()
            .filter(|operation| config.overrides.contains_key(&operation.id))
        {
            let candidates = &config.overrides[&operation.id];
            for other in self.active_operations() {
                if other.id == operation.id
                    || !self.profiles.overlap(&operation.target, &other.target)
                {
                    continue;
                }
                let bindings = config.overrides.get(&other.id).unwrap_or(&other.defaults);
                for binding in bindings {
                    if candidates
                        .iter()
                        .any(|candidate| prefixes_overlap(candidate, binding))
                    {
                        let conflict = Conflict {
                            operation: other.id.clone(),
                            title: other.title.clone(),
                            binding: binding.clone(),
                        };
                        if !conflicts.contains(&conflict) {
                            conflicts.push(conflict);
                        }
                    }
                }
            }
        }
        if conflicts.is_empty() {
            Ok(())
        } else {
            Err(BindingError::Conflicts(conflicts))
        }
    }

    /// Build first, persist second, publish last; no fallible work follows the write.
    fn commit(
        &mut self,
        config: UserBindings,
        activate: Option<&str>,
        cx: &mut App,
    ) -> Result<(), BindingError> {
        self.absorb_late_defaults(cx);
        let bindings = self.native_bindings(&config, activate)?;
        config.save(&self.path).map_err(BindingError::Storage)?;
        self.config = config;
        if let Some(id) = activate {
            self.suspended.remove(id);
        }
        self.invalidate_pending();
        self.install(bindings, cx);
        Ok(())
    }

    fn require_operation(&self, id: &str) -> Result<&Operation, BindingError> {
        self.operations
            .get(id)
            .ok_or_else(|| BindingError::UnknownOperation(id.to_owned()))
    }
}

/// Equality includes opaque action payloads; equal labels or action names are insufficient.
fn same_operation(left: &Operation, right: &Operation) -> bool {
    left.id == right.id
        && left.title == right.title
        && left.scope == right.scope
        && left.defaults == right.defaults
        && same_target(&left.target, &right.target)
}

fn same_target(left: &Target, right: &Target) -> bool {
    match (left, right) {
        (
            Target::Native {
                action: left,
                predicate: left_scope,
                action_input: left_input,
            },
            Target::Native {
                action: right,
                predicate: right_scope,
                action_input: right_input,
            },
        ) => {
            left.partial_eq(right.as_ref())
                && left_scope == right_scope
                && left_input == right_input
        }
        (
            Target::Plugin {
                plugin: left,
                command: left_command,
            },
            Target::Plugin {
                plugin: right,
                command: right_command,
            },
        ) => left == right && left_command == right_command,
        _ => false,
    }
}
