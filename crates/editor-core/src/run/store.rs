//! Host-local persistence for run configurations.
//!
//! Files live beside the editor's other host-local state and never inside the project, so
//! experimenting with a configuration cannot modify shared project files by accident. Writing is
//! atomic: a failed save leaves the previous file intact.
use super::{RUN_CONFIG_VERSION, RunConfigError, RunConfigSet, short_digest};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Failures a caller must explain to the user instead of silently starting with defaults.
#[derive(Debug)]
pub enum RunStoreError {
    /// Reading or writing host-local state failed.
    Io(std::io::Error),
    /// The stored bytes are not a valid set.
    Malformed(serde_json::Error),
    /// The stored set is structurally valid but violates a run configuration rule.
    Invalid(RunConfigError),
}

impl std::fmt::Display for RunStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "Run configuration storage failed: {error}"),
            Self::Malformed(error) => {
                write!(formatter, "Run configuration file is unreadable: {error}")
            }
            Self::Invalid(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for RunStoreError {}

/// Base directory for host-local run configurations, shared with other editor state.
pub fn default_root() -> Option<PathBuf> {
    // Isolated development processes supply their host-local profile before GUI initialization.
    if let Some(root) = std::env::var_os("ME_EDITOR_PROFILE_HOME").filter(|root| !root.is_empty()) {
        return Some(PathBuf::from(root).join("run"));
    }
    Some(dirs::config_dir()?.join("MeEditor").join("run"))
}

/// File holding the configurations for one workspace, addressed by a non-reversible digest.
pub fn file_for(base: &Path, workspace: &str) -> PathBuf {
    base.join(format!("{}.json", short_digest(workspace)))
}

/// File the editor reads and writes for one workspace, under the platform configuration directory.
///
/// Callers outside this crate address configurations by workspace rather than by path, so the
/// storage layout stays one decision in one place.
pub fn storage_path(workspace: &str) -> Option<PathBuf> {
    default_root().map(|base| file_for(&base, workspace))
}

/// Read the stored set, treating a missing file as an empty set.
///
/// A malformed or newer file is reported instead of being replaced: the user's previous
/// configurations must not be discarded merely because this build could not read them.
pub fn load(base: &Path, workspace: &str) -> Result<RunConfigSet, RunStoreError> {
    let path = file_for(base, workspace);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RunConfigSet {
                version: RUN_CONFIG_VERSION,
                ..Default::default()
            });
        }
        Err(error) => return Err(RunStoreError::Io(error)),
    };
    RunConfigSet::from_json(&bytes)
}

/// Write the set atomically, preserving the previous file when any step fails.
pub fn save(base: &Path, workspace: &str, set: &RunConfigSet) -> Result<(), RunStoreError> {
    for configuration in &set.configurations {
        configuration.validate().map_err(RunStoreError::Invalid)?;
    }
    let path = file_for(base, workspace);
    let parent = path.parent().ok_or_else(|| {
        RunStoreError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Run configuration path has no parent directory",
        ))
    })?;
    fs::create_dir_all(parent).map_err(RunStoreError::Io)?;
    let bytes = set.to_json()?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, bytes).map_err(RunStoreError::Io)?;
    fs::rename(&temporary, &path).map_err(RunStoreError::Io)
}
