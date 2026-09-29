//! Read local ZIP packages into bounded memory before any installation side effect.
use plugin_protocol::Manifest;
use plugin_schema::{FileIconConfig, PluginManifest, ThemeFile};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
    path::Path,
};
#[derive(Clone)]
pub struct Package {
    pub manifest: Manifest,
    pub files: BTreeMap<String, Vec<u8>>,
    pub digest: String,
    /// Display the exact local package source in the consent UI; it grants no trust by itself.
    pub source: Option<String>,
}
impl Package {
    /// A package is a ZIP with a manifest and either executable WASM or declarative assets.
    pub fn read(path: &Path) -> anyhow::Result<Self> {
        anyhow::ensure!(
            path.metadata()?.len() <= 64 * 1024 * 1024,
            "Package exceeds 64 MiB"
        );
        let mut package = Self::from_bytes(&std::fs::read(path)?)?;
        package.source = Some(path.display().to_string());
        Ok(package)
    }
    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(bytes.len() <= 64 * 1024 * 1024, "Package too large");
        let digest = format!("{:x}", Sha256::digest(bytes));
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes))?;
        anyhow::ensure!(zip.len() <= 512, "Too many package entries");
        let mut files = BTreeMap::new();
        let mut total = 0;
        let mut actual_total = 0usize;
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index)?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().to_owned();
            validate_relative(&name)?;
            anyhow::ensure!(!entry.is_symlink(), "Package symlinks are forbidden");
            total += entry.size();
            anyhow::ensure!(total <= 128 * 1024 * 1024, "Expanded package exceeds quota");
            let mut data = Vec::new();
            let remaining = 128 * 1024 * 1024 - actual_total;
            entry
                .by_ref()
                .take(remaining as u64 + 1)
                .read_to_end(&mut data)?;
            anyhow::ensure!(
                data.len() <= remaining,
                "Actual expanded package exceeds quota"
            );
            actual_total += data.len();
            anyhow::ensure!(files.insert(name, data).is_none(), "Duplicate package path");
        }
        let manifest: Manifest = serde_json::from_slice(
            files
                .get("manifest.json")
                .ok_or_else(|| anyhow::anyhow!("Missing manifest.json"))?,
        )?;
        anyhow::ensure!(
            manifest.id.len() <= 100
                && !manifest.id.is_empty()
                && manifest.id.bytes().all(|b| b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || b == b'.'
                    || b == b'-')
                && !manifest.id.starts_with('.'),
            "Invalid plugin identity"
        );
        semver::Version::parse(&manifest.version)?;
        anyhow::ensure!(
            matches!(manifest.protocol, 1..=3),
            "Unsupported plugin protocol"
        );
        anyhow::ensure!(
            (1024..=32 * 1024 * 1024).contains(&manifest.storage_limit),
            "Invalid storage quota"
        );
        anyhow::ensure!(
            manifest.component.is_some() || manifest.contributions.is_some(),
            "Package declares neither a component nor contributions"
        );
        if let Some(path) = &manifest.component {
            validate_relative(path)?;
            let wasm = files
                .get(path)
                .ok_or_else(|| anyhow::anyhow!("Missing component"))?;
            anyhow::ensure!(
                wasm.starts_with(b"\0asm\x0d\0\x01\0"),
                "Expected a WebAssembly component, not a core module"
            );
        } else {
            // Host-managed packages cannot declare executable UI or privileged operations.
            anyhow::ensure!(
                manifest.panels.is_empty()
                    && manifest.commands.is_empty()
                    && manifest.permissions.is_empty(),
                "Declarative packages cannot request executable capabilities"
            );
        }
        if let Some(path) = &manifest.contributions {
            // Validate every declarative reference before installation can mutate the registry.
            validate_relative(path)?;
            let source = package_text(&files, path)?;
            let contributions = PluginManifest::parse(source)?;
            anyhow::ensure!(
                contributions.plugin.id == manifest.id,
                "Contribution manifest identity differs from package identity"
            );
            anyhow::ensure!(
                contributions.plugin.version == manifest.version,
                "Contribution manifest version differs from package version"
            );
            for language in &contributions.languages {
                let grammar = package_bytes(&files, &language.grammar.to_string_lossy())?;
                anyhow::ensure!(grammar.starts_with(b"\0asm"), "Invalid grammar module");
                package_text(&files, &language.highlights.to_string_lossy())?;
            }
            if let Some(theme) = &contributions.theme {
                ThemeFile::parse(package_text(&files, &theme.file.to_string_lossy())?)?;
                if let Some(path) = &theme.file_icons {
                    validate_file_icons(&files, path)?;
                }
            }
            if let Some(path) = &contributions.file_icons {
                validate_file_icons(&files, path)?;
            }
        }
        for permission in &manifest.permissions {
            anyhow::ensure!(
                [
                    "process.pty",
                    "workspace.read",
                    "storage",
                    "clipboard",
                    "editor.commands"
                ]
                .contains(&permission.as_str()),
                "Unsupported capability {permission}"
            );
        }
        anyhow::ensure!(
            manifest.panels.len() <= 8 && manifest.commands.len() <= 64,
            "Contribution quota exceeded"
        );
        let mut ids = std::collections::BTreeSet::new();
        for panel in &manifest.panels {
            anyhow::ensure!(
                !panel.id.is_empty()
                    && panel.id.len() <= 100
                    && !panel.id.contains('/')
                    && ids.insert(panel.id.clone()),
                "Invalid or duplicate panel ID"
            );
            anyhow::ensure!(
                ["left", "right", "bottom"].contains(&panel.position.as_str()),
                "Unsupported dock position"
            );
            // Panel artwork must come from this package and remain small enough for native UI.
            for icon in [&panel.icon_light, &panel.icon_dark].into_iter().flatten() {
                validate_relative(icon)?;
                let svg = files
                    .get(icon)
                    .ok_or_else(|| anyhow::anyhow!("Missing panel icon: {icon}"))?;
                anyhow::ensure!(
                    svg.len() <= 64 * 1024
                        && std::str::from_utf8(svg)?.trim_start().starts_with("<svg"),
                    "Panel icon must be an SVG under 64 KiB"
                );
                let source = std::str::from_utf8(svg)?.to_ascii_lowercase();
                anyhow::ensure!(
                    !source.contains("<!doctype")
                        && !source.contains("<script")
                        && !source.contains("href="),
                    "Panel icon cannot load external content or scripts"
                );
            }
        }
        ids.clear();
        for command in &manifest.commands {
            anyhow::ensure!(
                !command.id.is_empty() && command.id.len() <= 128 && ids.insert(command.id.clone()),
                "Invalid or duplicate command ID"
            );
        }
        Ok(Self {
            manifest,
            files,
            digest,
            source: None,
        })
    }
    /// Executable packages expose their component; declarative packages have no guest.
    pub fn component(&self) -> Option<&[u8]> {
        self.manifest
            .component
            .as_ref()
            .and_then(|path| self.files.get(path).map(Vec::as_slice))
    }
    /// Materialize only validated files under a fresh, content-addressed version directory.
    pub(crate) fn extract(&self, path: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(path)?;
        for (name, bytes) in &self.files {
            let target = path.join(name);
            // A content-addressed version is immutable, including during a failed update.
            if target.exists() {
                anyhow::ensure!(
                    std::fs::read(&target)? == *bytes,
                    "Installed version content differs from package"
                );
                continue;
            }
            std::fs::create_dir_all(target.parent().unwrap())?;
            atomic_write(&target, bytes)?;
        }
        Ok(())
    }
}

