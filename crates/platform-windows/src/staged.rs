//! Prepare slow writes off-thread, then commit through DocumentSession after host revision checks.
use super::*;
use std::cell::RefCell;

/// An immutable, single-use save; dropping it before commit removes the temporary file.
pub struct PreparedFileStore {
    path: PathBuf,
    contents: String,
    temporary: RefCell<Option<NamedTempFile>>,
}

impl PreparedFileStore {
    /// Flush only a sibling temporary file; the destination remains untouched until write_utf8.
    pub fn prepare(path: PathBuf, contents: String) -> Result<Self, DocumentError> {
        let mut temporary = NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))
            .map_err(|source| DocumentError::Write {
                path: path.clone(),
                source,
            })?;
        temporary
            .write_all(contents.as_bytes())
            .and_then(|_| temporary.as_file().sync_all())
            .map_err(|source| DocumentError::Write {
                path: path.clone(),
                source,
            })?;
        Ok(Self {
            path,
            contents,
            temporary: RefCell::new(Some(temporary)),
        })
    }

    /// DocumentSession receives the exact snapshot that was prepared, never a newer buffer.
    pub fn contents(&self) -> &str {
        &self.contents
    }
}

impl DocumentStore for PreparedFileStore {
    fn read_utf8(&self, path: &Path) -> Result<String, DocumentError> {
        NativeFileStore.read_utf8(path)
    }

    fn write_utf8(&self, path: &Path, contents: &str) -> Result<(), DocumentError> {
        // Reject accidental reuse for another document or revision instead of saving mismatched bytes.
        if path != self.path || contents != self.contents {
            return Err(DocumentError::Write {
                path: path.into(),
                source: std::io::Error::other("Prepared save target or contents changed"),
            });
        }
        let temporary = self
            .temporary
            .borrow_mut()
            .take()
            .ok_or_else(|| DocumentError::Write {
                path: path.into(),
                source: std::io::Error::other("Prepared save was already committed"),
            })?;
        temporary
            .persist(path)
            .map_err(|error| DocumentError::Write {
                path: path.into(),
                source: error.error,
            })?;
        Ok(())
    }
}
