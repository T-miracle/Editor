//! Scoped file IO shares path, quota and staged-transaction rules with opaque preferences.
use super::*;
use api::{ErrorCode, Failure};
use std::{collections::BTreeMap, io::Read};

/// Read only within an already authorized root, including uncommitted private migration writes.
pub(super) fn read(
    root: &Path,
    relative: &str,
    staged: &Option<BTreeMap<PathBuf, Vec<u8>>>,
) -> Result<Vec<u8>, Failure> {
    let path = safe_path(root, relative, false).map_err(path_failure)?;
    if let Some(bytes) = staged.as_ref().and_then(|writes| writes.get(&path)) {
        return Ok(bytes.clone());
    }
    let file = std::fs::File::open(path).map_err(io_failure)?;
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_failure)?;
    if bytes.len() > 1024 * 1024 {
        return Err(Failure::new(
            ErrorCode::LimitExceeded,
            "File read quota exceeded",
        ));
    }
    Ok(bytes)
}

/// Private writes are flat and count all disk/staged records before any mutation or atomic replace.
pub(super) fn write(
    root: &Path,
    relative: &str,
    bytes: Vec<u8>,
    limit: usize,
    staged: &mut Option<BTreeMap<PathBuf, Vec<u8>>>,
) -> Result<(), Failure> {
    let root = root.canonicalize().map_err(io_failure)?;
    let path = safe_path(&root, relative, false).map_err(path_failure)?;
    if path.parent() != Some(root.as_path()) {
        return Err(Failure::new(
            ErrorCode::InvalidPath,
            "Private data files must be direct children",
        ));
    }
    let mut sizes = BTreeMap::new();
    for entry in std::fs::read_dir(&root).map_err(io_failure)? {
        let entry = entry.map_err(io_failure)?;
        sizes.insert(entry.path(), entry.metadata().map_err(io_failure)?.len());
    }
    if let Some(writes) = staged.as_ref() {
        for (path, bytes) in writes {
            sizes.insert(path.clone(), bytes.len() as u64);
        }
    }
    sizes.insert(path.clone(), bytes.len() as u64);
    if bytes.len() > 1024 * 1024 || sizes.values().sum::<u64>() > limit as u64 {
        return Err(Failure::new(
            ErrorCode::LimitExceeded,
            "Private data quota exceeded",
        ));
    }
    if let Some(writes) = staged {
        if writes.len() >= 64 && !writes.contains_key(&path) {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Initialization write quota exceeded",
            ));
        }
        writes.insert(path, bytes);
    } else {
        super::super::package::atomic_write(&path, &bytes)
            .map_err(|error| Failure::new(ErrorCode::OperationFailed, error.to_string()))?;
    }
    Ok(())
}

/// Keep missing records distinguishable from malformed paths without granting an escaped root.
fn path_failure(error: anyhow::Error) -> Failure {
    if let Some(error) = error.downcast_ref::<std::io::Error>()
        && error.kind() == std::io::ErrorKind::NotFound
    {
        return Failure::new(ErrorCode::NotFound, error.to_string());
    }
    Failure::new(ErrorCode::InvalidPath, error.to_string())
}
fn io_failure(error: std::io::Error) -> Failure {
    Failure::new(
        if error.kind() == std::io::ErrorKind::NotFound {
            ErrorCode::NotFound
        } else {
            ErrorCode::OperationFailed
        },
        error.to_string(),
    )
}
