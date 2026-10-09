//! Owned editor operations use the common completion gate across worker and native UI threads.
use crate::request_state::Completion;
use plugin_protocol::api::{
    CancelMode, CancellationEffect, EditorOperation, EditorValue, ErrorCode, Failure,
    RequestUpdate, ResourceHandle,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

/// Runtime-issued, read-only authority for native resources retained after an editor task completes.
/// It borrows the existing instance and delegated caller lifetimes, never a deadline or result gate.
/// Private fields prevent guests or native callers from constructing or reviving authority.
#[derive(Clone)]
pub struct EditorAuthority {
    owner: Arc<AtomicBool>,
    callers: Vec<Arc<AtomicBool>>,
}

impl EditorAuthority {
    /// Return whether the initiating instance and every delegated source remain authorized.
    /// Completing a task does not retire a persistent view; disable/cutover/trust revocation does.
    pub fn is_live(&self) -> bool {
        self.owner.load(Ordering::Acquire)
            && self
                .callers
                .iter()
                .all(|alive| alive.load(Ordering::Acquire))
    }
}

/// Clones share one completion gate; dropping an instance seals every outstanding call.
#[derive(Clone)]
pub struct EditorRequest {
    handle: ResourceHandle,
    operation: EditorOperation,
    workspace: String,
    /// Trusted runtime metadata; the guest supplies only a relative file name.
    data_root: std::path::PathBuf,
    completion: Completion<EditorValue>,
    /// Persistent views must outlive completion while still observing their initiating authority.
    authority: EditorAuthority,
    /// Only native-offered bytes can back an image save; the JSON operation contains an opaque handle.
    image_input: Option<std::sync::Arc<crate::ImageInputResource>>,
    /// Persistent authority is distinct from this transient completion handle.
    virtual_document: Option<std::sync::Arc<crate::VirtualDocumentResource>>,
}
impl EditorRequest {
    /// Construction follows authority checks in the instance; only typed owned data crosses threads.
    pub(crate) fn new(
        handle: ResourceHandle,
        operation: EditorOperation,
        workspace: String,
        data_root: std::path::PathBuf,
        timeout_ms: u32,
        owner: Arc<AtomicBool>,
        context: Option<&crate::plugin_services::Context>,
    ) -> Self {
        let mut completion = Completion::new(timeout_ms);
        completion.lifetimes = context.map_or_else(Vec::new, |context| context.lifetimes.clone());
        Self {
            handle,
            operation,
            workspace,
            data_root,
            completion,
            authority: EditorAuthority {
                owner,
                // Keep the entire delegated chain rather than substituting the final provider.
                callers: context.map_or_else(Vec::new, |context| context.lifetimes.clone()),
            },
            image_input: None,
            virtual_document: None,
        }
    }
    pub fn handle(&self) -> &ResourceHandle {
        &self.handle
    }
    pub fn operation(&self) -> &EditorOperation {
        &self.operation
    }
    /// Borrow the unforgeable lifetime authority for an admitted persistent native view.
    /// Hosts may clone it to observe revocation, but cannot mutate its runtime-owned lifetimes.
    pub fn authority(&self) -> &EditorAuthority {
        &self.authority
    }
    /// Native virtual opens require runtime-issued owned authority.
    pub fn virtual_document(&self) -> Option<&std::sync::Arc<crate::VirtualDocumentResource>> {
        self.virtual_document.as_ref()
    }
    /// Attach after the instance admits ownership, permissions and quota.
    pub(crate) fn with_virtual_document(
        mut self,
        resource: std::sync::Arc<crate::VirtualDocumentResource>,
    ) -> Self {
        self.virtual_document = Some(resource);
        self
    }
    /// The native writer borrows immutable authorized pixels and the original document/selection binding.
    pub fn image_input(&self) -> Option<&crate::ImageInputResource> {
        self.image_input.as_deref()
    }
    /// Attach only after the instance has checked handle ownership, permissions and name shape.
    pub(crate) fn with_image_input(
        mut self,
        input: std::sync::Arc<crate::ImageInputResource>,
    ) -> Self {
        self.image_input = Some(input);
        self
    }
    pub fn workspace(&self) -> &str {
        &self.workspace
    }
    /// The UI revalidates containment at execution rather than trusting a guest-provided native path.
    pub fn data_root(&self) -> &std::path::Path {
        &self.data_root
    }
    pub fn begin(&self) -> bool {
        if self
            .virtual_document
            .as_ref()
            .is_some_and(|resource| !resource.is_live())
        {
            self.completion.finish(Err(Failure::new(
                ErrorCode::InvalidHandle,
                "Virtual resource was revoked",
            )));
            return false;
        }
        self.completion.begin()
    }
    pub fn finish(&self, result: Result<EditorValue, Failure>) {
        let result = if let (
            EditorOperation::SaveImageInput { input, name },
            Some(resource),
            Ok(value),
        ) = (&self.operation, &self.image_input, &result)
        {
            if !matches!(value, EditorValue::ImageSaved { input: saved, document, name: saved_name }
                if saved == input && document == &resource.document && saved_name == name)
            {
                Err(Failure::new(
                    ErrorCode::InvalidRequest,
                    "Image save receipt does not match its owned request",
                ))
            } else {
                result
            }
        } else {
            result
        };
        self.completion.finish(result);
    }
    pub fn status(&self) -> RequestUpdate {
        self.completion.status()
    }
    pub(crate) fn update(&self) -> (u64, RequestUpdate) {
        self.completion.update()
    }
    pub(crate) fn retire(&self) {
        self.completion.retire();
    }
    pub fn enter_side_effect(&self) -> bool {
        self.completion.enter_side_effect()
    }
    pub fn expire(&self, now: Instant) {
        self.completion.expire(now);
    }
    pub(crate) fn cancel(
        &self,
        mode: CancelMode,
        reason: ErrorCode,
    ) -> Result<CancellationEffect, Failure> {
        self.completion.cancel(mode, reason)
    }
}
pub(crate) struct PendingRequest {
    pub context: Option<crate::plugin_services::Context>,
    pub call: EditorRequest,
    pub sent: bool,
    pub reported: u64,
}
