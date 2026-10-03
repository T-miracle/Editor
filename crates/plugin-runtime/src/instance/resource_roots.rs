//! Typed file handles carry immutable instance ownership and are revoked together on retirement.
use super::*;
use api::{ErrorCode, Failure, ResourceHandle, Value};
use std::collections::BTreeMap;

/// Slots describe authority, not user-supplied paths; the caller cannot retarget a root.
#[derive(Clone, Copy)]
pub(super) enum RootKind {
    Workspace,
    Data,
    EditorRequest,
    ImageInput,
    ServiceReference,
    ServiceRequest,
    Subscription,
    Process(u64),
}

pub(super) struct ResourceRoots {
    instance: String,
    scope: String,
    next: u64,
    slots: BTreeMap<u64, RootKind>,
    pub(super) application: bool,
    pub(super) retired: bool,
    limit: usize,
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
    fn file_authority(&self, kind: RootKind, write: bool) -> Result<&Path, Failure> {
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
            | RootKind::ImageInput
            | RootKind::ServiceReference
            | RootKind::ServiceRequest
            | RootKind::Subscription
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
                let root = self.file_authority(kind, false)?;
                let path = safe_path(root, &path, false).map_err(path_failure)?;
                if let Some(bytes) = self
                    .staged_writes
                    .as_ref()
                    .and_then(|writes| writes.get(&path))
                {
                    return Ok(Value::Bytes(bytes.clone()));
                }
                use std::io::Read;
                let file = std::fs::File::open(path).map_err(io_failure)?;
                let mut bytes = Vec::new();
                file.take(1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(io_failure)?;
                if bytes.len() > 1024 * 1024 {
                    return Err(Failure::new(
                        ErrorCode::LimitExceeded,
                        "File read quota exceeded",
                    ));
                }
                Ok(Value::Bytes(bytes))
            }
            api::Operation::WriteFile {
                handle,
                path,
                bytes,
            } => {
                let kind = self.roots.resolve(&handle)?;
                let root = self
                    .file_authority(kind, true)?
                    .canonicalize()
                    .map_err(io_failure)?;
                let path = safe_path(&root, &path, false).map_err(path_failure)?;
                // Private file creation is flat in this capability version; no recursive quota gaps.
                if path.parent() != Some(root.as_path()) {
                    return Err(Failure::new(
                        ErrorCode::InvalidPath,
                        "Private data files must be direct children",
                    ));
                }
                let mut sizes = BTreeMap::new();
                for entry in std::fs::read_dir(&root).map_err(io_failure)? {
                    let entry = entry.map_err(io_failure)?;
                    sizes.insert(entry.path(), entry.metadata().map_err(io_failure)?.len());
                }
                if let Some(writes) = &self.staged_writes {
                    for (path, bytes) in writes {
                        sizes.insert(path.clone(), bytes.len() as u64);
                    }
                }
                sizes.insert(path.clone(), bytes.len() as u64);
                if bytes.len() > 1024 * 1024
                    || sizes.values().sum::<u64>() > self.roots.limit as u64
                {
                    return Err(Failure::new(
                        ErrorCode::LimitExceeded,
                        "Private data quota exceeded",
                    ));
                }
                if let Some(writes) = &mut self.staged_writes {
                    if writes.len() >= 64 && !writes.contains_key(&path) {
                        return Err(Failure::new(
                            ErrorCode::LimitExceeded,
                            "Initialization write quota exceeded",
                        ));
                    }
                    writes.insert(path, bytes);
                } else {
                    super::super::package::atomic_write(&path, &bytes).map_err(|error| {
                        Failure::new(ErrorCode::OperationFailed, error.to_string())
                    })?;
                }
                Ok(Value::Unit)
            }
            api::Operation::CloseResource { handle } => {
                self.roots.resolve(&handle)?;
                self.plugin_services.resources.remove(&handle.resource);
                if let RootKind::Process(_) = self.roots.resolve(&handle)? {
                    return self
                        .process_request(plugin_protocol::process::Operation::Terminate { handle })
                        .map(|_| Value::Unit);
                }
                self.subscriptions.remove(&handle.resource);
                self.image_inputs.remove(&handle.resource);
                self.plugin_services.references.remove(&handle.resource);
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
            | api::Operation::DescribeSdk
            | api::Operation::Service { .. }
            | api::Operation::Process { .. }
            | api::Operation::SubscribeDocuments
            | api::Operation::Editor { .. }
            | api::Operation::CancelRequest { .. } => {
                unreachable!("assets are handled before resource dispatch")
            }
        }
    }
}

/// Preserve missing-file errors while keeping escaped roots and malformed paths distinguishable.
fn path_failure(error: anyhow::Error) -> Failure {
    if let Some(error) = error.downcast_ref::<std::io::Error>() {
        if error.kind() == std::io::ErrorKind::NotFound {
            return Failure::new(ErrorCode::NotFound, error.to_string());
        }
    }
    Failure::new(ErrorCode::InvalidPath, error.to_string())
}
fn io_failure(error: std::io::Error) -> Failure {
    Failure::new(
        if error.kind() == std::io::ErrorKind::NotFound {
            ErrorCode::NotFound
        } else {
            ErrorCode::OperationFailed
        },
        error.to_string(),
    )
}
