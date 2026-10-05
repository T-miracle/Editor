//! Native migration imports opaque owner data once, without interpreting guest display semantics.
use super::*;

impl Manager {
    /// Import a missing private preference for a live workspace plugin on the runtime worker.
    /// Returns whether a record was created; existing values win, including repeated imports.
    /// Requires current trust, `storage` consent and negotiated `storage.private >=1.1`.
    /// Corrupt records, quota errors or missing owners remain explicit failures with data preserved.
    pub fn import_preference(
        &mut self,
        id: &str,
        key: api::PreferenceKey,
        data: serde_json::Value,
    ) -> anyhow::Result<bool> {
        let entry = self.installed.get(id).ok_or_else(|| {
            api::Failure::new(api::ErrorCode::NotFound, "Preference owner is unknown")
        })?;
        if !self.trusted
            || !self.workspace_open
            || entry.manifest.scope != api::InstanceScope::Workspace
            || !entry.grants.contains("storage")
        {
            return Err(api::Failure::new(
                api::ErrorCode::PermissionDenied,
                "Preference import requires a trusted owning workspace and storage consent",
            )
            .into());
        }
        let instance = self.live.get_mut(id).ok_or_else(|| {
            api::Failure::new(api::ErrorCode::InvalidState, "Preference owner is disabled")
        })?;
        Ok(instance.import_preference(key, data)?)
    }
}
