//! Validate panel ownership, input revisions and preview authority before invoking a guest.
use super::*;

impl Manager {
    pub fn event(&mut self, id: &str, event: Event) -> anyhow::Result<()> {
        self.refresh_services();
        // Never deliver document text to a guest without the same editor permission as native reads.
        let mut inner = &event;
        let mut surfaces = Vec::new();
        while let Event::Surface { panel, event } = inner {
            surfaces.push(panel);
            inner = event;
        }
        if let Event::Capability(api::Notification::Preview { document, text }) = inner {
            let entry = self
                .installed
                .get(id)
                .ok_or_else(|| api::Failure::new(api::ErrorCode::NotFound, "Unknown plugin"))?;
            if surfaces.len() != 1
                || entry.manifest.protocol != 7
                || !entry.grants.contains("editor.read")
                || entry.manifest.scope != api::InstanceScope::Workspace
                || !entry
                    .manifest
                    .panels
                    .iter()
                    .any(|panel| panel.id == *surfaces[0] && panel.position == "editor")
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
            if let (Some(Some(current)), Some(next)) =
                (instance.preview_sources.get(surfaces[0]), document)
                && current.id == next.id
                && next.revision < current.revision
            {
                return Err(api::Failure::new(
                    api::ErrorCode::StaleRevision,
                    "Preview document is obsolete",
                )
                .into());
            }
            instance
                .preview_sources
                .insert(surfaces[0].clone(), document.clone());
        }
        if self
            .installed
            .get(id)
            .is_some_and(|entry| entry.manifest.protocol == 7)
            && let Event::Ui(event) | Event::Capability(api::Notification::Ui(event)) = inner
        {
            if surfaces.len() != 1 {
                return Err(api::Failure::new(
                    api::ErrorCode::InvalidRequest,
                    "UI events require one owning panel",
                )
                .into());
            }
            let document = self
                .live
                .get(id)
                .and_then(|instance| instance.scenes.get(surfaces[0]))
                .and_then(|scene| scene.ui.as_ref())
                .ok_or_else(|| {
                    api::Failure::new(api::ErrorCode::InvalidHandle, "UI panel is unavailable")
                })?;
            // Reject stale host callbacks before calling WASM; they are not plugin crashes.
            document.validate_event(event)?;
        }
        if matches!(inner, Event::Document { .. }) {
            anyhow::ensure!(
                surfaces.len() == 1,
                "Document events must be scoped to one editor preview surface"
            );
            let entry = self
                .installed
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("Unknown plugin"))?;
            anyhow::ensure!(
                entry.manifest.protocol >= 6
                    && entry.grants.contains("editor.commands")
                    && entry.manifest.panels.iter().any(|descriptor| {
                        descriptor.id == *surfaces[0] && descriptor.position == "editor"
                    }),
                "Document previews require a declared surface and editor.commands grant"
            );
        }
        let instance = self
            .live
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("Plugin is disabled"))?;
        if let Err(error) = instance.call(Message::Event(event)) {
            instance.stop();
            if let Some(entry) = self.installed.get_mut(id) {
                entry.error = Some(format!("{error:#}"));
            }
            return Err(error);
        }
        Ok(())
    }
}
