//! Validate panel ownership, input revisions and preview authority before invoking a guest.
use super::*;

impl Manager {
    /// Route a notification with an explicit owning panel; never infer ownership from guest identity.
    pub fn event(
        &mut self,
        id: &str,
        panel: Option<String>,
        event: api::Notification,
    ) -> anyhow::Result<()> {
        if matches!(&event, api::Notification::ImageInput { .. }) {
            return Err(api::Failure::new(
                api::ErrorCode::InvalidRequest,
                "Image input must originate from a native owned offer",
            )
            .into());
        }
        self.refresh_services();
        // Never deliver document text to a guest without the same editor permission as native reads.
        let inner = &event;
        if let api::Notification::Preview { document, text } = inner {
            let panel_id = panel.as_ref();
            let entry = self
                .installed
                .get(id)
                .ok_or_else(|| api::Failure::new(api::ErrorCode::NotFound, "Unknown plugin"))?;
            if panel.is_none()
                || !entry.grants.contains("editor.read")
                || entry.manifest.scope != api::InstanceScope::Workspace
                || !entry
                    .manifest
                    .panels
                    .iter()
                    .any(|panel| Some(&panel.id) == panel_id && panel.position == "editor")
            {
                return Err(api::Failure::new(
                    api::ErrorCode::PermissionDenied,
                    "Preview requires a workspace-owned declared surface and editor.read grant",
                )
                .into());
            }
            if text.len() > 1024 * 1024 || (document.is_none() && !text.is_empty()) {
                return Err(api::Failure::new(
                    api::ErrorCode::LimitExceeded,
                    "Invalid or oversized preview document",
                )
                .into());
            }
            let instance = self.live.get_mut(id).ok_or_else(|| {
                api::Failure::new(api::ErrorCode::InvalidState, "Preview plugin is disabled")
            })?;
            // Late memory notifications must not roll back source authority or retire a healthy guest.
            if let (Some(Some(current)), Some(next)) = (
                instance
                    .preview_sources
                    .get(panel.as_ref().expect("validated panel")),
                document,
            ) && current.id == next.id
                && next.revision < current.revision
            {
                return Err(api::Failure::new(
                    api::ErrorCode::StaleRevision,
                    "Preview document is obsolete",
                )
                .into());
            }
            instance.preview_sources.insert(
                panel.as_ref().expect("validated panel").clone(),
                document.clone(),
            );
            // Revoke old offers before the new Preview hook can attempt a save using their handles.
            // Accepted writers retain their original payload and may still report completed files.
            instance.reconcile_image_inputs();
        }
        if let api::Notification::Ui(event) = inner {
            if panel.is_none() {
                return Err(api::Failure::new(
                    api::ErrorCode::InvalidRequest,
                    "UI events require one owning panel",
                )
                .into());
            }
            let document = self
                .live
                .get(id)
                .and_then(|instance| instance.views.get(panel.as_ref().expect("validated panel")))
                .ok_or_else(|| {
                    api::Failure::new(api::ErrorCode::InvalidHandle, "UI panel is unavailable")
                })?;
            // Reject stale host callbacks before calling WASM; they are not plugin crashes.
            document.validate_event(event)?;
        }
        let instance = self
            .live
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("Plugin is disabled"))?;
        if let Err(error) = instance.notify(panel, event) {
            instance.stop();
            if let Some(entry) = self.installed.get_mut(id) {
                entry.error = Some(format!("{error:#}"));
            }
            return Err(error);
        }
        // Replacing or withdrawing a declaration cancels its resource before returning to publication.
        self.reconcile_images();
        Ok(())
    }
}
