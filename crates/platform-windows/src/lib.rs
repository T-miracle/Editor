use editor_core::{DocumentError, DocumentStore};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tempfile::NamedTempFile;
use thiserror::Error;

mod staged;
pub use staged::PreparedFileStore;
mod attachments;
pub use attachments::{NewWorkspaceFileStore, create_document_attachment};
pub mod file_picker;

#[derive(Debug, Default, Clone, Copy)]
pub struct NativeFileStore;

impl DocumentStore for NativeFileStore {
    fn read_utf8(&self, path: &Path) -> Result<String, DocumentError> {
        fs::read_to_string(path).map_err(|source| DocumentError::Read {
            path: path.to_path_buf(),
            source,
        })
    }

    fn write_utf8(&self, path: &Path, contents: &str) -> Result<(), DocumentError> {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let mut temporary =
            NamedTempFile::new_in(parent).map_err(|source| DocumentError::Write {
                path: path.to_path_buf(),
                source,
            })?;
        temporary
            .write_all(contents.as_bytes())
            .and_then(|_| temporary.as_file().sync_all())
            .map_err(|source| DocumentError::Write {
                path: path.to_path_buf(),
                source,
            })?;
        temporary
            .persist(path)
            .map_err(|error| DocumentError::Write {
                path: path.to_path_buf(),
                source: error.error,
            })?;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum HistoryError {
    #[error("a local data directory is unavailable")]
    DataDirectoryUnavailable,
    #[error("failed to create history directory {path}: {source}")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write history snapshot {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Clone)]
pub struct LocalHistory {
    root: PathBuf,
}

impl LocalHistory {
    pub fn for_current_user() -> Result<Self, HistoryError> {
        // Development profiles retain history independently from the user's normal editor data.
        if let Some(root) =
            std::env::var_os("ME_EDITOR_PROFILE_HOME").filter(|root| !root.is_empty())
        {
            return Self::at(PathBuf::from(root).join("history"));
        }
        let root = dirs::data_local_dir()
            .ok_or(HistoryError::DataDirectoryUnavailable)?
            .join("MeEditor")
            .join("history");
        Self::at(root)
    }

    pub fn at(root: impl Into<PathBuf>) -> Result<Self, HistoryError> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(|source| HistoryError::CreateDirectory {
            path: root.clone(),
            source,
        })?;
        Ok(Self { root })
    }

    pub fn snapshot_file(&self, source_path: &Path) -> Result<PathBuf, HistoryError> {
        let contents = fs::read(source_path).map_err(|source| HistoryError::Read {
            path: source_path.to_path_buf(),
            source,
        })?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let safe_name = source_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("document")
            .replace(
                |character: char| !character.is_ascii_alphanumeric() && character != '.',
                "_",
            );
        let directory = self
            .root
            .join(format!("{:016x}", stable_path_hash(source_path)));
        fs::create_dir_all(&directory).map_err(|source| HistoryError::CreateDirectory {
            path: directory.clone(),
            source,
        })?;
        let destination = directory.join(format!("{timestamp}-{safe_name}"));
        fs::write(&destination, contents).map_err(|source| HistoryError::Write {
            path: destination.clone(),
            source,
        })?;
        Ok(destination)
    }
}

fn stable_path_hash(path: &Path) -> u64 {
    path.to_string_lossy()
        .bytes()
        .fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_is_stored_outside_the_project() {
        let project = tempfile::tempdir().unwrap();
        let history = tempfile::tempdir().unwrap();
        let source = project.path().join("main.rs");
        fs::write(&source, "fn main() {}").unwrap();

        let destination = LocalHistory::at(history.path())
            .unwrap()
            .snapshot_file(&source)
            .unwrap();

        assert!(destination.starts_with(history.path()));
        assert_eq!(fs::read_to_string(destination).unwrap(), "fn main() {}");
    }
}
