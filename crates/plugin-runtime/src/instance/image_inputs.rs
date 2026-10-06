//! Instance-owned image offers expire independently from immutable, already accepted save requests.
use super::*;
use crate::{HostImageInput, HostImageOrigin, ImageInputResource, image_input};
use api::{ErrorCode, Failure, ResourceHandle};
use resource_roots::RootKind;
use std::{
    sync::{Arc, atomic::AtomicUsize},
    time::{Duration, Instant},
};

pub(super) struct Input {
    resource: Arc<ImageInputResource>,
    panel: String,
    expires: Instant,
    /// One input cannot create duplicate concurrent files; conflicts release this slot for a retry.
    in_flight: Option<ResourceHandle>,
}

impl State {
    /// Publication itself requires read/write editor authority, while capture additionally needs workspace.write.
    pub(super) fn check_image_input_authority(&self, panel: &str) -> Result<(), Failure> {
        if !self.api.capabilities.contains_key("editor.images") {
            return Err(Failure::new(
                ErrorCode::CapabilityUnavailable,
                "editor.images was not negotiated",
            ));
        }
        if self.roots.application
            || !self.declared_editor_panels.contains(panel)
            || !["editor.read", "editor.write"]
                .iter()
                .all(|permission| self.permissions.contains(*permission))
        {
            return Err(Failure::new(
                ErrorCode::PermissionDenied,
                "Image input requires an owned workspace editor panel and editor.read/editor.write",
            ));
        }
        Ok(())
    }

    /// A native input cannot be borrowed through a service invocation or interpreted as a file handle.
    pub(super) fn image_input_payload(
        &self,
        handle: &ResourceHandle,
        name: &str,
    ) -> Result<Arc<ImageInputResource>, Failure> {
        if !matches!(self.roots.resolve(handle)?, RootKind::ImageInput) {
            return Err(Failure::new(
                ErrorCode::InvalidHandle,
                "Not an image input handle",
            ));
        }
        if self.plugin_services.context.is_some() || !self.permissions.contains("workspace.write") {
            return Err(Failure::new(
                ErrorCode::PermissionDenied,
                "Native image input requires workspace.write and its original owner",
            ));
        }
        let input = self
            .image_inputs
            .get(&handle.resource)
            .filter(|input| input.expires > Instant::now())
            .ok_or_else(|| {
                Failure::new(
                    ErrorCode::InvalidHandle,
                    "Image input expired or was withdrawn",
                )
            })?;
        self.check_image_input_authority(&input.panel)?;
        if input.in_flight.is_some() {
            return Err(Failure::new(
                ErrorCode::Conflict,
                "Image input already has an accepted save",
            ));
        }
        image_input::validate_name(name, input.resource.input.format)?;
        Ok(input.resource.clone())
    }

    /// Success consumes the token; a failed or unexecuted write permits collision-name retry.
    pub(super) fn finish_image_input_request(
        &mut self,
        call: &crate::EditorRequest,
        update: &api::RequestUpdate,
    ) {
        let api::EditorOperation::SaveImageInput { input, .. } = call.operation() else {
            return;
        };
        let Some(owned) = self.image_inputs.get_mut(&input.resource) else {
            return;
        };
        if owned.in_flight.as_ref() != Some(call.handle()) {
            return;
        }
        match update {
            api::RequestUpdate::Completed {
                result: Ok(api::EditorValue::ImageSaved { .. }),
            } => {
                self.image_inputs.remove(&input.resource);
                self.roots.remove(input);
            }
            api::RequestUpdate::Completed { result: Err(_) }
            | api::RequestUpdate::Cancelled {
                effect: api::CancellationEffect::NotExecuted,
                ..
            } => owned.in_flight = None,
            // WaitingStopped gives no proof that a filesystem effect did not occur; never retry it.
            _ => {}
        }
    }

    pub(super) fn mark_image_input_pending(
        &mut self,
        input: &ResourceHandle,
        request: ResourceHandle,
    ) {
        self.image_inputs
            .get_mut(&input.resource)
            .expect("validated image input")
            .in_flight = Some(request);
    }
}

