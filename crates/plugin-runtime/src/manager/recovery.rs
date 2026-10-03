//! Recovery replaces one committed instance and leaves peer instances and persisted data intact.
use super::*;
impl Manager {
    pub fn diagnostics(&self, id: &str) -> Vec<crate::faults::Diagnostic> {
        let mut history = self.diagnostic_history.get(id).cloned().unwrap_or_default();
        if let Some(instance) = self.live.get(id) {
            history.extend(instance.diagnostics.clone());
        }
        // Background native teardown may finish after disable/uninstall removed the instance.
        history.extend(self.native_diagnostics.for_plugin(id));
        crate::faults::latest(history)
    }
    /// Explicit recovery does not replay failed commands or discard the last committed snapshot.
    pub fn restart_plugin(&mut self, id: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.trusted && self.workspace_open,
            "Workspace is restricted or closed"
        );
        anyhow::ensure!(
            self.published_entries()
                .iter()
                .any(|entry| entry.manifest.id == id && entry.enabled),
            "Enable this plugin before restarting it"
        );
        let enabled = self
            .installed
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("Unknown plugin"))?
            .enabled;
        // Prefer a current safe checkpoint; a trapping guest leaves the last committed checkpoint intact.
        if let Some(instance) = self
            .live
            .get_mut(id)
            .filter(|instance| instance.service_provider().is_some())
        {
            if let Ok(snapshot) = instance.snapshot() {
                self.save_snapshot(id, &snapshot)?;
            }
        }
        // The shared native sink survives restart itself; do not copy and later duplicate its entries.
        let mut history = self.diagnostic_history.get(id).cloned().unwrap_or_default();
        if let Some(instance) = self.live.get(id) {
            history.extend(instance.diagnostics.clone());
        }
        let excess = history.len().saturating_sub(32);
        history.drain(..excess);
        self.diagnostic_history.insert(id.into(), history);
        if let Some(mut instance) = self.live.remove(id) {
            instance.stop();
        }
        self.retire_language_services(id);
        self.refresh_services();
        let result = self.enable(id);
        let entry = self.installed.get_mut(id).unwrap();
        entry.enabled = enabled;
        if let Err(error) = &result {
            entry.error = Some(format!("Restart failed: {error:#}"));
        }
        self.save_registry()?;
        result
    }
}
