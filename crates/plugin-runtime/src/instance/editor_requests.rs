//! Validate typed editor authority before issuing scoped request handles.
use super::*;
use crate::editor_requests::{EditorRequest, PendingRequest};
use api::{EditorOperation, ErrorCode, Failure, Notification, Value};
use resource_roots::RootKind;

/// Return the target's required grant for direct and delegated navigation alike.
/// Callers also require editor.read; sharing this mapping prevents stronger provider grants leaking.
pub(super) fn navigation_permission(target: &api::NavigationTarget) -> &'static str {
    match target {
        api::NavigationTarget::PreviewNode { .. } => "editor.read",
        api::NavigationTarget::RelativeDocument { .. } => "workspace.read",
        api::NavigationTarget::ExternalUrl { .. } => "navigation.external",
    }
}

impl State {
    /// Editor access never follows an application's currently selected workspace implicitly.
    pub(super) fn editor_request(
        &mut self,
        operation: EditorOperation,
        timeout_ms: u32,
    ) -> Result<Value, Failure> {
        if !self.active || self.roots.retired || self.roots.application {
            return Err(Failure::new(
                ErrorCode::PermissionDenied,
                "No active workspace editor authority",
            ));
        }
        if timeout_ms == 0 || timeout_ms > 300_000 {
            return Err(Failure::new(
                ErrorCode::InvalidRequest,
                "Deadline must be between 1 and 300000 ms",
            ));
        }
        // New local-resource and position operations are additive editor.documents 1.1 contracts.
        if matches!(
            &operation,
            EditorOperation::OpenDocument {
                resource: api::ResourceIdentity::Local { .. }
            } | EditorOperation::LocateDocument { .. }
                | EditorOperation::ListDocuments
                | EditorOperation::ReadDocument { .. }
        ) && self
            .api
            .capabilities
            .get("editor.documents")
            .is_none_or(|version| *version < semver::Version::new(1, 1, 0))
        {
            return Err(Failure::new(
                ErrorCode::CapabilityUnavailable,
                "Document resource operations require editor.documents 1.1",
            ));
        }
        let (capability, permission) = match &operation {
            EditorOperation::OpenVirtualDocument { .. }
            | EditorOperation::RefreshVirtualDocument { .. } => ("editor.virtual", "editor.read"),
            EditorOperation::CompareDocuments { .. } => ("editor.diff", "editor.read"),
            EditorOperation::OpenDocument {
                resource: api::ResourceIdentity::Virtual { .. },
            } => ("editor.virtual", "editor.read"),
            EditorOperation::OpenDocument {
                resource: api::ResourceIdentity::Local { path },
            } => {
                api::ResourceIdentity::Local { path: path.clone() }.validate()?;
                if !self.permissions.contains("workspace.read") {
                    return Err(Failure::new(
                        ErrorCode::PermissionDenied,
                        "Opening local documents requires workspace.read",
                    ));
                }
                ("editor.documents", "editor.read")
            }
            EditorOperation::LocateDocument { .. } => ("editor.documents", "editor.read"),
            EditorOperation::ListDocuments | EditorOperation::ReadDocument { .. } => {
                ("editor.documents", "editor.read")
            }
            EditorOperation::LocateViewport {
                panel,
                target,
                origin,
                ..
            } => {
                target.validate()?;
                if *origin == 0 {
                    return Err(Failure::new(
                        ErrorCode::InvalidRequest,
                        "Viewport location requires a nonzero origin",
                    ));
                }
                self.check_editor_viewport_authority(panel)?;
                ("editor.viewport", "editor.read")
            }
            EditorOperation::NavigateDocument { target, .. } => {
                target.validate()?;
                if !self.permissions.contains("editor.read") {
                    return Err(Failure::new(
                        ErrorCode::PermissionDenied,
                        "Navigation requires editor.read",
                    ));
                }
                if let api::NavigationTarget::PreviewNode { panel, .. } = target {
                    if !self.declared_editor_panels.contains(panel) {
                        return Err(Failure::new(
                            ErrorCode::PermissionDenied,
                            "Navigation panel is not owned by this instance",
                        ));
                    }
                }
                ("editor.navigation", navigation_permission(target))
            }
            EditorOperation::SaveImageInput { .. } => ("editor.images", "workspace.write"),
            EditorOperation::ReadDocumentSelection { .. } => ("editor.edit", "editor.read"),
            EditorOperation::ReplaceDocumentRange { .. } => ("editor.edit", "editor.write"),
            EditorOperation::WriteClipboard { text } if text.len() > 1024 * 1024 => {
                return Err(Failure::new(
                    ErrorCode::LimitExceeded,
                    "Clipboard exceeds 1 MiB",
                ));
            }
            EditorOperation::ReadClipboard | EditorOperation::WriteClipboard { .. } => {
                ("ui.clipboard", "clipboard")
            }
            EditorOperation::OpenDataFile { path } => {
                if path.is_empty()
                    || path.len() > 4096
                    || path.contains(['\\', ':', '\0'])
                    || path
                        .split('/')
                        .any(|part| part.is_empty() || matches!(part, "." | ".."))
                {
                    return Err(Failure::new(
                        ErrorCode::InvalidPath,
                        "Private file path must be relative",
                    ));
                }
                ("storage.editor", "storage")
            }
            EditorOperation::SetPanelVisibility { panel, .. } => {
                if !self.declared_panels.contains(panel) {
                    return Err(Failure::new(
                        ErrorCode::PermissionDenied,
                        "Panel is not owned by this plugin",
                    ));
                }
                ("ui.panels", "ui.panels")
            }
            EditorOperation::SaveDocument { .. } => ("editor.documents", "editor.write"),
            _ => ("editor.documents", "editor.read"),
        };
        if !self.api.capabilities.contains_key(capability) {
            return Err(Failure::new(
                ErrorCode::CapabilityUnavailable,
                format!("{capability} was not negotiated"),
            ));
        }
        if !self.permissions.contains(permission) {
            return Err(Failure::new(
                ErrorCode::PermissionDenied,
                format!("{permission} permission required"),
            ));
        }
        validate_edit_request(&operation)?;
        let image_input = if let EditorOperation::SaveImageInput { input, name } = &operation {
            if timeout_ms > crate::IMAGE_INPUT_TIMEOUT_MS {
                return Err(Failure::new(
                    ErrorCode::InvalidRequest,
                    "Image save deadline must not exceed 30000 ms",
                ));
            }
            Some(self.image_input_payload(input, name)?)
        } else {
            None
        };
        if self.editor_requests.len() >= 32 {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Editor request queue is full",
            ));
        }
        let Value::Resource(handle) = self.roots.open(RootKind::EditorRequest)? else {
            unreachable!()
        };
        let virtual_document = match self.prepare_virtual_document(&operation) {
            Ok(resource) => resource,
            Err(error) => {
                self.roots.remove(&handle);
                return Err(error);
            }
        };
        let mut call = EditorRequest::new(
            handle.clone(),
            operation,
            self.workspace.display().to_string(),
            self.data.clone(),
            timeout_ms,
            self.plugin_services.context.as_ref(),
        );
        if let Some(input) = image_input {
            self.mark_image_input_pending(&input.input.handle, handle.clone());
            call = call.with_image_input(input);
        }
        if let Some(resource) = virtual_document {
            call = call.with_virtual_document(resource);
        }
        self.editor_requests.insert(
            handle.resource,
            PendingRequest {
                context: self.plugin_services.context.clone(),
                call,
                sent: false,
                reported: 0,
            },
        );
        Ok(Value::Accepted(handle))
    }

    /// Virtual access is direct, owned and bounded; service providers cannot borrow their caller's document.
    fn prepare_virtual_document(
        &mut self,
        operation: &EditorOperation,
    ) -> Result<Option<std::sync::Arc<crate::VirtualDocumentResource>>, Failure> {
        let check = |document: &api::DocumentVersion| -> Result<Option<std::sync::Arc<crate::VirtualDocumentResource>>, Failure> {
            if !document.path.starts_with("nanobug-virtual://") { return Ok(None); }
            if self.plugin_services.context.is_some() {
                return Err(Failure::new(ErrorCode::PermissionDenied, "Virtual documents are instance-owned"));
            }
            let resource = self.virtual_documents.values().find(|resource| resource.document().is_some_and(|bound| bound.id == document.id))
                .ok_or_else(|| Failure::new(ErrorCode::PermissionDenied, "Virtual document is not owned by this instance"))?;
            self.roots.resolve(resource.handle())?;
            if !resource.is_live() { return Err(Failure::new(ErrorCode::InvalidHandle, "Virtual document was revoked")); }
            if resource.document().as_ref() != Some(document) { return Err(Failure::new(ErrorCode::StaleRevision, "Virtual document revision changed")); }
            Ok(Some(resource.clone()))
        };
        match operation {
            EditorOperation::OpenVirtualDocument {
                title,
                language,
                text,
            } => {
                if self.plugin_services.context.is_some() {
                    return Err(Failure::new(
                        ErrorCode::PermissionDenied,
                        "Virtual providers require direct ownership",
                    ));
                }
                if title.is_empty()
                    || title.len() > 256
                    || title.chars().any(char::is_control)
                    || language.as_ref().is_some_and(|value| {
                        value.len() > 128 || value.chars().any(char::is_control)
                    })
                {
                    return Err(Failure::new(
                        ErrorCode::InvalidRequest,
                        "Invalid virtual document title or language",
                    ));
                }
                if text.len() > 1024 * 1024 {
                    return Err(Failure::new(
                        ErrorCode::LimitExceeded,
                        "Virtual content exceeds 1 MiB",
                    ));
                }
                if self.virtual_documents.len() >= 32 {
                    return Err(Failure::new(
                        ErrorCode::LimitExceeded,
                        "Virtual document quota exceeded",
                    ));
                }
                let Value::Resource(handle) = self.roots.open(RootKind::VirtualDocument)? else {
                    unreachable!()
                };
                let resource = std::sync::Arc::new(crate::VirtualDocumentResource::new(
                    handle.clone(),
                    self.plugin_services.alive.clone(),
                ));
                self.virtual_documents
                    .insert(handle.resource, resource.clone());
                Ok(Some(resource))
            }
            EditorOperation::OpenDocument {
                resource: api::ResourceIdentity::Virtual { handle },
            } => {
                if self.plugin_services.context.is_some() {
                    return Err(Failure::new(
                        ErrorCode::PermissionDenied,
                        "Virtual providers require direct ownership",
                    ));
                }
                self.roots.resolve(handle)?;
                let resource = self
                    .virtual_documents
                    .get(&handle.resource)
                    .filter(|resource| resource.is_live())
                    .ok_or_else(|| {
                        Failure::new(ErrorCode::InvalidHandle, "Virtual document was revoked")
                    })?;
                Ok(Some(resource.clone()))
            }
            EditorOperation::RefreshVirtualDocument { document, text } => {
                if text.len() > 1024 * 1024 {
                    return Err(Failure::new(
                        ErrorCode::LimitExceeded,
                        "Virtual content exceeds 1 MiB",
                    ));
                }
                check(document)?.map(Some).ok_or_else(|| {
                    Failure::new(
                        ErrorCode::UnsupportedOperation,
                        "Only virtual documents can be refreshed",
                    )
                })
            }
            EditorOperation::ReadDocument { document, .. }
            | EditorOperation::LocateDocument { document, .. }
            | EditorOperation::SaveDocument { document }
            | EditorOperation::ReadDocumentSelection { document }
            | EditorOperation::ReplaceDocumentRange { document, .. } => check(document),
            EditorOperation::CompareDocuments { left, right } => {
                check(left)?;
                check(right)?;
                Ok(None)
            }
            _ => Ok(None),
        }
    }
}

