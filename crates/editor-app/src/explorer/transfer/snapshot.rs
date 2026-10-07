//! Private session backups and content fingerprints used by both transfer and recovery.

use rust_i18n::t;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

/// Disk identity includes content, entry kind and permissions; timestamps alone miss quick edits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Stamp {
    Missing,
    File([u8; 32], bool),
    Directory([u8; 32]),
}

/// An immutable backup outside the workspace. Directory assets are needed only for actual removals.
#[derive(Clone, Debug)]
pub(super) enum Snapshot {
    Missing,
    File {
        asset: PathBuf,
        permissions: fs::Permissions,
    },
    Directory {
        asset: Option<PathBuf>,
    },
}

/// Treat Windows junctions and every other reparse point as links, even when Rust reports a directory.
pub(super) fn is_link(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        return metadata.file_attributes() & 0x400 != 0;
    }
    #[cfg(not(windows))]
    false
}

/// Resolve existing ancestors and reject links below the canonical workspace boundary.
pub(super) fn check_target(root: &Path, path: &Path) -> Result<(), String> {
    check_native_path(path)?;
    let root_metadata = fs::symlink_metadata(root).map_err(|error| error.to_string())?;
    if is_link(&root_metadata)
        || !root_metadata.is_dir()
        || root.canonicalize().map_err(|error| error.to_string())? != root
    {
        // The workspace was canonical when opened; a replaced ancestor must not redirect later writes.
        return Err(t!("transfer.link_target").to_string());
    }
    let relative = path
        .strip_prefix(root)
        .map_err(|_| t!("transfer.outside_workspace").to_string())?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err(t!("explorer.invalid_source").to_string());
        }
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(meta) if is_link(&meta) => {
                return Err(format!(
                    "{}: {}",
                    current.display(),
                    t!("transfer.link_target")
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("{}: {error}", current.display())),
        }
    }
    Ok(())
}

