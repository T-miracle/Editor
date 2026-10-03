//! Native image capture is an explicit host offer, never a forgeable guest or generic event payload.
use super::*;
use crate::{HostImageInput, HostImageOrigin, image_input};

impl Manager {
    /// Offer a finite native batch to its current source-bound editor preview. Clipboard origins
    /// require clipboard consent; all origins require editor.read/editor.write/workspace.write.
    /// The caller must reject retired native callback epochs before calling this method.
    pub fn offer_image_input(
        &mut self,
        id: &str,
        panel: &str,
        document: api::DocumentVersion,
        selection: api::TextRange,
        origin: HostImageOrigin,
        images: Vec<HostImageInput>,
    ) -> anyhow::Result<()> {
        image_input::validate_batch(&images)?;
        if !self.trusted || !self.workspace_open {
            return Err(api::Failure::new(
                api::ErrorCode::PermissionDenied,
                "No trusted active workspace image authority",
            )
            .into());
        }
        if selection.start > selection.end || selection.end > 1024 * 1024 {
            return Err(api::Failure::new(
                api::ErrorCode::InvalidRequest,
                "Image input selection is invalid",
            )
            .into());
        }
        // A source is an existing workspace-relative document; native creation revalidates its canonical parent.
        let path = crate::instance::safe_path(
            Path::new(&self.environment.workspace),
            &document.path,
            true,
        )
        .map_err(|error| api::Failure::new(api::ErrorCode::InvalidPath, format!("{error:#}")))?;
        if !path.is_file() {
            return Err(api::Failure::new(
                api::ErrorCode::InvalidPath,
                "Image input requires a saved document file",
            )
            .into());
        }
        let instance = self.live.get_mut(id).ok_or_else(|| {
            api::Failure::new(
                api::ErrorCode::InvalidState,
                "Image input owner is disabled",
            )
        })?;
        instance.reconcile_image_inputs();
        if instance.preview_sources.get(panel).and_then(Option::as_ref) != Some(&document)
            || self.retired_image_sources.get(&format!("{id}/{panel}")) == Some(&document)
            || !instance.views.get(panel).is_some_and(|view| {
                view.editor_image_input && view.source.as_ref() == Some(&document)
            })
        {
            return Err(api::Failure::new(
                api::ErrorCode::StaleRevision,
                "Image input preview is no longer current or opted in",
            )
            .into());
        }
        let metadata = instance.grant_image_inputs(
            panel,
            document.clone(),
            selection,
            origin,
            images,
            self.image_input_budget.clone(),
        )?;
        if let Err(error) = instance.notify(
            Some(panel.into()),
            api::Notification::ImageInput {
                document,
                selection,
                images: metadata,
            },
        ) {
            instance.stop();
            if let Some(entry) = self.installed.get_mut(id) {
                entry.error = Some(format!("{error:#}"));
            }
            return Err(error);
        }
        self.reconcile_images();
        Ok(())
    }

    /// Selected-workspace capture tokens cannot survive parking, even when the guest itself remains warm.
    pub(super) fn retire_workspace_image_inputs(&mut self) {
        for instance in self.live.values_mut() {
            instance.clear_image_inputs();
        }
    }
}
