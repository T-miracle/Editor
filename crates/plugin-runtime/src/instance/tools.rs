//! Tool metadata is accepted only for an owned surface and immutable package artwork.
use super::*;
use api::{ErrorCode, Failure};
use std::io::Read;

impl Instance {
    /// Validate target permissions and all SVG bytes before publishing any part of a guest reply.
    pub(super) fn check_tool_authority(
        &self,
        panel: &str,
        document: &ui::Document,
    ) -> Result<(), Failure> {
        if document.tools.is_empty() {
            return Ok(());
        }
        let state = self.store.data();
        if !state.api.capabilities.contains_key("ui.tools") {
            return Err(Failure::new(
                ErrorCode::CapabilityUnavailable,
                "ui.tools was not negotiated",
            ));
        }
        for tool in &document.tools {
            match &tool.target {
                ui::ToolTarget::File { .. }
                    if !state.roots.application
                        && state.permissions.contains("editor.read")
                        && state.declared_editor_panels.contains(panel) => {}
                ui::ToolTarget::Window { panel: target }
                    if target == panel
                        && state.declared_panels.contains(panel)
                        && !state.declared_editor_panels.contains(panel) => {}
                _ => {
                    return Err(Failure::new(
                        ErrorCode::PermissionDenied,
                        "Tool must target its file or own independent window",
                    ));
                }
            }
            for path in [&tool.icon.light, &tool.icon.dark] {
                let path = safe_path(&state.assets, path, true)
                    .map_err(|error| Failure::new(ErrorCode::InvalidPath, error.to_string()))?;
                let file = std::fs::File::open(path)
                    .map_err(|error| Failure::new(ErrorCode::NotFound, error.to_string()))?;
                let mut bytes = Vec::new();
                file.take(64 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|error| Failure::new(ErrorCode::OperationFailed, error.to_string()))?;
                crate::package::icons::svg(&bytes)
                    .map_err(|error| Failure::new(ErrorCode::InvalidRequest, error.to_string()))?;
            }
        }
        Ok(())
    }
}
