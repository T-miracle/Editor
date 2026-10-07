//! Keeps published command ownership tied to the real Manager instance, including same-package restarts.

use super::*;

impl Published {
    /// Advance ownership before blocking retirement; old callbacks must never reach a replacement.
    pub(super) fn retire_instance(&mut self, plugin: &str) {
        *self.instance_epochs.entry(plugin.to_owned()).or_default() += 1;
    }

    /// Compare actual instance IDs without resetting epochs already advanced by early retirement.
    /// The production actor and real-Manager application harness use this same reconciliation.
    pub(in crate::extensions) fn reconcile_instances(&mut self, manager: &Manager) {
        let next = manager
            .live
            .keys()
            .filter_map(|id| {
                manager
                    .instance_id(id)
                    .map(|identity| (id.clone(), identity.to_owned()))
            })
            .collect::<BTreeMap<_, _>>();
        let changed = next
            .iter()
            .filter(|(id, identity)| self.instance_ids.get(*id) != Some(*identity))
            .map(|(id, _)| id.clone())
            .chain(
                self.instance_ids
                    .keys()
                    .filter(|id| !next.contains_key(*id))
                    .cloned(),
            )
            .collect::<Vec<_>>();
        for id in changed {
            self.retire_instance(&id);
        }
        self.instance_ids = next;
    }

    /// Only an opened Manager supplies completion, effective entries and real instance identities.
    pub(in crate::extensions) fn publish_manager(&mut self, manager: &Manager) {
        self.reconcile_instances(manager);
        self.publish_entries(manager.published_entries());
        self.startup.clear();
        self.ready = true;
    }

    /// Return the epoch only for a declared command belonging to a fully published live instance.
    /// Trust is checked by the caller's worker authority, outside this immutable publication.
    pub(in crate::extensions) fn command_epoch(&self, plugin: &str, command: &str) -> Option<u64> {
        if !self.ready
            || self.startup.contains_key(plugin)
            || !self.instance_ids.contains_key(plugin)
        {
            return None;
        }
        self.entries
            .iter()
            .any(|entry| {
                entry.manifest.id == plugin
                    && entry.enabled
                    && entry.error.is_none()
                    && entry
                        .manifest
                        .commands
                        .iter()
                        .any(|item| item.id == command)
            })
            .then(|| self.instance_epochs.get(plugin).copied())
            .flatten()
    }

    /// Reject delayed commands before Manager invocation, preserving the original queued epoch.
    /// Reconciliation here also catches lifecycle operations performed between fixture publications.
    pub(in crate::extensions) fn admit_command(
        &mut self,
        manager: &Manager,
        trusted: bool,
        plugin: &str,
        command: &str,
        expected_epoch: u64,
    ) -> anyhow::Result<()> {
        self.reconcile_instances(manager);
        anyhow::ensure!(
            trusted
                && self.command_epoch(plugin, command) == Some(expected_epoch)
                && manager.instance_id(plugin) == self.instance_ids.get(plugin).map(String::as_str),
            api::Failure::new(
                api::ErrorCode::StaleRevision,
                "Plugin command owner changed"
            )
        );
        Ok(())
    }
}
