//! Native UI operations keep the guest's presentation and resource checks intact.
use super::Manager;
use crate::TerminalPresentation;
use plugin_protocol::api::ResourceHandle;

impl Manager {
    /// Explicit host closure affects only the presented resource, retaining its actual exit observer.
    pub fn terminal_exit(
        &mut self,
        handle: &ResourceHandle,
        mode: plugin_protocol::process::ExitMode,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.trusted && self.workspace_open,
            "Restricted/closed workspace cannot use terminals"
        );
        let instance = self
            .live
            .values_mut()
            .find(|instance| instance.has_presented_terminal(handle))
            .ok_or_else(|| anyhow::anyhow!("Terminal owner is no longer active"))?;
        let effective = match instance.exit_presented(handle, mode) {
            Ok(()) => mode,
            Err(error)
                if mode == plugin_protocol::process::ExitMode::Graceful
                    && error.code == plugin_protocol::api::ErrorCode::UnsupportedOperation =>
            {
                // Stdio has no universal graceful protocol; an explicit host close still converges.
                instance
                    .exit_presented(handle, plugin_protocol::process::ExitMode::Force)
                    .map_err(|error| anyhow::anyhow!(error.message))?;
                plugin_protocol::process::ExitMode::Force
            }
            Err(error) => return Err(anyhow::anyhow!(error.message)),
        };
        self.host_resources.terminals.stopping(handle, effective);
        Ok(())
    }
    /// A noncooperative presented process retains its controls until force and actual native EOF.
    pub(super) fn advance_terminal_exits(&mut self) {
        for handle in self.host_resources.terminals.overdue() {
            if let Err(error) =
                self.terminal_exit(&handle, plugin_protocol::process::ExitMode::Force)
            {
                self.host_resources
                    .terminals
                    .fail(&handle, error.to_string());
            }
        }
    }
    /// Consume only projections explicitly requested through the public guest API.
    pub fn take_terminal_presentations(&self) -> Vec<TerminalPresentation> {
        if !self.trusted || !self.workspace_open {
            return Vec::new();
        }
        self.host_resources.terminals.take(&self.host_scope())
    }
    /// Revoke workspace views before parking their instances, keeping the native close observers.
    pub(super) fn retire_workspace_terminals(&mut self) {
        let scope = self.host_scope();
        for handle in self.host_resources.terminals.handles(&scope) {
            if let Some(instance) = self
                .live
                .values_mut()
                .find(|instance| instance.has_presented_terminal(&handle))
            {
                instance.release_presented(&handle);
            }
        }
        self.host_resources.terminals.discard_scope(&scope);
    }
    /// Deliver literal input to an active, presented resource in this trusted workspace.
    pub fn terminal_input(&mut self, handle: &ResourceHandle, bytes: &[u8]) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.trusted && self.workspace_open,
            "Restricted/closed workspace cannot use terminals"
        );
        let instance = self
            .live
            .values_mut()
            .find(|instance| instance.has_presented_terminal(handle))
            .ok_or_else(|| anyhow::anyhow!("Terminal owner is no longer active"))?;
        instance
            .write_presented(handle, bytes)
            .map_err(|error| anyhow::anyhow!(error.message))
    }
    /// Geometry reaches the original PTY; no new execution or cross-scope resource is created.
    pub fn terminal_resize(
        &mut self,
        handle: &ResourceHandle,
        columns: u16,
        rows: u16,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.trusted && self.workspace_open,
            "Restricted/closed workspace cannot use terminals"
        );
        let instance = self
            .live
            .values_mut()
            .find(|instance| instance.has_presented_terminal(handle))
            .ok_or_else(|| anyhow::anyhow!("Terminal owner is no longer active"))?;
        instance
            .resize_presented(handle, columns, rows)
            .map_err(|error| anyhow::anyhow!(error.message))
    }
}
