//! Adapts approved generic service plans to shared document transport without language-specific startup.
use super::*;
#[cfg(test)]
mod tests;
impl LanguageServer {
    /// Bind transport lifetime to a validated runtime service plan.
    pub(crate) fn from_service(service: Arc<plugin_runtime::LanguageService>) -> Option<Self> {
        let root_uri = file_uri(&service.root)?;
        Some(Self {
            recovery: Default::default(),
            attempt: Mutex::new(()),
            started: Instant::now(),
            documents: Default::default(),
            root: service.root.clone(),
            root_uri,
            service,
            retired: Default::default(),
            connection: Mutex::new(None),
        })
    }
    pub(crate) fn uses_service(&self, service: &Arc<plugin_runtime::LanguageService>) -> bool {
        Arc::ptr_eq(&self.service, service)
    }
    pub(crate) fn is_active(&self) -> bool {
        !self.retired.load(std::sync::atomic::Ordering::Acquire) && self.service.is_active()
    }
    /// Revocation does not wait for a request holding the connection lock on another thread.
    pub(crate) fn retire(&self) {
        if self.retired.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return;
        }
        self.service.stop_processes();
        if let Ok(mut connection) = self.connection.try_lock() {
            *connection = None;
        }
    }

    /// Sequential transport fixtures use the same synchronous lifetime revocation as the editor.
    #[cfg(test)]
    pub(crate) fn document_closed(&self, uri: Uri) -> anyhow::Result<()> {
        if let Some(document) = self.retire_document(&uri) {
            self.close_document(document)?;
        }
        Ok(())
    }
}