/// Reject device namespaces, alternate data streams and ambiguous native filenames before disk access.
pub(super) fn check_native_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(t!("explorer.invalid_source").to_string());
    }
    for component in path.components() {
        match component {
            std::path::Component::ParentDir | std::path::Component::CurDir => {
                return Err(t!("explorer.invalid_source").to_string());
            }
            #[cfg(windows)]
            std::path::Component::Prefix(prefix) => {
                if !matches!(
                    prefix.kind(),
                    std::path::Prefix::Disk(_)
                        | std::path::Prefix::VerbatimDisk(_)
                        | std::path::Prefix::UNC(_, _)
                        | std::path::Prefix::VerbatimUNC(_, _)
                ) {
                    return Err(t!("explorer.invalid_source").to_string());
                }
            }
            #[cfg(windows)]
            std::path::Component::Normal(name) => {
                let name = name.to_string_lossy();
                let stem = name
                    .split('.')
                    .next()
                    .unwrap_or_default()
                    .to_ascii_uppercase();
                if name.contains(':')
                    || name.ends_with(['.', ' '])
                    || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                    || stem
                        .strip_prefix("COM")
                        .or_else(|| stem.strip_prefix("LPT"))
                        .is_some_and(|number| {
                            matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
                        })
                {
                    return Err(t!("explorer.invalid_source").to_string());
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Recovery and offered external move sources may leave the workspace; no existing ancestor may be a link.
pub(super) fn check_ancestors(path: &Path) -> Result<(), String> {
    check_native_path(path)?;
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if is_link(&metadata) => {
                return Err(format!(
                    "{}: {}",
                    ancestor.display(),
                    t!("transfer.link_target")
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("{}: {error}", ancestor.display())),
        }
    }
    Ok(())
}

/// Hash a complete subtree without following links; unknown entry kinds are never treated as files.
pub(super) fn stamp(path: &Path) -> Result<Stamp, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Stamp::Missing),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    if is_link(&metadata) {
        return Err(format!(
            "{}: {}",
            path.display(),
            t!("transfer.link_target")
        ));
    }
    if metadata.is_file() {
        let mut input = fs::File::open(path).map_err(|error| error.to_string())?;
        let mut digest = Sha256::new();
        let mut buffer = vec![0; 256 * 1024];
        loop {
            let count = input.read(&mut buffer).map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        return Ok(Stamp::File(
            digest.finalize().into(),
            metadata.permissions().readonly(),
        ));
    }
    if !metadata.is_dir() {
        return Err(format!(
            "{}: {}",
            path.display(),
            t!("transfer.unsupported")
        ));
    }
    let mut entries = fs::read_dir(path)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    let mut digest = Sha256::new();
    for entry in entries {
        let name = entry.file_name();
        digest.update(name.as_encoded_bytes().len().to_le_bytes());
        digest.update(name.as_encoded_bytes());
        let child = entry.path();
        let child_meta = fs::symlink_metadata(&child).map_err(|error| error.to_string())?;
        if is_link(&child_meta) {
            // Fingerprint link identity without following it; unrelated links survive directory merges.
            digest.update([3]);
            digest.update(
                fs::read_link(&child)
                    .map_err(|error| error.to_string())?
                    .as_os_str()
                    .as_encoded_bytes(),
            );
            continue;
        }
        match stamp(&child)? {
            Stamp::File(hash, readonly) => {
                digest.update([1, readonly as u8]);
                digest.update(hash);
            }
            Stamp::Directory(hash) => {
                digest.update([2]);
                digest.update(hash);
            }
            Stamp::Missing => {
                return Err(format!("{}: {}", path.display(), t!("transfer.changed")));
            }
        }
    }
    Ok(Stamp::Directory(digest.finalize().into()))
}

/// Save the current entry with no links; the caller owns the enclosing private TempDir.
pub(super) fn capture(path: &Path, backup: &Path) -> Result<Snapshot, String> {
    match stamp(path)? {
        Stamp::Missing => Ok(Snapshot::Missing),
        Stamp::File(_, _) => {
            let asset = tempfile::NamedTempFile::new_in(backup)
                .map_err(|error| error.to_string())?
                .into_temp_path()
                .keep()
                .map_err(|error| error.to_string())?;
            fs::copy(path, &asset).map_err(|error| error.to_string())?;
            Ok(Snapshot::File {
                asset,
                permissions: fs::metadata(path)
                    .map_err(|error| error.to_string())?
                    .permissions(),
            })
        }
        Stamp::Directory(_) => {
            if fs::read_dir(path)
                .map_err(|error| error.to_string())?
                .next()
                .is_none()
            {
                return Ok(Snapshot::Directory { asset: None });
            }
            let asset = tempfile::Builder::new()
                .prefix("directory-")
                .tempdir_in(backup)
                .map_err(|error| error.to_string())?
                .keep();
            copy_directory(path, &asset)?;
            Ok(Snapshot::Directory { asset: Some(asset) })
        }
    }
}

/// Copy a reviewed backup tree; rechecking each entry prevents a link swapped in during inspection.
fn copy_directory(source: &Path, target: &Path) -> Result<(), String> {
    for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let from = entry.path();
        let to = target.join(entry.file_name());
        let metadata = fs::symlink_metadata(&from).map_err(|error| error.to_string())?;
        if is_link(&metadata) {
            return Err(format!(
                "{}: {}",
                from.display(),
                t!("transfer.link_target")
            ));
        }
        if metadata.is_dir() {
            fs::create_dir(&to).map_err(|error| error.to_string())?;
            copy_directory(&from, &to)?;
        } else if metadata.is_file() {
            fs::copy(&from, &to).map_err(|error| error.to_string())?;
        } else {
            return Err(format!(
                "{}: {}",
                from.display(),
                t!("transfer.unsupported")
            ));
        }
    }
    Ok(())
}

/// Atomically install file bytes in the target's volume; failed writes leave the old file in place.
pub(super) fn install_file(
    asset: &Path,
    target: &Path,
    permissions: &fs::Permissions,
) -> Result<(), String> {
    let parent = target
        .parent()
        .ok_or_else(|| t!("transfer.missing_parent").to_string())?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".me-transfer-")
        .tempfile_in(parent)
        .map_err(|error| error.to_string())?;
    let mut input = fs::File::open(asset).map_err(|error| error.to_string())?;
    std::io::copy(&mut input, temporary.as_file_mut()).map_err(|error| error.to_string())?;
    temporary.flush().map_err(|error| error.to_string())?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    temporary
        .as_file()
        .set_permissions(permissions.clone())
        .map_err(|error| error.to_string())?;
    temporary
        .persist(target)
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Restore a captured entry. This is called only after conflict consent and boundary checks.
pub(super) fn restore(snapshot: &Snapshot, path: &Path) -> Result<(), String> {
    match snapshot {
        Snapshot::File { asset, permissions } if !path.is_dir() => {
            install_file(asset, path, permissions)
        }
        _ => {
            let metadata = fs::symlink_metadata(path).ok();
            if let Some(meta) = metadata {
                if is_link(&meta) {
                    return Err(format!(
                        "{}: {}",
                        path.display(),
                        t!("transfer.link_target")
                    ));
                }
                if meta.is_dir() {
                    // Check the whole tree before recursive removal; force consent never follows links.
                    stamp(path)?;
                    fs::remove_dir_all(path).map_err(|error| error.to_string())?;
                } else {
                    fs::remove_file(path).map_err(|error| error.to_string())?;
                }
            }
            match snapshot {
                Snapshot::Missing => Ok(()),
                Snapshot::File { asset, permissions } => install_file(asset, path, permissions),
                Snapshot::Directory { asset } => {
                    fs::create_dir(path).map_err(|error| error.to_string())?;
                    if let Some(asset) = asset {
                        copy_directory(asset, path)?;
                    }
                    Ok(())
                }
            }
        }
    }
}
