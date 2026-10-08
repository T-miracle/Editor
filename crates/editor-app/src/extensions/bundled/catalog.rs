//! A bounded shipped index selects one ZIP lazily; project settings never configure production roots.
use super::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::Read;

const INDEX_BYTES: usize = 1024 * 1024;
const PACKAGE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    version: u32,
    packages: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    file: String,
    sha256: String,
    file_extensions: Vec<String>,
}

/// Tests have one isolated fixture location; production prefers the executable's shipped resources.
fn roots(workspace: &Path) -> Vec<PathBuf> {
    #[cfg(test)]
    {
        vec![workspace.join(".bundled-plugin-test")]
    }
    #[cfg(not(test))]
    {
        let _ = workspace;
        crate::app::distribution::shipped_plugin_roots()
    }
}

/// A non-file, redirected child or oversized read is an error, never permission to try a weaker source.
fn read(root: &Path, name: &str, limit: usize) -> anyhow::Result<Vec<u8>> {
    let path = root.join(name);
    let metadata = std::fs::symlink_metadata(&path)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "Invalid bundled resource"
    );
    anyhow::ensure!(
        metadata.len() <= limit as u64,
        "Bundled resource exceeds quota"
    );
    let canonical = path.canonicalize()?;
    anyhow::ensure!(
        canonical.starts_with(root),
        "Bundled resource escapes its directory"
    );
    let mut bytes = Vec::new();
    std::fs::File::open(canonical)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= limit, "Bundled resource exceeds quota");
    Ok(bytes)
}

/// Admit a single portable filename, excluding device names, alternate streams and traversal spelling.
fn filename(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    !name.is_empty()
        && name.len() <= 128
        && name.ends_with(".zip")
        && !name.starts_with('.')
        && !name.contains(['/', '\\', ':', '*', '?', '<', '>', '|'])
        && !name.chars().any(char::is_control)
        && !name.ends_with(['.', ' '])
        && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit())
}

/// Validate the entire bounded index before selecting; duplicate extension owners are ambiguous, not ordered.
fn validate(catalog: &Catalog) -> anyhow::Result<()> {
    anyhow::ensure!(
        catalog.version == 1 && catalog.packages.len() <= 64,
        "Invalid bundled index version or quota"
    );
    let mut extensions = std::collections::BTreeSet::new();
    let mut files = std::collections::BTreeSet::new();
    for entry in &catalog.packages {
        anyhow::ensure!(
            filename(&entry.file)
                && files.insert(&entry.file)
                && entry.sha256.len() == 64
                && entry.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
                && !entry.file_extensions.is_empty()
                && entry.file_extensions.len() <= 128,
            "Invalid bundled package declaration"
        );
        for extension in &entry.file_extensions {
            anyhow::ensure!(
                !extension.is_empty()
                    && extension.len() <= 128
                    && extension
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
                    && extensions.insert(extension.to_ascii_lowercase()),
                "Invalid or ambiguous bundled extension"
            );
        }
    }
    Ok(())
}

/// Only a matching active file triggers ZIP decoding; its immutable bytes are checked before package parsing.
pub(super) fn matching(request: &Request) -> anyhow::Result<Option<(Package, Vec<String>)>> {
    let Some(extension) = request.file.extension().and_then(|value| value.to_str()) else {
        return Ok(None);
    };
    for root in roots(&request.workspace) {
        let index = root.join("bundle-defaults.json");
        match std::fs::symlink_metadata(&index) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        let root = root.canonicalize()?;
        let catalog: Catalog =
            serde_json::from_slice(&read(&root, "bundle-defaults.json", INDEX_BYTES)?)?;
        validate(&catalog)?;
        let Some(entry) = catalog.packages.into_iter().find(|entry| {
            entry
                .file_extensions
                .iter()
                .any(|value| value.eq_ignore_ascii_case(extension))
        }) else {
            return Ok(None);
        };
        if !request.is_active() {
            return Ok(None);
        }
        let bytes = read(&root, &entry.file, PACKAGE_BYTES)?;
        let digest = format!("{:x}", Sha256::digest(&bytes));
        anyhow::ensure!(
            digest.eq_ignore_ascii_case(&entry.sha256),
            "Bundled package hash mismatch"
        );
        let mut package = Package::from_bytes(&bytes)?;
        package.source = Some(root.join(entry.file).display().to_string());
        return Ok(Some((package, entry.file_extensions)));
    }
    Ok(None)
}

/// Installed language declarations are read from their canonical immutable version, under the same quota.
pub(super) fn contribution(
    manager: &plugin_runtime::Manager,
    entry: &Installed,
) -> anyhow::Result<Option<plugin_schema::PluginManifest>> {
    let Some(name) = &entry.manifest.contributions else {
        return Ok(None);
    };
    let root = manager
        .root()
        .join("packages")
        .join(&entry.manifest.id)
        .join(&entry.digest)
        .canonicalize()?;
    let packages = manager.root().join("packages").canonicalize()?;
    anyhow::ensure!(
        root.starts_with(&packages),
        "Installed bundle contribution escapes its package store"
    );
    anyhow::ensure!(
        !name.is_empty()
            && !name.contains(['\\', ':'])
            && Path::new(name)
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_))),
        "Unsafe installed contribution path"
    );
    let bytes = read(&root, name, INDEX_BYTES)?;
    let contribution = plugin_schema::PluginManifest::parse(std::str::from_utf8(&bytes)?)?;
    anyhow::ensure!(
        contribution.plugin.id == entry.manifest.id,
        "Installed contribution identity mismatch"
    );
    Ok(Some(contribution))
}
