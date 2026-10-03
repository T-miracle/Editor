//! Validate typed editor authority before issuing scoped request handles.
use super::*;
use crate::editor_requests::{EditorRequest, PendingRequest};
use api::{EditorOperation, ErrorCode, Failure, Notification, Value};
use resource_roots::RootKind;

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
        let (capability, permission) = match &operation {
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