/// Read only a named package file after checking its path and UTF-8 encoding.
fn package_text<'a>(files: &'a BTreeMap<String, Vec<u8>>, path: &str) -> anyhow::Result<&'a str> {
    std::str::from_utf8(package_bytes(files, path)?).map_err(Into::into)
}

/// Require all referenced resources to be present inside this ZIP archive.
fn package_bytes<'a>(files: &'a BTreeMap<String, Vec<u8>>, path: &str) -> anyhow::Result<&'a [u8]> {
    validate_relative(path)?;
    files
        .get(path)
        .map(Vec::as_slice)
        .ok_or_else(|| anyhow::anyhow!("Missing package asset: {path}"))
}

/// Check both the icon mapping and its referenced SVG files during package inspection.
fn validate_file_icons(files: &BTreeMap<String, Vec<u8>>, path: &Path) -> anyhow::Result<()> {
    let icons = FileIconConfig::parse(package_text(files, &path.to_string_lossy())?)?;
    for icon in icons.icons {
        for path in [&icon.light, &icon.dark] {
            let bytes = package_bytes(files, &path.to_string_lossy())?;
            let svg = std::str::from_utf8(bytes)?;
            let lower = svg.to_ascii_lowercase();
            anyhow::ensure!(
                bytes.len() <= 64 * 1024
                    && svg.trim_start().starts_with("<svg")
                    && !lower.contains("<!doctype")
                    && !lower.contains("<script")
                    && !lower.contains("href="),
                "File icon must be a safe SVG under 64 KiB"
            );
        }
    }
    Ok(())
}
fn validate_relative(name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !name.is_empty()
            && !name.contains(['\\', ':'])
            && Path::new(name)
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_))),
        "Unsafe package path: {name}"
    );
    Ok(())
}
/// Replace atomically in the same directory; a failed write leaves the prior file intact.
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Missing parent"))?;
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The host must reject an empty archive instead of installing an inert registry entry.
    #[test]
    fn package_requires_component_or_declarative_contributions() {
        let manifest = serde_json::json!({
            "id": "me.empty", "name": "Empty", "version": "0.1.0", "protocol": 1,
            "permissions": [], "storage_limit": 1024
        });
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("manifest.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&serde_json::to_vec(&manifest).unwrap())
            .unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        assert!(
            Package::from_bytes(&bytes)
                .err()
                .unwrap()
                .to_string()
                .contains("neither a component nor contributions")
        );
    }
    /// Packages are untrusted even when local: extraction never accepts relative traversal.
    #[test]
    fn rejects_unsafe_zip_paths() {
        for name in ["../escape", "/absolute", "C:/device", "a\\b", "./hidden"] {
            let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"data").unwrap();
            let bytes = zip.finish().unwrap().into_inner();
            assert!(Package::from_bytes(&bytes).is_err(), "accepted {name}");
        }
    }
    /// Failed replacement cannot truncate the last valid state file.
    #[test]
    fn state_replacement_is_complete() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("snapshot.json");
        atomic_write(&path, b"old").unwrap();
        atomic_write(&path, b"new valid data").unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"new valid data");
    }
}
