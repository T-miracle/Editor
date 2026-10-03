//! Preserve installed code, private data and grants when moving to prefix-free plugin identities.

use crate::{Installed, package::atomic_write};
use plugin_schema::canonical_plugin_id;
use std::{collections::BTreeMap, fs, path::Path};
mod installed;
pub(crate) use installed::{
    discard_legacy_data, needs_legacy_import, scope_snapshot, stage_legacy_data,
};

/// Copy before committing registry changes; original folders and a registry backup stay recoverable.
pub(crate) fn migrate_registry(
    root: &Path,
    installed: BTreeMap<String, Installed>,
) -> anyhow::Result<BTreeMap<String, Installed>> {
    installed::backup_legacy(root, &installed)?;
    if installed
        .iter()
        .all(|(id, entry)| historical_id(id, entry) == id)
    {
        return Ok(installed);
    }
    // An already installed canonical package wins a collision, including its grants and settings.
    let mut current = installed
        .iter()
        .filter(|(id, entry)| historical_id(id, entry) == id.as_str())
        .map(|(id, entry)| (id.clone(), entry.clone()))
        .collect::<BTreeMap<_, _>>();
    for (old_id, mut entry) in installed {
        let id = historical_id(&old_id, &entry).to_owned();
        if id == old_id || current.contains_key(&id) {
            continue;
        }
        // Registry IDs are path components; migration must never interpret arbitrary directory paths.
        anyhow::ensure!(
            safe_id(&old_id) && safe_id(&id),
            "Unsafe plugin identity in registry"
        );
        anyhow::ensure!(entry.manifest.id == old_id, "Registry identity mismatch");
        for directory in ["packages", "data"] {
            copy_missing(
                &root.join(directory).join(&old_id),
                &root.join(directory).join(&id),
            )?;
        }
        // Host configuration is a separate namespace from opaque data/settings.json owned by the guest.
        copy_missing(
            &root.join("settings").join(format!("{old_id}.json")),
            &root.join("settings").join(format!("{id}.json")),
        )?;
        entry.manifest.id = id.clone();
        if id == "svg" {
            entry.manifest.name = "SVG".into();
            for panel in &mut entry.manifest.panels {
                if panel.id == "preview" {
                    panel.title = "SVG".into();
                }
            }
        }
        current.insert(id, entry);
    }
    let registry = root.join("registry.json");
    let backup = root.join("registry.before-plugin-id-rename.json");
    if !backup.exists() {
        atomic_write(&backup, &fs::read(&registry)?)?;
    }
    atomic_write(&registry, &serde_json::to_vec_pretty(&current)?)?;
    Ok(current)
}

/// Only retired installation records participate in the bounded historical rename, never current packages.
fn historical_id<'a>(id: &'a str, entry: &Installed) -> &'a str {
    if matches!(entry.manifest.protocol, 1..=6) {
        canonical_plugin_id(id)
    } else {
        id
    }
}

/// Only plain ASCII plugin identifiers may form private directory names.
fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('.')
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
        })
}

/// Idempotent copies preserve newer destination files and never follow package/data symlinks.
fn copy_missing(source: &Path, destination: &Path) -> anyhow::Result<()> {
    installed::reject_links(source)?;
    installed::reject_links(destination)?;
    if !source.exists() {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(source)?;
    anyhow::ensure!(
        !metadata.file_type().is_symlink(),
        "Plugin migration cannot follow symlinks"
    );
    if destination.exists() {
        anyhow::ensure!(
            !fs::symlink_metadata(destination)?.file_type().is_symlink(),
            "Plugin destination is a symlink"
        );
    }
    if metadata.is_dir() {
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_missing(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else if metadata.is_file() && !destination.exists() {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, destination)?;
    }
    Ok(())
}