impl Instance {
    /// The manager is the only allocator; opaque metadata is delivered after the complete batch reserves its budget.
    pub(crate) fn grant_image_inputs(
        &mut self,
        panel: &str,
        document: api::DocumentVersion,
        selection: api::TextRange,
        origin: HostImageOrigin,
        images: Vec<HostImageInput>,
        budget: Arc<AtomicUsize>,
    ) -> Result<Vec<api::ImageInput>, Failure> {
        let state = self.store.data_mut();
        state.check_image_input_authority(panel)?;
        if !state.active
            || state.roots.retired
            || !state.permissions.contains("workspace.write")
            || (origin == HostImageOrigin::Clipboard && !state.permissions.contains("clipboard"))
        {
            return Err(Failure::new(
                ErrorCode::PermissionDenied,
                "Image capture needs workspace.write and clipboard consent for paste",
            ));
        }
        let mut allocated = Vec::new();
        let result = (|| {
            for image in images {
                let api::Value::Resource(handle) = state.roots.open(RootKind::ImageInput)? else {
                    unreachable!()
                };
                allocated.push(handle.clone());
                let input = api::ImageInput {
                    handle: handle.clone(),
                    format: image.format,
                    byte_len: image.bytes.len() as u64,
                };
                let resource = Arc::new(ImageInputResource::new(
                    input.clone(),
                    document.clone(),
                    selection,
                    image.bytes,
                    budget.clone(),
                )?);
                state.image_inputs.insert(
                    handle.resource,
                    Input {
                        resource,
                        panel: panel.into(),
                        expires: Instant::now()
                            + Duration::from_millis(image_input::IMAGE_INPUT_TIMEOUT_MS.into()),
                        in_flight: None,
                    },
                );
            }
            Ok(allocated
                .iter()
                .map(|handle| state.image_inputs[&handle.resource].resource.input.clone())
                .collect())
        })();
        if result.is_err() {
            for handle in allocated {
                state.image_inputs.remove(&handle.resource);
                state.roots.remove(&handle);
            }
        }
        result
    }

    /// View withdrawal, source replacement and expiry revoke unused inputs without sealing accepted writer receipts.
    pub(crate) fn reconcile_image_inputs(&mut self) {
        let now = Instant::now();
        let stale: Vec<_> = self
            .store
            .data()
            .image_inputs
            .values()
            .filter(|input| {
                now >= input.expires
                    || self
                        .preview_sources
                        .get(&input.panel)
                        .and_then(Option::as_ref)
                        != Some(&input.resource.document)
                    || !self.views.get(&input.panel).is_some_and(|view| {
                        view.editor_image_input
                            && view.source.as_ref() == Some(&input.resource.document)
                    })
            })
            .map(|input| input.resource.input.handle.clone())
            .collect();
        self.release_image_inputs(stale);
    }

    /// Document changes revoke offers before subscriptions or replacement Preview publication can lag behind.
    pub(super) fn invalidate_image_inputs(&mut self, change: &api::DocumentChange) {
        let stale = self
            .store
            .data()
            .image_inputs
            .values()
            .filter(|input| {
                let source = &input.resource.document;
                source.id == change.document.id
                    && (change.closed
                        || (change.document.revision >= source.revision
                            && change.document != *source))
            })
            .map(|input| input.resource.input.handle.clone())
            .collect();
        self.release_image_inputs(stale);
    }

    /// Workspace parking must not keep a previously active editor's native input authority.
    pub(crate) fn clear_image_inputs(&mut self) {
        let handles = self
            .store
            .data()
            .image_inputs
            .values()
            .map(|input| input.resource.input.handle.clone())
            .collect();
        self.release_image_inputs(handles);
    }

    fn release_image_inputs(&mut self, handles: Vec<ResourceHandle>) {
        for handle in handles {
            self.store.data_mut().image_inputs.remove(&handle.resource);
            self.store.data_mut().roots.remove(&handle);
        }
    }
}
