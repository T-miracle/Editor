//! Adapts approved generic service plans to shared document transport without language-specific startup.
use super::*;
#[cfg(test)]
mod tests;
impl LanguageServer {
    pub(crate) fn from_service(service: Arc<plugin_runtime::LanguageService>) -> Option<Self> {
        let root_uri = file_uri(&service.root)?;
        // Only legacy consumers still store a LanguageContribution; no grammar is required by this adapter.
        let language = LanguageContribution {
            id: service.provider.language.clone(),
            extensions: vec![],
            filenames: vec![],
            grammar: PathBuf::new(),
            highlights: PathBuf::new(),
            tree_sitter_abi: 0,
            lsp_command: None,
            lsp_args: vec![],
            lsp_search_paths: vec![],
            lsp_check_args: vec![],
            lsp_readiness: None,
            completion_triggers: service.provider.completion_triggers.clone(),
            completion_after_whitespace: service.provider.completion_after_whitespace.clone(),
        };
        Some(Self {
            recovery: Default::default(),
            attempt: Mutex::new(()),
            started: Instant::now(),
            documents: Default::default(),
            root: service.root.clone(),
            root_uri,
            language,
            service: Some(service),
            retired: Default::default(),
            connection: Mutex::new(None),
        })
    }
    pub(crate) fn uses_service(&self, service: &Arc<plugin_runtime::LanguageService>) -> bool {
        self.service
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, service))
    }
    pub(crate) fn is_dynamic(&self) -> bool {
        self.service.is_some()
    }
    pub(crate) fn is_active(&self) -> bool {
        !self.retired.load(std::sync::atomic::Ordering::Acquire)
            && self
                .service
                .as_ref()
                .is_none_or(|service| service.is_active())
    }
    /// Revocation does not wait for a request holding the connection lock on another thread.
    pub(crate) fn retire(&self) {
        if self.retired.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return;
        }
        if let Some(service) = &self.service {
            service.stop_processes();
        }
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
