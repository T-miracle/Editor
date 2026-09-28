//! Loads installed declarative resources through the same registry as WASM plugins.

use plugin_runtime::{Installed, Manager};
use plugin_schema::{
    FileIconConfig, LanguageContribution, PluginManifest, ThemeDefinition, ThemeFile, ThemeMode,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, RwLock},
};

/// A snapshot lets render and document paths read plugin metadata without disk I/O.
#[derive(Default)]
struct Catalog {
    plugins: BTreeMap<String, Contribution>,
    icon_rules: Arc<Vec<(String, FileIconConfig, bool)>>,
}

struct Contribution {
    root: PathBuf,
    digest: String,
    manifest: PluginManifest,
    icons: Option<FileIconConfig>,
    theme_icons: Option<FileIconConfig>,
    theme: Option<ThemeFile>,
    assets: BTreeMap<String, Vec<u8>>,
}

static CATALOG: LazyLock<RwLock<Arc<Catalog>>> =
    LazyLock::new(|| RwLock::new(Arc::new(Catalog::default())));

/// Rebuild from enabled packages after a lifecycle change; failed assets stay unavailable.
pub fn refresh(root: &Path) -> anyhow::Result<()> {
    let installed = Manager::read_registry(root)?
        .into_values()
        .collect::<Vec<_>>();
    refresh_entries(root, &installed);
    Ok(())
}

/// Restore contributions enabled only for the active workspace before tabs load.
pub fn refresh_for_workspace(root: &Path, workspace: &Path) -> anyhow::Result<()> {
    let key = workspace.display().to_string();
    let installed = Manager::read_registry(root)?
        .into_values()
        .map(|mut entry| {
            entry.enabled |= entry.project_enabled.contains(&key);
            entry
        })
        .collect::<Vec<_>>();
    refresh_entries(root, &installed);
    Ok(())
}

/// The worker's live error state prevents failed components from contributing assets.
pub fn refresh_entries(root: &Path, installed: &[Installed]) {
    let mut catalog = Catalog::default();
    for installed in installed {
        let id = &installed.manifest.id;
        if !installed.enabled
            || installed.error.is_some()
            || !id.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-'
            })
            || installed.digest.len() != 64
            || !installed
                .digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            continue;
        }
        let Some(path) = &installed.manifest.contributions else {
            continue;
        };
        let version = root.join("packages").join(&id).join(&installed.digest);
        match Contribution::read(&version, path, id, &installed.digest) {
            Ok(contribution) => {
                catalog.plugins.insert(id.clone(), contribution);
            }
            Err(error) => {
                tracing::warn!(plugin = %id, %error, "declarative plugin assets unavailable")
            }
        }
    }
    // Keep icon mappings immutable so drawing each explorer row only clones an Arc.
    let mut rules = Vec::new();
    for (id, entry) in &catalog.plugins {
        if let Some(config) = &entry.theme_icons {
            rules.push((asset_prefix(id, &entry.digest), config.clone(), true));
        }
    }
    for (id, entry) in &catalog.plugins {
        if let Some(config) = &entry.icons {
            rules.push((asset_prefix(id, &entry.digest), config.clone(), false));
        }
    }
    catalog.icon_rules = Arc::new(rules);
    *CATALOG.write().unwrap() = Arc::new(catalog);
}

impl Contribution {
    /// Only canonical paths inside the immutable package version may be read.
    fn read(root: &Path, manifest_path: &str, id: &str, digest: &str) -> anyhow::Result<Self> {
        let root = root.canonicalize()?;
        let source = read_asset(&root, Path::new(manifest_path))?;
        let manifest = PluginManifest::parse(std::str::from_utf8(&source)?)?;
        anyhow::ensure!(manifest.plugin.id == id, "Contribution identity mismatch");
        let icons = manifest
            .file_icons
            .as_ref()
            .map(|path| read_icons(&root, path))
            .transpose()?;
        let theme_icons = manifest
            .theme
            .as_ref()
            .and_then(|theme| theme.file_icons.as_ref())
            .map(|path| read_icons(&root, path))
            .transpose()?;
        let theme = manifest
            .theme
            .as_ref()
            .map(|theme| -> anyhow::Result<_> {
                Ok(ThemeFile::parse(std::str::from_utf8(&read_asset(
                    &root,
                    &theme.file,
                )?)?)?)
            })
            .transpose()?;
        let mut assets = BTreeMap::new();
        for config in [&icons, &theme_icons].into_iter().flatten() {
            for icon in &config.icons {
                for path in [&icon.light, &icon.dark] {
                    let bytes = read_asset(&root, path)?;
                    let svg = std::str::from_utf8(&bytes)?;
                    let lower = svg.to_ascii_lowercase();
                    anyhow::ensure!(
                        bytes.len() <= 64 * 1024
                            && svg.trim_start().starts_with("<svg")
                            && !lower.contains("<!doctype")
                            && !lower.contains("<script")
                            && !lower.contains("href="),
                        "Invalid SVG asset"
                    );
                    assets.insert(path.to_string_lossy().replace('\\', "/"), bytes);
                }
            }
        }
        Ok(Self {
            root,
            digest: digest.to_owned(),
            manifest,
            icons,
            theme_icons,
            theme,
            assets,
        })
    }
}

