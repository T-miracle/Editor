//! Capability transport handling, isolated from the legacy request/scene wire format.
use super::*;
use api::{ErrorCode, Failure};

impl State {
    /// Domain failures remain correlated responses, including malformed and unknown operations.
    pub(super) fn capability_request(&mut self, payload: &str) -> Result<String, String> {
        let mut id = 0;
        let result = (|| {
            if self.roots.retired {
                return Err(Failure::new(
                    ErrorCode::InvalidState,
                    "Instance has been retired",
                ));
            }
            if payload.len() > api::MAX_REQUEST_BYTES {
                return Err(Failure::new(
                    ErrorCode::LimitExceeded,
                    "Host request too large",
                ));
            }
            let value: serde_json::Value = serde_json::from_str(payload)
                .map_err(|error| Failure::new(ErrorCode::InvalidRequest, error.to_string()))?;
            id = value
                .get("id")
                .and_then(serde_json::Value::as_u64)
                .filter(|id| *id != 0)
                .ok_or_else(|| {
                    Failure::new(ErrorCode::InvalidRequest, "Request ID must be nonzero")
                })?;
            let method = value
                .get("operation")
                .and_then(|op| op.get("method"))
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    Failure::new(ErrorCode::InvalidRequest, "Missing operation method")
                })?;
            if ![
                "read_asset",
                "open_workspace",
                "open_data",
                "read_file",
                "write_file",
                "close_resource",
                "editor",
                "cancel_request",
                "subscribe_documents",
                "process",
                "service",
            ]
            .contains(&method)
            {
                return Err(Failure::new(
                    ErrorCode::UnsupportedOperation,
                    format!("Unknown operation: {method}"),
                ));
            }
            let request: api::Request = serde_json::from_value(value)
                .map_err(|error| Failure::new(ErrorCode::InvalidRequest, error.to_string()))?;
            // A migration may not borrow workspace, editor, service or process authority from its normal grants.
            if self.migrating
                && !matches!(
                    &request.operation,
                    api::Operation::ReadAsset { .. }
                        | api::Operation::OpenData
                        | api::Operation::ReadFile { .. }
                        | api::Operation::WriteFile { .. }
                        | api::Operation::CloseResource { .. }
                )
            {
                return Err(Failure::new(
                    ErrorCode::PermissionDenied,
                    "Migration has private-copy authority only",
                ));
            }
            // Discovery may read granted roots but cannot create side effects or subscriptions.
            if self.language_hook
                && !matches!(
                    &request.operation,
                    api::Operation::ReadAsset { .. }
                        | api::Operation::OpenWorkspace { .. }
                        | api::Operation::OpenData { .. }
                        | api::Operation::ReadFile { .. }
                )
            {
                return Err(Failure::new(
                    ErrorCode::InvalidState,
                    "LSP discovery is read-only",
                ));
            }
            self.check_service_authority(&request.operation)?;
            let result = match request.operation {
                api::Operation::Service { operation } => self.service_request(operation),
                api::Operation::Process { operation } => self.process_request(operation),
                api::Operation::SubscribeDocuments => self.subscribe_documents(),
                api::Operation::CancelRequest { handle, mode } => {
                    self.roots.resolve(&handle)?;
                    if let Some(request) = self.plugin_services.pending.get(&handle.resource) {
                        return request
                            .call
                            .completion
                            .cancel(mode, ErrorCode::Cancelled)
                            .map(api::Value::Cancellation);
                    }
                    self.editor_requests
                        .get(&handle.resource)
                        .ok_or_else(|| {
                            Failure::new(ErrorCode::InvalidHandle, "Not an outstanding request")
                        })?
                        .call
                        .cancel(mode, ErrorCode::Cancelled)
                        .map(api::Value::Cancellation)
                }
                api::Operation::ReadAsset { path } => self.read_capability_asset(&path),
                api::Operation::Editor {
                    operation,
                    timeout_ms,
                } => self.editor_request(operation, timeout_ms),
                operation => self.resource_request(operation),
            };
            // Record all delegated allocations at the common boundary, including file and request handles.
            if let (
                Ok(api::Value::Resource(handle) | api::Value::Accepted(handle)),
                Some(context),
            ) = (&result, &self.plugin_services.context)
            {
                self.plugin_services
                    .resources
                    .insert(handle.resource, (handle.clone(), context.clone()));
            }
            result
        })();
        serde_json::to_string(&api::Response { id, result }).map_err(|error| error.to_string())
    }

    /// Availability and authorization are separate; preparation can read only immutable assets.
    fn read_capability_asset(&self, path: &str) -> Result<api::Value, Failure> {
        if !self
            .api
            .as_ref()
            .is_some_and(|api| api.capabilities.contains_key("package.assets"))
        {
            return Err(Failure::new(
                ErrorCode::CapabilityUnavailable,
                "package.assets was not negotiated",
            ));
        }
        if !self.permissions.contains("assets.read") {
            return Err(Failure::new(
                ErrorCode::PermissionDenied,
                "assets.read permission required",
            ));
        }
        let path = safe_path(&self.assets, path, true).map_err(|error| {
            // Missing assets are different from rejected traversal or an escaped symlink.
            let missing = error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound);
            Failure::new(
                if missing {
                    ErrorCode::NotFound
                } else {
                    ErrorCode::InvalidPath
                },
                error.to_string(),
            )
        })?;
        let file = std::fs::File::open(path).map_err(|error| {
            Failure::new(
                if error.kind() == std::io::ErrorKind::NotFound {
                    ErrorCode::NotFound
                } else {
                    ErrorCode::OperationFailed
                },
                error.to_string(),
            )
        })?;
        let mut bytes = Vec::new();
        use std::io::Read;
        file.take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| Failure::new(ErrorCode::OperationFailed, error.to_string()))?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Asset read quota exceeded",
            ));
        }
        Ok(api::Value::Asset { bytes })
    }
}

