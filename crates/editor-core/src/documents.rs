use std::path::{Path, PathBuf};
use thiserror::Error;

pub trait DocumentStore {
    fn read_utf8(&self, path: &Path) -> Result<String, DocumentError>;
    fn write_utf8(&self, path: &Path, contents: &str) -> Result<(), DocumentError>;
}

#[derive(Debug, Error)]
pub enum DocumentError {
    #[error("document is readonly: {0}")]
    ReadOnly(PathBuf),
    #[error("document has no file name: {0}")]
    MissingFileName(PathBuf),
    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Clone)]
pub struct OpenedDocument {
    pub session: DocumentSession,
    pub contents: String,
}

/// Tracks file identity and revisions while the UI editor owns the live text.
#[derive(Debug, Clone)]
pub struct DocumentSession {
    path: PathBuf,
    revision: u64,
    saved_revision: u64,
    /// Readonly session identities are never interpreted as filesystem save destinations.
    readonly: bool,
}

impl DocumentSession {
    pub fn open(
        store: &impl DocumentStore,
        path: impl Into<PathBuf>,
    ) -> Result<OpenedDocument, DocumentError> {
        let path = path.into();
        let contents = store.read_utf8(&path)?;
        Ok(OpenedDocument {
            session: Self {
                path,
                revision: 0,
                saved_revision: 0,
                readonly: false,
            },
            contents,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Construct metadata for host-owned readonly text; the native editor still owns its value.
    /// The identity may be a non-file URI and must never be passed to a DocumentStore.
    pub fn readonly_resource(identity: impl Into<PathBuf>, contents: String) -> OpenedDocument {
        OpenedDocument {
            session: Self {
                path: identity.into(),
                revision: 0,
                saved_revision: 0,
                readonly: true,
            },
            contents,
        }
    }
    /// Callers must offer explicit read/edit/save abilities rather than assuming a disk file.
    pub fn is_readonly(&self) -> bool {
        self.readonly
    }

    /// Preserve the editor revision when a paired filesystem rename moves this file.
    pub fn rename(&mut self, path: PathBuf) {
        self.path = path;
    }

    pub fn file_name(&self) -> Result<&str, DocumentError> {
        self.path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| DocumentError::MissingFileName(self.path.clone()))
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    pub fn note_edit(&mut self) {
        if self.readonly {
            return;
        }
        self.revision = self.revision.saturating_add(1);
    }

    /// A host-confirmed replacement (disk reload or readonly provider refresh) becomes the saved baseline.
    /// Call only after replacing the editor value, including an explicit discard of unsaved content.
    pub fn accept_disk_reload(&mut self) {
        self.revision = self.revision.saturating_add(1);
        self.saved_revision = self.revision;
    }

    pub fn save(
        &mut self,
        store: &impl DocumentStore,
        current_editor_text: &str,
    ) -> Result<(), DocumentError> {
        if self.readonly {
            return Err(DocumentError::ReadOnly(self.path.clone()));
        }
        store.write_utf8(&self.path, current_editor_text)?;
        self.saved_revision = self.revision;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::HashMap};

    #[derive(Default)]
    struct MemoryStore(RefCell<HashMap<PathBuf, String>>);

    impl DocumentStore for MemoryStore {
        fn read_utf8(&self, path: &Path) -> Result<String, DocumentError> {
            Ok(self.0.borrow().get(path).cloned().unwrap_or_default())
        }

        fn write_utf8(&self, path: &Path, contents: &str) -> Result<(), DocumentError> {
            self.0
                .borrow_mut()
                .insert(path.to_path_buf(), contents.into());
            Ok(())
        }
    }

    #[test]
    fn edit_and_save_updates_dirty_state() {
        let path = PathBuf::from("main.rs");
        let store = MemoryStore::default();
        store
            .0
            .borrow_mut()
            .insert(path.clone(), "fn main() {}".into());

        let opened = DocumentSession::open(&store, &path).unwrap();
        let mut session = opened.session;
        assert!(!session.is_dirty());

        session.note_edit();
        assert!(session.is_dirty());
        session
            .save(&store, "fn main() { println!(\"ok\"); }")
            .unwrap();

        assert!(!session.is_dirty());
        assert_eq!(session.revision(), 1);
    }

    /// Readonly URI sessions never call a file writer, including after provider refresh.
    #[test]
    fn readonly_resource_cannot_be_saved_as_a_file() {
        let store = MemoryStore::default();
        let mut session =
            DocumentSession::readonly_resource("nanobug-virtual://instance/1", "preview".into())
                .session;
        session.note_edit();
        assert!(!session.is_dirty());
        session.accept_disk_reload();
        assert_eq!(session.revision(), 1);
        assert!(!session.is_dirty());
        assert!(matches!(
            session.save(&store, "updated"),
            Err(DocumentError::ReadOnly(_))
        ));
        assert!(store.0.borrow().is_empty());
    }
}
