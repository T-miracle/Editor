//! Typed file handles carry immutable instance ownership and are revoked together on retirement.
use super::*;
use api::{ErrorCode, Failure, ResourceHandle, Value};
use std::collections::BTreeMap;

/// Slots describe authority, not user-supplied paths; the caller cannot retarget a root.
#[derive(Clone, Copy)]
pub(super) enum RootKind {
    Workspace,
    Data,
    Selected,
    EditorRequest,
    ImageInput,
    /// Readonly text never grants file-root authority.
    VirtualDocument,
    ServiceReference,
    ServiceRequest,
    /// Provider-side deferred reply; it is not a consumer reference or a native process authority.
    ServiceInvocation,
    Subscription,
    PreferenceSubscription,
    Process(u64),
}

pub(super) struct ResourceRoots {
    instance: String,
    scope: String,
    next: u64,
    slots: BTreeMap<u64, RootKind>,
    pub(super) application: bool,
    pub(super) retired: bool,
    pub(super) limit: usize,
}

impl ResourceRoots {
    /// Expose immutable ownership metadata, never a caller-controlled target instance.
    pub(super) fn principal(
        &self,
        plugin: &str,
        permissions: &BTreeSet<String>,
    ) -> plugin_protocol::service::Caller {
        plugin_protocol::service::Caller {
            plugin: plugin.into(),
            instance: self.instance.clone(),
            scope: self.scope.clone(),
            permissions: permissions.clone(),
        }
    }
    /// Hook-local file handles cannot escape the discovery invocation or consume permanent quota.
    pub(super) fn checkpoint(&self) -> u64 {
        self.next
    }
    pub(super) fn release_since(&mut self, checkpoint: u64) {
        self.slots.retain(|id, _| *id < checkpoint);
    }
    /// Random owner identities prevent persisted handles from becoming valid after a host restart.
    pub(super) fn new(workspace: &str, application: bool, limit: usize) -> Self {
        let instance = uuid::Uuid::new_v4().to_string();
        Self {
            instance,
            scope: if application {
                "application".into()
            } else {
                crate::manager::scopes::workspace_key(workspace)
            },
            next: 1,
            slots: BTreeMap::new(),
            application,
            retired: false,
            limit,
        }
    }

    /// Clearing slots plus sealing the owner prevents old messages from reviving resources.
    pub(super) fn retire(&mut self) {
        self.retired = true;
        self.slots.clear();
    }
    pub(super) fn len(&self) -> usize {
        self.slots.len()
    }
    /// Rollback retains the declared quota while allocating a new owner identity.
    pub(super) fn renewed(&self, workspace: &str) -> Self {
        Self::new(workspace, self.application, self.limit)
    }

    pub(super) fn resolve(&self, handle: &ResourceHandle) -> Result<RootKind, Failure> {
        if self.retired || handle.instance != self.instance || handle.scope != self.scope {
            return Err(Failure::new(
                ErrorCode::InvalidHandle,
                "Resource owner is no longer available",
            ));
        }
        self.slots
            .get(&handle.resource)
            .copied()
            .ok_or_else(|| Failure::new(ErrorCode::InvalidHandle, "Unknown or released resource"))
    }

    /// Preflight is safe on the serial instance worker and must precede native side effects.
    pub(super) fn ensure_capacity(&self) -> Result<(), Failure> {
        if self.slots.len() >= 128 {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Resource handle quota exceeded",
            ));
        }
        self.next
            .checked_add(1)
            .ok_or_else(|| Failure::new(ErrorCode::LimitExceeded, "Resource IDs exhausted"))?;
        Ok(())
    }

    pub(super) fn open(&mut self, kind: RootKind) -> Result<Value, Failure> {
        self.ensure_capacity()?;
        let resource = self.next;
        self.next += 1;
        self.slots.insert(resource, kind);
        Ok(Value::Resource(ResourceHandle {
            instance: self.instance.clone(),
            scope: self.scope.clone(),
            resource,
        }))
    }

    /// Ownership is checked before callers remove the slot and its associated resource.
    pub(super) fn remove(&mut self, handle: &ResourceHandle) {
        self.slots.remove(&handle.resource);
    }
}

