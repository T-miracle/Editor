//! Document leases retire synchronously on the UI thread, before asynchronous close reaches the wire.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone)]
pub(crate) struct DocumentLease {
    pub uri: Uri,
    active: Arc<AtomicBool>,
}
impl DocumentLease {
    pub(crate) fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
    fn same_lifetime(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.active, &other.active)
    }
}
impl LanguageServer {
    /// Save completions must not create a new document after the user has already closed its tab.
    pub(crate) fn document(&self, uri: &Uri) -> Option<DocumentLease> {
        self.documents.lock().unwrap().get(uri.as_str()).cloned()
    }
    /// All providers for the same open document share one lifetime; reopening allocates a fresh one.
    pub(crate) fn open_document(&self, uri: Uri) -> DocumentLease {
        self.documents
            .lock()
            .unwrap()
            .entry(uri.as_str().into())
            .or_insert_with(|| DocumentLease {
                uri,
                active: Arc::new(AtomicBool::new(true)),
            })
            .clone()
    }
    pub(crate) fn retire_document(&self, uri: &Uri) -> Option<DocumentLease> {
        let lease = self.documents.lock().unwrap().remove(uri.as_str())?;
        lease.active.store(false, Ordering::Release);
        Some(lease)
    }
    /// Only the matching old wire lifetime may close; a delayed close cannot remove a reopened document.
    pub(crate) fn close_document(&self, document: DocumentLease) -> anyhow::Result<()> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("LSP connection lock poisoned"))?;
        if let Some(connection) = &mut *connection {
            if connection
                .documents
                .get(document.uri.as_str())
                .is_some_and(|active| active.same_lifetime(&document))
            {
                connection.close_synchronized_document(&document.uri)?;
                connection.documents.remove(document.uri.as_str());
            }
        }
        Ok(())
    }
}
impl LanguageServerConnection {
    /// This runs under the connection lock before synchronization, so queued retired work cannot didOpen again.
    pub(super) fn bind_document(&mut self, document: &DocumentLease) -> anyhow::Result<()> {
        ensure!(document.is_active(), "Document has been closed");
        if let Some(old) = self.documents.get(document.uri.as_str()) {
            if old.same_lifetime(document) {
                return Ok(());
            }
            self.close_synchronized_document(&document.uri)?;
        }
        self.documents
            .insert(document.uri.as_str().into(), document.clone());
        Ok(())
    }
}

/// Legacy/native transport tests are sequential callers; production captures leases before dispatch.
#[cfg(test)]
impl LanguageServer {
    pub(crate) fn definitions(
        &self,
        uri: Uri,
        source: String,
        position: Position,
    ) -> anyhow::Result<Vec<LocationLink>> {
        self.definitions_for(self.open_document(uri), source, position)
    }
    pub(crate) fn completions(
        &self,
        uri: Uri,
        source: String,
        position: Position,
    ) -> anyhow::Result<CompletionResponse> {
        self.completions_for(self.open_document(uri), source, position)
    }
    pub(crate) fn hover(
        &self,
        uri: Uri,
        source: String,
        position: Position,
    ) -> anyhow::Result<Option<Hover>> {
        self.hover_for(self.open_document(uri), source, position)
    }
    pub(crate) fn diagnostics(
        &self,
        uri: Uri,
        source: &str,
    ) -> anyhow::Result<Option<Vec<lsp_types::Diagnostic>>> {
        self.diagnostics_for(self.open_document(uri), source)
    }
    pub(crate) fn document_saved(&self, uri: Uri, source: String) -> anyhow::Result<()> {
        self.document_saved_for(self.open_document(uri), source)
    }
}
