//! One-time legacy data import is separate from executable API dispatch and preserves its original evidence.
use super::*;
use crate::plugin_protocol::{Environment, Snapshot};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::PathBuf};

/// Backups are immutable per plugin; a copied record is published only after all of its data is complete.
pub(super) fn backup_legacy(
    root: &Path,
    installed: &BTreeMap<String, Installed>,
) -> anyhow::Result<()> {
    let legacy = installed
        .iter()
        .filter(|(_, entry)| matches!(entry.manifest.protocol, 1..=6));
    for (old_id, entry) in legacy {
        let id = canonical_plugin_id(old_id);
        anyhow::ensure!(
            safe_id(old_id) && safe_id(id) && entry.manifest.id == *old_id,
            "Unsafe legacy registry identity"
        );
        let base = root.join("legacy-backup/v7");
        plain_directory(&base)?;
        let registry = base.join("registry.json");
        if !registry.exists() {
            atomic_write(&registry, &fs::read(root.join("registry.json"))?)?;
        }
        let packages = base.join("plugins");
        plain_directory(&packages)?;
        // An existing canonical package wins a collision. Keep the old backup under its original identity,
        // preventing a later scope from silently combining two distinct packages' private files.
        let destination = packages.join(if id != old_id && installed.contains_key(id) {
            old_id
        } else {
            id
        });
        if destination.exists() {
            plain_directory(&destination)?;
            anyhow::ensure!(
                destination.join("record.json").is_file(),
                "Incomplete legacy backup retained"
            );
            continue;
        }
        let staging = tempfile::tempdir_in(&packages)?;
        let source = root.join("data").join(old_id);
        reject_links(&source)?;
        let data = staging.path().join("data");
        fs::create_dir(&data)?;
        if source.exists() {
            crate::data_transaction::copy_tree(&source, &data, &mut (0, 0), 0)?;
        }
        copy_missing(
            &root.join("settings").join(format!("{old_id}.json")),
            &staging.path().join("settings.json"),
        )?;
        atomic_write(
            &staging.path().join("record.json"),
            &serde_json::to_vec_pretty(entry)?,
        )?;
        fs::rename(staging.path(), destination)?;
    }
    Ok(())
}

/// A retained backup is imported only into a scope that has never committed its own import marker.
pub(crate) fn needs_legacy_import(root: &Path, id: &str, scope: &Path) -> bool {
    // Literal current IDs that resemble historical aliases cannot claim a collision backup.
    canonical_plugin_id(id) == id
        && legacy_directory(root, id).join("record.json").is_file()
        && !scope.join("legacy-import.json").exists()
}

/// Stage historical files in the existing transaction, so failed installation cannot consume the import.
pub(crate) fn stage_legacy_data(
    root: &Path,
    id: &str,
    environment: &Environment,
    scope: &Path,
) -> anyhow::Result<()> {
    if !needs_legacy_import(root, id, scope) {
        return Ok(());
    }
    let backup = legacy_directory(root, id);
    plain_directory(&backup)?;
    let record = super::decode_installed(&fs::read(backup.join("record.json"))?)?;
    // The directory name alone does not prove ownership of retained historical evidence.
    if canonical_plugin_id(&record.manifest.id) != id {
        return Ok(());
    }
    let data = backup.join("data");
    let files = scope.join("files");
    fs::create_dir_all(&files)?;
    // Legacy writes were private flat files. New scope folders and historical snapshots are never business files.
    for entry in fs::read_dir(&data)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        anyhow::ensure!(!kind.is_symlink(), "Linked legacy data cannot be imported");
        let name = entry.file_name();
        let text = name.to_string_lossy();
        // Only the old host's exact hash envelope is reserved; similarly named business settings remain opaque.
        let snapshot_name = text
            .strip_prefix("state-")
            .and_then(|name| name.strip_suffix(".json"))
            .is_some_and(|hash| {
                hash.len() == 16 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            });
        if kind.is_file() && !snapshot_name {
            copy_missing(&entry.path(), &files.join(&name))?;
        }
    }
    // Only the current path or an explicitly recorded alias of the same canonical workspace may select a snapshot.
    if !scope.join("state.json").exists() {
        if let Some(snapshot) = legacy_snapshot(&data, &record, environment)? {
            atomic_write(&scope.join("state.json"), &serde_json::to_vec(&snapshot)?)?;
        }
    }
    if !scope.join("data-format.json").exists() {
        atomic_write(&scope.join("data-format.json"), b"1")?;
    }
    atomic_write(&scope.join("legacy-import.json"), br#"{"version":1}"#)?;
    Ok(())
}

