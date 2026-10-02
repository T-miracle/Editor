//! Read local ZIP packages into bounded memory before any installation side effect.
use plugin_protocol::Manifest;
use plugin_schema::{FileIconConfig, PluginManifest, ThemeFile};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
    path::Path,
};

/// LSP declarations carry exactly the native service authority that the host will exercise.
fn validate_language_services(manifest: &Manifest) -> anyhow::Result<()> {
    use plugin_protocol::{api::InstanceScope, settings::SettingType};
    let mut ids = std::collections::BTreeSet::new();
    anyhow::ensure!(
        manifest.language_servers.len() <= 64,
        "Too many LSP providers"
    );
    for provider in &manifest.language_servers {
        anyhow::ensure!(
            manifest.protocol == 7
                && manifest.scope == InstanceScope::Workspace
                && manifest
                    .api
                    .as_ref()
                    .is_some_and(|api| api.required.contains_key("language.lsp")
                        && api.required.contains_key("process"))
                && provider.valid()
                && ids.insert(&provider.id),
            "Invalid LSP provider declaration"
        );
        anyhow::ensure!(
            !provider.hook || manifest.component.is_some(),
            "LSP hooks require WASM"
        );
        for service in std::iter::once(&provider.service).chain(&provider.alternatives) {
            anyhow::ensure!(
                manifest.services.contains_key(service)
                    && manifest
                        .permissions
                        .contains(&format!("process.service.{service}")),
                "LSP service must be declared and granted"
            );
        }
        if let Some(key) = &provider.executable_setting {
            anyhow::ensure!(
                manifest
                    .settings
                    .get(key)
                    .is_some_and(|definition| matches!(
                        definition.value_type,
                        SettingType::String { .. }
                    )),
                "LSP executable setting must declare a string"
            );
        }
    }
    Ok(())
}
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
        let mut manifest: Manifest = serde_json::from_slice(
            files
                .get("manifest.json")
                .ok_or_else(|| anyhow::anyhow!("Missing manifest.json"))?,
        )?;
        // Older install files enter the same canonical identity as current market packages.
        manifest.id = plugin_schema::canonical_plugin_id(&manifest.id).to_owned();
        if manifest.id == "svg" {
            manifest.name = "SVG".into();
        }
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
            matches!(manifest.protocol, 1..=7),
            "Unsupported plugin protocol"
        );
        super::capabilities::negotiate(&manifest)?;
        validate_language_services(&manifest)?;
        if manifest.permissions.contains("dependencies.install") {
            anyhow::ensure!(
                manifest.permissions.contains("dependencies.prepare"),
                "Installer permission requires dependency preparation"
            );
        }
        if manifest.permissions.contains("dependencies.prepare") {
            anyhow::ensure!(
                manifest.protocol == 7
                    && manifest
                        .api
                        .as_ref()
                        .is_some_and(|api| api.required.contains_key("dependencies")),
                "Dependency preparation requires dependencies capability"
            );
        }
        for service in manifest.services.values() {
            if let Some(plan) = &service.installation {
                anyhow::ensure!(
                    manifest.permissions.contains("dependencies.prepare"),
                    "Dependency preparation permission required"
                );
                crate::dependencies::validate(plan)?;
                for artifact in &plan.artifacts {
                    anyhow::ensure!(
                        artifact.installer.is_none()
                            || manifest.permissions.contains("dependencies.install"),
                        "Installer permission required"
                    );
                    if let plugin_protocol::dependencies::Source::Package { path } =
                        &artifact.source
                    {
                        package_bytes(&files, path)?;
                    }
                }
            }
        }
        // Service keys are authority scopes, and definitions never accept dynamic command templates.
        anyhow::ensure!(manifest.services.len() <= 32, "Too many native services");
        for (id, service) in &manifest.services {
            anyhow::ensure!(
                manifest.protocol == 7
                    && !id.is_empty()
                    && id.len() <= 100
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                    && service.valid(),
                "Invalid native service declaration"
            );
        }
        // Reject malformed forms before installation, not when a settings window tries to render them.
        anyhow::ensure!(manifest.settings.len() <= 64, "Too many settings");
        anyhow::ensure!(
            manifest
                .settings
                .iter()
                .all(|(key, definition)| !key.is_empty()
                    && key.len() <= 64
                    && key
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
                    && definition.valid()),
            "Invalid settings declaration"
        );
        if !manifest.settings.is_empty() || manifest.settings_hook {
            anyhow::ensure!(
                manifest.protocol == 7,
                "Settings require the capability protocol"
            );
            anyhow::ensure!(
                !manifest.settings_hook || manifest.component.is_some(),
                "Settings hooks require WASM"
            );
            if manifest.component.is_some() {
                anyhow::ensure!(
                    manifest
                        .api
                        .as_ref()
                        .is_some_and(|api| api.required.contains_key("configuration")),
                    "Settings require configuration capability"
                );
            }
        }
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
                    && manifest.permissions.iter().all(|permission| {
                        if matches!(
                            permission.as_str(),
                            "dependencies.prepare" | "dependencies.install"
                        ) {
                            return true;
                        }
                        permission
                            .strip_prefix("process.service.")
                            .is_some_and(|service| {
                                manifest.language_servers.iter().any(|provider| {
                                    provider.service == service
                                        || provider.alternatives.iter().any(|id| id == service)
                                })
                            })
                    }),
                "Declarative packages cannot request executable capabilities"
            );
        }
        if let Some(path) = &manifest.contributions {
            // Validate every declarative reference before installation can mutate the registry.
            validate_relative(path)?;
            let source = package_text(&files, path)?;
            let contributions = PluginManifest::parse(source)?;
            if !contributions.language_definitions.is_empty()
                || !contributions.highlighters.is_empty()
            {
                anyhow::ensure!(
                    manifest.protocol == 7,
                    "Dynamic languages require capability protocol"
                );
            }
            for provider in &contributions.highlighters {
                let grammar = package_bytes(&files, &provider.grammar.to_string_lossy())?;
                anyhow::ensure!(grammar.starts_with(b"\0asm"), "Invalid grammar module");
                package_text(&files, &provider.highlights.to_string_lossy())?;
            }
            anyhow::ensure!(
                plugin_schema::canonical_plugin_id(&contributions.plugin.id) == manifest.id,
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
                .contains(&permission.as_str())
                    || (manifest.protocol == 7
                        && ([
                            "assets.read",
                            "editor.read",
                            "editor.write",
                            "ui.panels",
                            "process.exec",
                            "dependencies.prepare",
                            "dependencies.install"
                        ]
                        .contains(&permission.as_str())
                            || permission
                                .strip_prefix("process.service.")
                                .is_some_and(|id| manifest.services.contains_key(id)))),
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
                ["left", "right", "bottom", "editor"].contains(&panel.position.as_str()),
                "Unsupported dock position"
            );
            // Older hosts do not know how to scope a preview to the current in-memory document.
            anyhow::ensure!(
                if panel.position == "editor" {
                    manifest.protocol >= 6
                        && if manifest.protocol == 7 {
                            manifest.permissions.contains("editor.read")
                                && manifest.scope == plugin_protocol::api::InstanceScope::Workspace
                                && manifest.api.as_ref().is_some_and(|api| {
                                    api.required.contains_key("editor.documents")
                                })
                        } else {
                            manifest.permissions.contains("editor.commands")
                        }
                        && !panel.file_extensions.is_empty()
                        && panel.file_extensions.len() <= 32
                        && panel.file_extensions.iter().all(|extension| {
                            !extension.is_empty()
                                && extension.len() <= 32
                                && extension.bytes().all(|byte| {
                                    byte.is_ascii_alphanumeric()
                                        || matches!(byte, b'_' | b'-' | b'+')
                                })
                        })
                } else {
                    panel.file_extensions.is_empty()
                },
                "Editor previews require document read authority and valid file extensions"
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
pub(crate) fn validate_relative(name: &str) -> anyhow::Result<()> {
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

    /// New preview packages use protocol 6 while all earlier packages remain installable.
    #[test]
    fn package_accepts_protocol_five_and_preserves_version_boundary() {
        for (protocol, supported) in [(3, true), (4, true), (5, true), (6, true), (7, false)] {
            let manifest = serde_json::json!({
                "id": "test", "name": "Test", "version": "0.1.0",
                "protocol": protocol, "component": "test.wasm",
                "permissions": [], "storage_limit": 1024
            });
            let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
            zip.start_file("manifest.json", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(&serde_json::to_vec(&manifest).unwrap())
                .unwrap();
            zip.start_file("test.wasm", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"\0asm\x0d\0\x01\0").unwrap();
            let bytes = zip.finish().unwrap().into_inner();
            assert_eq!(Package::from_bytes(&bytes).is_ok(), supported);
        }
    }

    /// The host must reject an empty archive instead of installing an inert registry entry.
    #[test]
    fn package_requires_component_or_declarative_contributions() {
        let manifest = serde_json::json!({
            "id": "empty", "name": "Empty", "version": "0.1.0", "protocol": 1,
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
