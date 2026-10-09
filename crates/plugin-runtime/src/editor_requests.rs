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
    /// Source wait cancellation closes pending UI, while successful calls may retain owned work.
    parents: Vec<Completion<serde_json::Value>>,
    /// Native paths never cross the guest transport; the instance issues grants only at delivery.
    selection: std::sync::Arc<std::sync::Mutex<Option<Vec<std::path::PathBuf>>>>,
    /// Replaceable presentation belongs to this one request, never a second cancellable task.
    progress: std::sync::Arc<std::sync::Mutex<(String, Option<u8>)>>,
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
        let parents = context.map_or_else(Vec::new, |context| context.native_waits.clone());
        completion
            .lifetimes
            .extend(parents.iter().map(Completion::wait_lifetime));
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
            parents,
            selection: Default::default(),
            progress: Default::default(),
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
        self.expire_parents();
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
        self.expire_parents();
        // A normal completion cannot fabricate selected handles or bypass the trusted picker seam.
        if matches!(
            self.operation,
            EditorOperation::Interaction {
                operation: plugin_protocol::interaction::Operation::Select { .. }
            }
        ) && result.is_ok()
        {
            self.completion.finish(Err(Failure::new(
                ErrorCode::InvalidRequest,
                "Selection requires native paths",
            )));
            return;
        }
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
    /// Complete a native selection with user-chosen paths. No guest-supplied path is accepted here.
    /// Cancellation, timeout and retirement win over a late callback. The worker validates paths
    /// and allocates instance-owned handles before delivering the final result to the guest.
    pub fn finish_selection(&self, paths: Vec<std::path::PathBuf>) {
        self.expire_parents();
        if !matches!(
            self.operation,
            EditorOperation::Interaction {
                operation: plugin_protocol::interaction::Operation::Select { .. }
            }
        ) {
            self.completion.finish(Err(Failure::new(
                ErrorCode::InvalidRequest,
                "Not a selection request",
            )));
            return;
        }
        let mut selection = self.selection.lock().unwrap();
        if selection.is_some() || self.completion.status().is_terminal() {
            return;
        }
        *selection = Some(paths);
        self.completion.finish(Ok(EditorValue::Unit));
    }
    /// Consume trusted picker data once, while the owning instance is delivering this completion.
    pub(crate) fn take_selection(&self) -> Option<Vec<std::path::PathBuf>> {
        self.selection.lock().unwrap().take()
    }
    pub fn status(&self) -> RequestUpdate {
        self.expire_parents();
        self.completion.status()
    }
    /// Native progress reads the latest bounded state without locking a WASM instance.
    pub fn progress(&self) -> (String, Option<u8>) {
        self.progress.lock().unwrap().clone()
    }
    /// Replace visible progress only while its original pending lifetime is still valid.
    pub(crate) fn update_progress(
        &self,
        message: String,
        percent: Option<u8>,
    ) -> Result<(), Failure> {
        if self.status().is_terminal()
            || !matches!(
                self.operation,
                EditorOperation::Interaction {
                    operation: plugin_protocol::interaction::Operation::Progress { .. }
                }
            )
        {
            return Err(Failure::new(
                ErrorCode::InvalidHandle,
                "Progress task ended or has another kind",
            ));
        }
        *self.progress.lock().unwrap() = (message, percent);
        Ok(())
    }
    /// A native user dismissal seals this wait; it never claims to undo an entered side effect.
    pub fn cancel_from_host(&self, mode: CancelMode) {
        let _ = self.completion.cancel(mode, ErrorCode::Cancelled);
    }
    pub(crate) fn update(&self) -> (u64, RequestUpdate) {
        self.expire_parents();
        self.completion.update()
    }
    pub(crate) fn retire(&self) {
        self.completion.retire();
    }
    pub fn enter_side_effect(&self) -> bool {
        self.expire_parents();
        self.completion.enter_side_effect()
    }
    pub fn expire(&self, now: Instant) {
        self.expire_parents();
        self.completion.expire(now);
    }
    /// Observe parent deadlines before checking their cancellation token on a native thread.
    fn expire_parents(&self) {
        for parent in &self.parents {
            parent.status();
        }
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