/// Persisted snapshots are opaque, but the host bounds their allocation before decoding the envelope.
pub(crate) fn scope_snapshot(scope: &Path) -> anyhow::Result<Option<Snapshot>> {
    read_snapshot(&scope.join("state.json"))
}

/// Explicit data deletion retires the backup and its original alias, preventing a later reinstall from restoring it.
pub(crate) fn discard_legacy_data(
    root: &Path,
    id: &str,
    installed: &BTreeMap<String, Installed>,
) -> anyhow::Result<()> {
    anyhow::ensure!(safe_id(id), "Unsafe legacy deletion identity");
    // A new package may legally use a former alias literally; its deletion must preserve collision evidence.
    if canonical_plugin_id(id) != id {
        return Ok(());
    }
    let backup = legacy_directory(root, id);
    reject_links(&backup)?;
    if !backup.exists() {
        return Ok(());
    }
    let record = super::decode_installed(&fs::read(backup.join("record.json"))?)?;
    let original = &record.manifest.id;
    anyhow::ensure!(safe_id(original), "Unsafe legacy deletion record");
    if canonical_plugin_id(original) != id {
        return Ok(());
    }
    // A separately installed current package keeps its own data even if it reused a historical alias.
    if original != id && !installed.contains_key(original) {
        remove_owned_path(root, &root.join("data").join(original))?;
        remove_owned_path(
            root,
            &root.join("settings").join(format!("{original}.json")),
        )?;
    }
    remove_owned_path(root, &backup)
}

/// Resolve and check the exact deletion target before removing a host-owned file or directory.
fn remove_owned_path(root: &Path, path: &Path) -> anyhow::Result<()> {
    reject_links(path)?;
    if !path.exists() {
        return Ok(());
    }
    let resolved = path.canonicalize()?;
    let root = root.canonicalize()?;
    anyhow::ensure!(
        resolved != root && resolved.starts_with(root),
        "Invalid legacy deletion path"
    );
    if resolved.is_dir() {
        fs::remove_dir_all(resolved)?;
    } else {
        fs::remove_file(resolved)?;
    }
    Ok(())
}

fn legacy_snapshot(
    data: &Path,
    record: &Installed,
    environment: &Environment,
) -> anyhow::Result<Option<Snapshot>> {
    let path_for = |workspace: &str| {
        let hash = format!("{:x}", Sha256::digest(workspace.as_bytes()));
        data.join(format!("state-{}.json", &hash[..16]))
    };
    if let Some(snapshot) = read_snapshot(&path_for(&environment.workspace))? {
        return Ok(Some(snapshot));
    }
    // A removed workspace has no provable aliases; retain its backup for a later explicit reopen.
    let Ok(canonical) = Path::new(&environment.workspace).canonicalize() else {
        return Ok(None);
    };
    let mut aliases: BTreeSet<String> = record
        .project_enabled
        .iter()
        .filter(|path| {
            Path::new(path)
                .canonicalize()
                .is_ok_and(|path| path == canonical)
        })
        .cloned()
        .collect();
    aliases.insert(canonical.display().to_string());
    // Windows extended and ordinary absolute forms identify the same confirmed workspace.
    if let Some(ordinary) = canonical.to_string_lossy().strip_prefix(r"\\?\") {
        aliases.insert(ordinary.to_owned());
    }
    let mut selected = None;
    for alias in aliases {
        if let Some(snapshot) = read_snapshot(&path_for(&alias))? {
            if let Some(previous) = &selected {
                anyhow::ensure!(
                    serde_json::to_vec(previous)? == serde_json::to_vec(&snapshot)?,
                    "Multiple legacy snapshots match this workspace; original backups retained"
                );
            }
            selected = Some(snapshot);
        }
    }
    Ok(selected)
}

fn read_snapshot(path: &Path) -> anyhow::Result<Option<Snapshot>> {
    if !path.exists() {
        return Ok(None);
    }
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata.len() <= 40 * 1024 * 1024,
        "Invalid or oversized legacy snapshot"
    );
    Ok(Some(serde_json::from_slice(&fs::read(path)?)?))
}

fn legacy_directory(root: &Path, id: &str) -> PathBuf {
    root.join("legacy-backup/v7/plugins").join(id)
}

/// Check every existing ancestor rather than following an imported path through a filesystem link.
fn plain_directory(path: &Path) -> anyhow::Result<()> {
    reject_links(path)?;
    fs::create_dir_all(path)?;
    Ok(())
}

/// Imported files and their parents must remain ordinary paths under the host-owned installation root.
pub(super) fn reject_links(path: &Path) -> anyhow::Result<()> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => anyhow::ensure!(
                !metadata.file_type().is_symlink(),
                "Linked legacy backup path"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