impl Instance {
    /// Only the migration adapter sees legacy lifecycle types; new guests receive typed envelopes.
    pub(super) fn encode_invocation(
        &mut self,
        message: Message,
    ) -> anyhow::Result<Option<(String, Option<u64>)>> {
        let Some(api) = &self.store.data().api else {
            return Ok(Some((serde_json::to_string(&message)?, None)));
        };
        let message = match message {
            Message::Prepare {
                environment,
                snapshot,
            } => api::Input::Prepare {
                environment,
                snapshot,
                api: api.clone(),
            },
            Message::Activate => api::Input::Activate,
            Message::Snapshot => api::Input::Snapshot,
            Message::Event(event) => match native_notification(event, None) {
                Some(message) => message,
                None => return Ok(None),
            },
        };
        let id = self.next_call;
        self.next_call = id
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Invocation IDs exhausted"))?;
        Ok(Some((
            serde_json::to_string(&api::Invocation { id, message })?,
            Some(id),
        )))
    }

    /// Convert a validated new UI result to the existing native renderer without exposing canvas fields.
    pub(super) fn decode_completion(
        &self,
        payload: &str,
        id: Option<u64>,
    ) -> anyhow::Result<Reply> {
        let Some(id) = id else {
            return Ok(serde_json::from_str(payload)?);
        };
        let completion: api::Completion = serde_json::from_str(payload)?;
        anyhow::ensure!(completion.id == id, "Plugin completion ID mismatch");
        let output = completion.result?;
        anyhow::ensure!(
            !self.store.data().migrating
                || (output.views.is_empty()
                    && output.configuration.is_none()
                    && output.language_service.is_none()
                    && output.service_reply.is_none()),
            "Migration may return only an opaque snapshot"
        );
        anyhow::ensure!(
            output.service_reply.is_none() || self.store.data().plugin_services.invoking,
            "Service results require an owning invocation"
        );
        // Reject forbidden hook output before the common call path publishes any view or snapshot.
        anyhow::ensure!(
            !self.store.data().language_hook
                || (output.views.is_empty()
                    && output.snapshot.is_none()
                    && output.configuration.is_none()),
            "LSP hook can return only a language proposal"
        );
        let api = self
            .store
            .data()
            .api
            .as_ref()
            .expect("capability invocation");
        anyhow::ensure!(
            output.views.is_empty() || api.capabilities.contains_key("ui.native"),
            "ui.native was not negotiated"
        );
        anyhow::ensure!(output.views.len() <= 8, "Too many native views");
        for view in &output.views {
            // Validate structure before walking or allocating native controls, preserving a typed failure.
            view.document
                .validate()
                .map_err(|message| Failure::new(ErrorCode::InvalidRequest, message))?;
            if view.document.source.as_ref()
                != self
                    .preview_sources
                    .get(&view.panel)
                    .and_then(Option::as_ref)
            {
                return Err(Failure::new(
                    ErrorCode::StaleRevision,
                    "Preview source version does not match its current input",
                )
                .into());
            }
            let mut canvas = false;
            let mut grid = false;
            let mut visit = |node: &ui::Node| {
                if let ui::Kind::Canvas(value) = &node.kind {
                    canvas = true;
                    grid |= value.grid;
                }
            };
            view.document.root.visit(&mut visit);
            if let Some(dialog) = &view.document.dialog {
                dialog.content.visit(&mut visit);
            }
            for (required, capability) in [(canvas, "ui.canvas"), (grid, "ui.grid")] {
                if required && !api.capabilities.contains_key(capability) {
                    return Err(api::Failure::new(
                        api::ErrorCode::CapabilityUnavailable,
                        format!("{capability} was not negotiated"),
                    )
                    .into());
                }
            }
        }
        Ok(Reply {
            service_reply: output.service_reply,
            language_service: output.language_service,
            configuration: output.configuration,
            snapshot: output.snapshot,
            scenes: output
                .views
                .into_iter()
                .map(|view| Scene {
                    panel: view.panel,
                    ui: Some(view.document),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        })
    }
}

/// Translate the migration-era editor routing seam into native-only public notifications.
fn native_notification(event: Event, panel: Option<String>) -> Option<api::Input> {
    let event = match event {
        Event::Capability(notification) => notification,
        Event::Surface { panel, event } => return native_notification(*event, Some(panel)),
        Event::Ui(event) => api::Notification::Ui(event),
        Event::Theme(environment) => api::Notification::Theme(environment),
        Event::Command { id, arguments, .. } => api::Notification::Command { id, arguments },
        Event::Focus(focused) => api::Notification::Focus(focused),
        Event::Resize { width, height, .. } => api::Notification::Resize { width, height },
        _ => return None,
    };
    Some(api::Input::Event { panel, event })
}
