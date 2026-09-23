use std::path::{Path, PathBuf};
use thiserror::Error;

pub trait DocumentStore {
    fn read_utf8(&self, path: &Path) -> Result<String, DocumentError>;
    fn write_utf8(&self, path: &Path, contents: &str) -> Result<(), DocumentError>;
}

#[derive(Debug, Error)]
pub enum DocumentError {
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
            },
            contents,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
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
        self.revision = self.revision.saturating_add(1);
    }

    pub fn save(
        &mut self,
        store: &impl DocumentStore,
        current_editor_text: &str,
    ) -> Result<(), DocumentError> {
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
}
