//! Correlated capability requests and atomic validation of typed guest completions.
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
                "find_files",
                "describe_sdk",
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
            // Cutover snapshots may serialize private files, but cannot enqueue new native side effects.
            if !self
                .plugin_services
                .alive
                .load(std::sync::atomic::Ordering::Acquire)
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
                    ErrorCode::InvalidState,
                    "Instance is switching versions",
                ));
            }
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
            // Cleanup is confined to roots allocated by this hook, never an older process or request.
            let hook_cleanup = if let api::Operation::CloseResource { handle } = &request.operation
            {
                self.language_hook_checkpoint
                    .is_some_and(|start| handle.resource >= start)
                    && matches!(
                        self.roots.resolve(handle),
                        Ok(resource_roots::RootKind::Workspace | resource_roots::RootKind::Data)
                    )
            } else {
                false
            };
            // Discovery may read granted roots and immutable host metadata, without native side effects.
            if self.language_hook
                && !hook_cleanup
                && !matches!(
                    &request.operation,
                    api::Operation::ReadAsset { .. }
                        | api::Operation::OpenWorkspace { .. }
                        | api::Operation::OpenData { .. }
                        | api::Operation::ReadFile { .. }
                        | api::Operation::FindFiles { .. }
                        | api::Operation::DescribeSdk
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
                api::Operation::DescribeSdk => self.describe_sdk(),
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

    /// Native toolchain metadata grants neither workspace access nor a filesystem root to WASI.
    fn describe_sdk(&self) -> Result<api::Value, Failure> {
        if !self.api.capabilities.contains_key("host.sdk") {
            return Err(Failure::new(
                ErrorCode::CapabilityUnavailable,
                "host.sdk was not negotiated",
            ));
        }
        if !self.active && !self.language_hook {
            return Err(Failure::new(
                ErrorCode::InvalidState,
                "SDK discovery requires an active instance or language hook",
            ));
        }
        let sdk = self
            .host_resources
            .sdk
            .as_ref()
            .ok_or_else(|| {
                Failure::new(
                    ErrorCode::NotFound,
                    "This host did not provide a plugin SDK",
                )
            })?
            .as_ref()
            .map_err(|error| {
                Failure::new(
                    ErrorCode::OperationFailed,
                    error.chars().take(4096).collect::<String>(),
                )
            })?;
        if !Path::new(&sdk.root).is_absolute()
            || !Path::new(&sdk.cargo_config).is_absolute()
            || !serde_json::to_vec(sdk).is_ok_and(|bytes| bytes.len() <= 64 * 1024)
        {
            return Err(Failure::new(
                ErrorCode::OperationFailed,
                "Invalid host SDK descriptor",
            ));
        }
        Ok(api::Value::Sdk(sdk.clone()))
    }

    /// Toolbar publication requires an independently negotiated capability and an owned workspace editor surface.
    fn check_editor_toolbar_authority(&self, panel: &str) -> Result<(), Failure> {
        if !self.api.capabilities.contains_key("editor.toolbar") {
            return Err(Failure::new(
                ErrorCode::CapabilityUnavailable,
                "editor.toolbar was not negotiated",
            ));
        }
        if self.roots.application
            || !self.permissions.contains("editor.read")
            || !self.declared_editor_panels.contains(panel)
        {
            return Err(Failure::new(
                ErrorCode::PermissionDenied,
                "Editor toolbar requires an owned workspace editor panel and editor.read",
            ));
        }
        Ok(())
    }

    /// Availability and authorization are separate; preparation can read only immutable assets.
    fn read_capability_asset(&self, path: &str) -> Result<api::Value, Failure> {
        if !self.api.capabilities.contains_key("package.assets") {
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
    /// Invocation IDs belong to one instance and cannot silently match an older response.
    pub(super) fn encode_invocation(
        &mut self,
        message: api::Input,
    ) -> anyhow::Result<(String, u64)> {
        let id = self.next_call;
        self.next_call = id
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Invocation IDs exhausted"))?;
        Ok((serde_json::to_string(&api::Invocation { id, message })?, id))
    }

    /// Validate every capability and source revision before publishing any document.
    pub(super) fn decode_completion(&self, payload: &str, id: u64) -> anyhow::Result<api::Output> {
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
        let api = &self.store.data().api;
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
            if view.document.editor_toolbar.is_some() {
                self.store
                    .data()
                    .check_editor_toolbar_authority(&view.panel)?;
            }
            let mut canvas = false;
            let mut grid = false;
            let mut collections = view.document.menu.is_some();
            let mut enhanced_canvas = false;
            let mut rich_text = false;
            let mut visit = |node: &ui::Node| {
                if let ui::Kind::Canvas(value) = &node.kind {
                    canvas = true;
                    grid |= value.grid;
                    enhanced_canvas |= value.scroll.is_some() || value.font != Default::default();
                }
                collections |= matches!(node.kind, ui::Kind::SideTabs(_));
                // Source metadata is part of the same optional interface even on ordinary nodes.
                rich_text |= node.source_range.is_some()
                    || matches!(
                        node.kind,
                        ui::Kind::RichText { .. } | ui::Kind::CodeBlock { .. }
                    );
            };
            view.document.root.visit(&mut visit);
            if let Some(toolbar) = &view.document.editor_toolbar {
                toolbar.visit(&mut visit);
            }
            if let Some(dialog) = &view.document.dialog {
                dialog.content.visit(&mut visit);
            }
            if enhanced_canvas
                && !api
                    .capabilities
                    .get("ui.canvas")
                    .is_some_and(|version| *version >= semver::Version::new(1, 1, 0))
            {
                return Err(Failure::new(
                    ErrorCode::CapabilityUnavailable,
                    "ui.canvas 1.1 is required for font and scroll range",
                )
                .into());
            }
            for (required, capability) in [
                (canvas, "ui.canvas"),
                (grid, "ui.grid"),
                (collections, "ui.collections"),
                (rich_text, "ui.richtext"),
            ] {
                if required && !api.capabilities.contains_key(capability) {
                    return Err(api::Failure::new(
                        api::ErrorCode::CapabilityUnavailable,
                        format!("{capability} was not negotiated"),
                    )
                    .into());
                }
            }
        }
        Ok(output)
    }
}
