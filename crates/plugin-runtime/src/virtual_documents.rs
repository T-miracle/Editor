//! Shared authority revokes native readonly tabs without retaining their text.
use plugin_protocol::api::{DocumentVersion, ResourceHandle};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

/// An instance-owned resource binds to one native document incarnation and version.
pub struct VirtualDocumentResource {
    handle: ResourceHandle,
    alive: Arc<AtomicBool>,
    revoked: AtomicBool,
    document: Mutex<Option<DocumentVersion>>,
}
impl VirtualDocumentResource {
    /// Only admitted instances allocate this persistent authority.
    pub(crate) fn new(handle: ResourceHandle, alive: Arc<AtomicBool>) -> Self {
        Self {
            handle,
            alive,
            revoked: AtomicBool::new(false),
            document: Mutex::new(None),
        }
    }
    /// Opaque identity never grants filesystem access.
    pub fn handle(&self) -> &ResourceHandle {
        &self.handle
    }
    /// Instance cutover and close seal every native clone.
    pub fn is_live(&self) -> bool {
        self.alive.load(Ordering::Acquire) && !self.revoked.load(Ordering::Acquire)
    }
    /// Native close uses the same revocation path as explicit release.
    pub fn revoke(&self) {
        self.revoked.store(true, Ordering::Release);
    }
    /// Publish metadata without a second text or undo state.
    pub fn bind(&self, document: DocumentVersion) {
        *self.document.lock().unwrap() = Some(document);
    }
    /// Runtime checks metadata ownership, not editable content.
    pub(crate) fn document(&self) -> Option<DocumentVersion> {
        self.document.lock().unwrap().clone()
    }
    /// This URI is not a workspace path and has no temporary backing file.
    pub fn uri(&self) -> String {
        format!(
            "nanobug-virtual://{}/{}",
            self.handle.instance, self.handle.resource
        )
    }
}