fn read_icons(root: &Path, path: &Path) -> anyhow::Result<FileIconConfig> {
    Ok(FileIconConfig::parse(std::str::from_utf8(&read_asset(
        root, path,
    )?)?)?)
}

fn read_asset(root: &Path, path: &Path) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        path.components()
            .all(|part| matches!(part, std::path::Component::Normal(_))),
        "Unsafe contribution asset"
    );
    let resolved = root.join(path).canonicalize()?;
    anyhow::ensure!(
        resolved.starts_with(root) && resolved.is_file(),
        "Contribution asset escapes package"
    );
    Ok(std::fs::read(resolved)?)
}

/// Return the owning package and configuration for a matching source extension.
pub fn language_for_path(path: &Path) -> Option<(String, LanguageContribution)> {
    let extension = path.extension()?.to_str()?;
    let catalog = CATALOG.read().unwrap().clone();
    catalog.plugins.iter().find_map(|(id, contribution)| {
        contribution
            .manifest
            .languages
            .iter()
            .find(|language| {
                language
                    .extensions
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(extension))
            })
            .map(|language| (id.clone(), language.clone()))
    })
}

/// Locate an enabled package so its grammar can be validated off the UI thread.
pub fn plugin_root(id: &str) -> Option<PathBuf> {
    CATALOG
        .read()
        .unwrap()
        .plugins
        .get(id)
        .map(|entry| entry.root.clone())
}

/// Installed theme packages override the built-in safety palette for their declared mode.
pub fn theme(dark: bool) -> Option<ThemeDefinition> {
    let mode = if dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    let catalog = CATALOG.read().unwrap().clone();
    catalog
        .plugins
        .values()
        .filter_map(|entry| entry.theme.as_ref())
        .flat_map(|file| &file.themes)
        .find(|theme| theme.mode == mode)
        .cloned()
}

/// Theme icon rules precede language rules, preserving the existing file-tree priority.
pub fn icon_rules() -> Arc<Vec<(String, FileIconConfig, bool)>> {
    let catalog = CATALOG.read().unwrap().clone();
    catalog.icon_rules.clone()
}

fn asset_prefix(id: &str, digest: &str) -> String {
    format!("plugins/{id}/{digest}")
}

/// Resolve a renderer asset only from an enabled installed plugin version.
pub fn asset(path: &str) -> Option<Vec<u8>> {
    let catalog = CATALOG.read().unwrap().clone();
    catalog.plugins.iter().find_map(|(id, entry)| {
        path.strip_prefix(&format!("{}/", asset_prefix(id, &entry.digest)))
            .and_then(|relative| entry.assets.get(relative).cloned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Source assets use the same validated directory format copied into each ZIP.
    #[test]
    fn declarative_language_packages_expose_their_distinct_resources() {
        let plugins = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
        let digest = "a".repeat(64);
        let rust =
            Contribution::read(&plugins.join("rust"), "plugin.toml", "me.rust", &digest).unwrap();
        assert_eq!(rust.manifest.languages[0].id, "rust");
        assert!(!rust.assets.is_empty());
        let toml =
            Contribution::read(&plugins.join("toml"), "plugin.toml", "me.toml", &digest).unwrap();
        assert_eq!(toml.manifest.languages[0].id, "toml");
        assert!(rust.theme.is_none() && toml.theme.is_none());
    }
}