impl State {
    /// Required negotiation and installation consent are checked on open and every subsequent use.
    pub(super) fn file_authority(&self, kind: RootKind, write: bool) -> Result<&Path, Failure> {
        if (!self.active && !self.migrating && !(self.language_hook && !write))
            || self.roots.retired
        {
            return Err(Failure::new(
                ErrorCode::InvalidState,
                "Instance is not active",
            ));
        }
        let (capability, permission, root) = match kind {
            RootKind::EditorRequest
            | RootKind::VirtualDocument
            | RootKind::Selected
            | RootKind::ImageInput
            | RootKind::ServiceReference
            | RootKind::ServiceRequest
            | RootKind::ServiceInvocation
            | RootKind::Subscription
            | RootKind::PreferenceSubscription
            | RootKind::Process(_) => {
                return Err(Failure::new(ErrorCode::InvalidHandle, "Not a file handle"));
            }
            RootKind::Workspace => {
                if write
                    || self.roots.application
                    || self.workspace.as_os_str().is_empty()
                    || !self
                        .plugin_services
                        .alive
                        .load(std::sync::atomic::Ordering::Acquire)
                {
                    return Err(Failure::new(
                        ErrorCode::PermissionDenied,
                        "No workspace authority for this operation",
                    ));
                }
                (
                    "workspace.files",
                    "workspace.read",
                    self.workspace.as_path(),
                )
            }
            RootKind::Data => ("storage.private", "storage", self.data.as_path()),
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
        Ok(root)
    }

    /// Workspace paths are resolved only below the owner root, never a currently selected project.
    pub(super) fn resource_request(&mut self, operation: api::Operation) -> Result<Value, Failure> {
        match operation {
            api::Operation::OpenWorkspace => {
                self.file_authority(RootKind::Workspace, false)?;
                self.roots.open(RootKind::Workspace)
            }
            api::Operation::FindFiles { handle, query } => {
                let kind = self.roots.resolve(&handle)?;
                if !matches!(kind, RootKind::Workspace) {
                    return Err(Failure::new(
                        ErrorCode::InvalidHandle,
                        "File discovery requires a workspace handle",
                    ));
                }
                let root = self.file_authority(kind, false)?;
                if !self
                    .api
                    .capabilities
                    .get("workspace.files")
                    .is_some_and(|version| *version >= semver::Version::new(1, 1, 0))
                {
                    return Err(Failure::new(
                        ErrorCode::CapabilityUnavailable,
                        "File discovery requires workspace.files 1.1",
                    ));
                }
                super::file_discovery::find(root, &query, self.call_deadline).map(Value::Files)
            }
            api::Operation::OpenData => {
                self.file_authority(RootKind::Data, false)?;
                self.roots.open(RootKind::Data)
            }
            api::Operation::ReadFile { handle, path } => {
                let kind = self.roots.resolve(&handle)?;
                if matches!(kind, RootKind::Selected) {
                    return self.read_selection(&handle, &path).map(Value::Bytes);
                }
                let root = self.file_authority(kind, false)?;
                data_files::read(root, &path, &self.staged_writes).map(Value::Bytes)
            }
            api::Operation::WriteFile {
                handle,
                path,
                bytes,
            } => {
                let kind = self.roots.resolve(&handle)?;
                if matches!(kind, RootKind::Selected) {
                    self.check_selection_authority()?;
                    return Err(Failure::new(
                        ErrorCode::UnsupportedOperation,
                        "Selected writes require the host document transaction capability",
                    ));
                }
                let root = self.file_authority(kind, true)?.to_path_buf();
                data_files::write(
                    &root,
                    &path,
                    bytes,
                    self.roots.limit,
                    &mut self.staged_writes,
                )?;
                Ok(Value::Unit)
            }
            api::Operation::CloseResource { handle } => {
                self.roots.resolve(&handle)?;
                if matches!(self.roots.resolve(&handle)?, RootKind::Selected) {
                    self.check_selection_authority()?;
                    self.selected_resources.remove(&handle.resource);
                }
                // Process termination still needs the original context for its native receipt.
                if let RootKind::Process(_) = self.roots.resolve(&handle)? {
                    return self
                        .process_request(plugin_protocol::process::Operation::Terminate { handle })
                        .map(|_| Value::Unit);
                }
                self.plugin_services.resources.remove(&handle.resource);
                self.subscriptions.remove(&handle.resource);
                self.document_streams.remove(&handle.resource);
                if let Some(resource) = self.virtual_documents.remove(&handle.resource) {
                    resource.revoke();
                }
                self.preference_subscriptions.remove(&handle.resource);
                self.image_inputs.remove(&handle.resource);
                self.plugin_services.references.remove(&handle.resource);
                if let Some(incoming) = self.plugin_services.incoming.remove(&handle.resource) {
                    // Explicitly dropping a reply rejects its wait; created native effects stay owned.
                    incoming.call.completion.finish(Err(Failure::new(
                        ErrorCode::Cancelled,
                        "Provider released its reply handle",
                    )));
                }
                if let Some(request) = self.plugin_services.pending.remove(&handle.resource) {
                    request.call.completion.retire();
                }
                if let Some(request) = self.editor_requests.remove(&handle.resource) {
                    request.call.retire();
                    self.finish_image_input_request(&request.call, &request.call.status());
                }
                self.roots.slots.remove(&handle.resource);
                Ok(Value::Unit)
            }
            api::Operation::ReadAsset { .. }
            | api::Operation::Commands { .. }
            | api::Operation::DescribeSdk
            | api::Operation::Service { .. }
            | api::Operation::Process { .. }
            | api::Operation::SubscribeDocuments
            | api::Operation::SubscribeDocumentEvents
            | api::Operation::ReadPreference { .. }
            | api::Operation::WritePreference { .. }
            | api::Operation::Editor { .. }
            | api::Operation::CancelRequest { .. } => {
                unreachable!("assets are handled before resource dispatch")
            }
        }
    }
}