/// Check bounded request shape; only the host can check the target revision and UTF-8 boundaries.
fn validate_edit_request(operation: &EditorOperation) -> Result<(), Failure> {
    if let EditorOperation::ReplaceDocumentRange {
        range,
        text,
        selection,
        expected_selection,
        ..
    } = operation
    {
        if text.len() > 1024 * 1024 {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Replacement text exceeds 1 MiB",
            ));
        }
        // Result selections address the resulting full document, so there is no replacement-size end bound.
        if [Some(range), Some(selection), expected_selection.as_ref()]
            .into_iter()
            .flatten()
            .any(|range| range.start > range.end)
        {
            return Err(Failure::new(
                ErrorCode::InvalidRequest,
                "Text range start must not exceed its end",
            ));
        }
    }
    Ok(())
}

impl Instance {
    /// The worker transfers only typed owned calls; the GPUI thread never touches a WASM store.
    pub fn take_editor_requests(&mut self) -> Vec<EditorRequest> {
        self.store
            .data_mut()
            .editor_requests
            .values_mut()
            .filter_map(|request| {
                if request.sent {
                    return None;
                }
                request.sent = true;
                Some(request.call.clone())
            })
            .collect()
    }
    /// Terminal results release slots even if the guest fails while consuming its notification.
    pub(super) fn poll_editor_requests(&mut self) -> anyhow::Result<()> {
        // A native tab close revokes its shared authority; release the persistent root before new work.
        let revoked = self
            .store
            .data()
            .virtual_documents
            .iter()
            .filter(|(_, resource)| !resource.is_live())
            .map(|(slot, resource)| (*slot, resource.handle().clone()))
            .collect::<Vec<_>>();
        for (slot, handle) in revoked {
            self.store.data_mut().virtual_documents.remove(&slot);
            self.store.data_mut().roots.remove(&handle);
        }
        let updates = self
            .store
            .data_mut()
            .editor_requests
            .iter_mut()
            .filter_map(|(slot, pending)| {
                let (version, update) = pending.call.update();
                if version == pending.reported {
                    return None;
                }
                pending.reported = version;
                Some((
                    *slot,
                    pending.call.handle().clone(),
                    update,
                    pending.context.clone(),
                ))
            })
            .collect::<Vec<_>>();
        for (slot, handle, update, context) in updates {
            // A preceding guest callback may explicitly release another request in this batch.
            if !self.store.data().editor_requests.contains_key(&slot) {
                continue;
            }
            if update.is_terminal() {
                if let Some(pending) = self.store.data_mut().editor_requests.remove(&slot) {
                    if matches!(
                        pending.call.operation(),
                        EditorOperation::OpenVirtualDocument { .. }
                    ) && !matches!(&update, api::RequestUpdate::Completed { result: Ok(_) })
                    {
                        if let Some(resource) = pending.call.virtual_document() {
                            resource.revoke();
                        }
                    }
                    self.store
                        .data_mut()
                        .finish_image_input_request(&pending.call, &update);
                }
                self.store.data_mut().roots.remove(&handle);
                self.store
                    .data_mut()
                    .plugin_services
                    .resources
                    .remove(&handle.resource);
            }
            self.call_with_service_context(
                context,
                api::Input::Event {
                    panel: None,
                    event: Notification::Request { handle, update },
                },
            )?;
        }
        Ok(())
    }
}

impl Drop for Instance {
    /// Manager removal also revokes calls retained by a queued UI callback.
    fn drop(&mut self) {
        self.stop();
    }
}
