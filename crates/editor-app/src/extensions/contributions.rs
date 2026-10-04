//! Loads installed declarative resources through the same registry as WASM plugins.

use plugin_runtime::{Installed, Manager};
use plugin_schema::{
    DiscoveredTarget, FileIconConfig, PluginManifest, ProviderFailure, RunTargetDiscovery,
    ThemeDefinition, ThemeFile, ThemeMode,
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
    /// Discovery declarations this plugin contributes, already validated.
    run_targets: Vec<Arc<RunTargetDiscovery>>,
    assets: BTreeMap<String, Vec<u8>>,
}

static CATALOG: LazyLock<RwLock<Arc<Catalog>>> =
    LazyLock::new(|| RwLock::new(Arc::new(Catalog::default())));

/// Rebuild from enabled packages after a lifecycle change; failed assets stay unavailable.
#[cfg(test)]
pub fn refresh(root: &Path) -> anyhow::Result<()> {
    let installed = Manager::read_registry(root)?
        .into_values()
        .collect::<Vec<_>>();
    refresh_entries(root, &installed);
    Ok(())
}

/// Restore contributions enabled only for the active workspace before tabs load.
pub fn refresh_for_workspace(root: &Path, workspace: &Path) -> anyhow::Result<()> {
    crate::language::providers::configure(root, workspace);
    let key = workspace.display().to_string();
    let installed = Manager::read_registry(root)?
        .into_values()
        .map(|mut entry| {
            entry.enabled |= entry.project_enabled_in(&key);
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
            || installed.compatibility_error().is_some()
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
    crate::language::providers::refresh(
        root,
        catalog
            .plugins
            .iter()
            .map(|(id, entry)| {
                (
                    id.clone(),
                    entry.root.clone(),
                    entry.manifest.language_definitions.clone(),
                    entry.manifest.highlighters.clone(),
                )
            })
            .collect(),
        installed
            .iter()
            .filter(|entry| {
                entry.enabled && entry.error.is_none() && entry.compatibility_error().is_none()
            })
            .map(|entry| {
                (
                    entry.manifest.id.clone(),
                    entry.manifest.language_servers.clone(),
                )
            })
            .collect(),
    );
    *CATALOG.write().unwrap() = Arc::new(catalog);
}

impl Contribution {
    /// Only canonical paths inside the immutable package version may be read.
    fn read(root: &Path, manifest_path: &str, id: &str, digest: &str) -> anyhow::Result<Self> {
        let root = root.canonicalize()?;
        let source = read_asset(&root, Path::new(manifest_path))?;
        let manifest = PluginManifest::parse(std::str::from_utf8(&source)?)?;
        // Historical identity import belongs to the registry migration; current packages keep exact author IDs.
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
        let (run_targets, failures) = read_run_targets(&root, &manifest.run_targets);
        for failure in &failures {
            tracing::warn!(
                plugin = %id,
                provider = %failure.provider,
                error = %failure.error,
                "run target discovery unavailable"
            );
        }
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
            run_targets,
            assets,
        })
    }
}

/// Whether a declaration may stand for the contribution that announced it.
///
/// The declaration's own identity is what a saved configuration names, so it has to be the identity
/// the contribution file announced; otherwise the two could disagree silently and a configuration
/// would point at a provider nobody installed.
pub fn declaration_matches_contribution(
    contribution: &str,
    declaration: &str,
) -> Result<(), String> {
    if contribution == declaration {
        return Ok(());
    }
    Err(format!(
        "declaration names provider {declaration} but the contribution names {contribution}"
    ))
}

/// Read every discovery declaration a plugin contributes.
///
/// Each declaration is validated as it is read, so a plugin that ships an unusable one is reported
/// with the file that failed instead of taking the rest of its contributions down with it.
fn read_run_targets(
    root: &Path,
    contributions: &[plugin_schema::RunTargetContribution],
) -> (Vec<Arc<RunTargetDiscovery>>, Vec<ProviderFailure>) {
    let mut providers = Vec::new();
    let mut failures = Vec::new();
    // The package root is resolved once, so a declaration may only name files inside it.
    let resolved = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    for contribution in contributions {
        let provider = contribution.id.clone();
        let read = || -> anyhow::Result<RunTargetDiscovery> {
            let bytes = read_asset(&resolved, &contribution.file)?;
            Ok(RunTargetDiscovery::from_json(&bytes)?)
        };
        match read() {
            Ok(discovery) => match declaration_matches_contribution(&provider, &discovery.id) {
                Ok(()) => providers.push(Arc::new(discovery)),
                Err(error) => failures.push(ProviderFailure { provider, error }),
            },
            Err(error) => failures.push(ProviderFailure {
                provider,
                error: format!("{error:#}"),
            }),
        }
    }
    (providers, failures)
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

/// Verify packaged selector resources independently from UI rendering.
#[cfg(test)]
fn language_for_path_in_catalog(
    catalog: &Catalog,
    path: &Path,
) -> Option<(String, plugin_schema::LanguageDefinition)> {
    let filename = path.file_name()?.to_str()?;
    for (id, contribution) in &catalog.plugins {
        if let Some(language) = contribution
            .manifest
            .language_definitions
            .iter()
            .find(|language| {
                language
                    .filenames
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(filename))
            })
        {
            return Some((id.clone(), language.clone()));
        }
    }

    let extension = path.extension()?.to_str()?;
    for (id, contribution) in &catalog.plugins {
        if let Some(language) = contribution
            .manifest
            .language_definitions
            .iter()
            .find(|language| {
                language
                    .extensions
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(extension))
            })
        {
            return Some((id.clone(), language.clone()));
        }
    }
    None
}

/// Run every enabled plugin's discovery declarations against one workspace.
///
/// Each declaration is applied independently, so a provider that offers nothing — or offers
/// something unusable — cannot stop another provider from offering its own targets. The candidates
/// are gathered here and handed on; nothing in this module decides what a candidate means.
pub fn discover_run_targets(workspace: &Path) -> Vec<DiscoveredTarget> {
    let providers = CATALOG
        .read()
        .unwrap()
        .plugins
        .values()
        .flat_map(|contribution| contribution.run_targets.iter().cloned())
        .collect::<Vec<_>>();
    if providers.is_empty() {
        return Vec::new();
    }
    // The caller lists the files once, so every provider reads the same view of the project and a
    // wildcard pattern cannot reach a file the workspace never offered.
    let files = workspace_files(workspace);
    let mut targets = Vec::new();
    for provider in providers {
        targets.extend(provider.discover(&files, |path| {
            let resolved = workspace.join(path);
            // A declaration may only read regular files inside the workspace it was given.
            (resolved.starts_with(workspace) && resolved.is_file())
                .then(|| std::fs::read_to_string(resolved).ok())
                .flatten()
        }));
    }
    targets
}

/// Workspace-relative file names, bounded so a discovery walk stays a bounded read.
fn workspace_files(workspace: &Path) -> Vec<String> {
    /// Most names one discovery walk offers to the providers.
    const MAX_LISTED_FILES: usize = 20000;
    let mut files = Vec::new();
    let mut pending = vec![workspace.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            // Generated directories are never discovery inputs and would dominate the walk.
            if matches!(name.to_str(), Some(".git" | "target" | "node_modules")) {
                continue;
            }
            if path.is_dir() {
                pending.push(path);
            } else if let Ok(relative) = path.strip_prefix(workspace) {
                files.push(relative.to_string_lossy().replace('\\', "/"));
                if files.len() >= MAX_LISTED_FILES {
                    return files;
                }
            }
        }
    }
    files
}

/// Locate an enabled package so its grammar can be validated off the UI thread.
#[cfg(test)]
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

    /// The shipped Rust plugin's declaration is read and applied by the host's own code.
    ///
    /// This is the vertical seam for discovery: the package's contribution file, the declaration it
    /// names, and the candidates a real Cargo manifest produces — with no host knowledge of Rust.
    #[test]
    fn the_rust_plugin_offers_a_real_projects_targets() {
        let plugins = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
        let rust = Contribution::read(
            &plugins.join("rust"),
            "plugin.toml",
            "rust",
            &"b".repeat(64),
        )
        .unwrap_or_else(|error| panic!("the shipped rust plugin reads: {error:#}"));
        assert_eq!(rust.run_targets.len(), 1);
        assert_eq!(rust.run_targets[0].id, "rust-binary");
        assert_eq!(rust.run_targets[0].target_type, "rust-binary");

        let project = tempfile::tempdir().unwrap();
        std::fs::write(
            project.path().join("Cargo.toml"),
            "[package]\nname = \"my-app\"\nversion = \"0.4.0\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(project.path().join("src")).unwrap();
        std::fs::write(project.path().join("src/main.rs"), "fn main() {}").unwrap();
        let mut catalog = Catalog::default();
        catalog.plugins.insert("rust".to_owned(), rust);
        *CATALOG.write().unwrap() = Arc::new(catalog);
        let targets = discover_run_targets(project.path());
        assert_eq!(targets.len(), 1, "{targets:?}");
        let target = &targets[0];
        assert_eq!(target.id, "rust-binary:my-app");
        assert_eq!(target.provider, "rust-binary");
        assert_eq!(target.label, "my-app");
        assert_eq!(
            target.program, "my-app",
            "the program comes from the field the provider named"
        );
        assert_eq!(target.found_in, "Cargo.toml");
        assert_eq!(
            target.fields.get("package").map(String::as_str),
            Some("my-app")
        );
        assert_eq!(
            target.fields.get("version").map(String::as_str),
            Some("0.4.0")
        );

        // A project without a manifest simply offers nothing.
        let empty = tempfile::tempdir().unwrap();
        assert!(discover_run_targets(empty.path()).is_empty());
        // Restore the catalog so another check does not inherit this one's plugins.
        *CATALOG.write().unwrap() = Arc::new(Catalog::default());
    }

    /// A declaration that disagrees with its contribution file is reported, not silently accepted.
    #[test]
    fn a_mismatched_discovery_identity_is_refused() {
        assert!(declaration_matches_contribution("rust-binary", "rust-binary").is_ok());
        let error = declaration_matches_contribution("declared", "other")
            .expect_err("a declaration naming another provider is refused");
        assert!(
            error.contains("other") && error.contains("declared"),
            "{error}"
        );
        // A contribution whose file is absent is reported with the file it named.
        let directory = tempfile::tempdir().unwrap();
        let (providers, failures) = read_run_targets(
            directory.path(),
            &[plugin_schema::RunTargetContribution {
                id: "declared".into(),
                file: PathBuf::from("missing.json"),
            }],
        );
        assert!(providers.is_empty());
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].provider, "declared");
        // The reason comes from the platform and is not asserted word for word, but a plugin that
        // names a file it does not ship has to be reported rather than quietly offering nothing.
        assert!(!failures[0].error.trim().is_empty());
    }
    /// Startup resource loading must reject old protocols before opening their grammar/theme/icon assets.
    #[test]
    fn incompatible_resource_records_do_not_publish_contributions() {
        let directory = tempfile::tempdir().unwrap();
        let package =
            crate::extensions::language_tests::packages::language_package("me.custom-language");
        let mut manager = Manager::open(directory.path().into(), Default::default()).unwrap();
        manager.install(&package, Default::default()).unwrap();
        let mut entries = manager.published_entries();
        refresh_entries(directory.path(), &entries);
        assert!(
            plugin_root("me.custom-language").is_some(),
            "current author ID must remain exact"
        );
        entries[0].manifest.protocol = 1;
        entries[0].manifest.api = None;
        refresh_entries(directory.path(), &entries);
        assert!(plugin_root("me.custom-language").is_none());
        assert!(icon_rules().is_empty());
        assert!(
            entries[0].enabled,
            "filtering must not clear the persisted preference"
        );
    }

    /// JavaScript aliases resolve from the installed manifest and carry valid themed icons.
    #[test]
    fn javascript_package_resolves_extensions_and_icons() {
        let plugins = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
        let javascript = Contribution::read(
            &plugins.join("javascript"),
            "plugin.toml",
            "javascript",
            &"a".repeat(64),
        )
        .unwrap();
        assert_eq!(javascript.assets.len(), 2);
        let mut catalog = Catalog::default();
        catalog.plugins.insert("javascript".to_owned(), javascript);
        for path in ["main.js", "module.mjs", "legacy.cjs", "View.jsx", "MAIN.JS"] {
            let (_, language) = language_for_path_in_catalog(&catalog, Path::new(path)).unwrap();
            assert_eq!(language.id, "javascript");
            let icons = catalog.plugins["javascript"].icons.as_ref().unwrap();
            let extension = Path::new(path).extension().unwrap().to_str().unwrap();
            assert!(
                icons.icons[0]
                    .extensions
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(extension))
            );
        }
        // TypeScript and JSON retain their own language identities.
        for path in ["main.ts", "View.tsx", "package.json"] {
            assert!(language_for_path_in_catalog(&catalog, Path::new(path)).is_none());
        }
        catalog.plugins.clear();
        assert!(language_for_path_in_catalog(&catalog, Path::new("main.js")).is_none());
    }

    /// Source assets use the same validated directory format copied into each ZIP.
    #[test]
    fn declarative_language_packages_expose_their_distinct_resources() {
        let plugins = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
        let digest = "a".repeat(64);
        let rust =
            Contribution::read(&plugins.join("rust"), "plugin.toml", "rust", &digest).unwrap();
        assert_eq!(rust.manifest.language_definitions[0].id, "rust");
        assert!(!rust.assets.is_empty());
        let toml =
            Contribution::read(&plugins.join("toml"), "plugin.toml", "toml", &digest).unwrap();
        assert_eq!(toml.manifest.language_definitions[0].id, "toml");
        assert!(rust.theme.is_none() && toml.theme.is_none());

        // Both HTML suffixes resolve to one language and ship valid theme-specific icons.
        let html =
            Contribution::read(&plugins.join("html"), "plugin.toml", "html", &digest).unwrap();
        assert_eq!(html.assets.len(), 2);
        assert_eq!(
            html.icons.as_ref().unwrap().icons[0].extensions,
            ["html", "htm"]
        );
        let mut html_catalog = Catalog::default();
        html_catalog.plugins.insert("html".to_owned(), html);
        for path in ["index.html", "legacy.htm", "INDEX.HTML", "LEGACY.HTM"] {
            let (owner, language) =
                language_for_path_in_catalog(&html_catalog, Path::new(path)).unwrap();
            assert_eq!(owner, "html");
            assert_eq!(language.id, "html");
        }
        for path in ["view.xhtml", "component.vue", "image.svg", "index.html.txt"] {
            assert!(language_for_path_in_catalog(&html_catalog, Path::new(path)).is_none());
        }

        // Known lockfiles use TOML, while a shared .lock suffix cannot select a grammar.
        let mut catalog = Catalog::default();
        catalog.plugins.insert("toml".to_owned(), toml);
        for path in ["config.toml", "Cargo.lock", "uv.lock", "CARGO.LOCK"] {
            assert_eq!(
                language_for_path_in_catalog(&catalog, Path::new(path))
                    .map(|(_, language)| language.id),
                Some("toml".to_owned()),
                "{path} should select the TOML plugin"
            );
        }
        for path in ["composer.lock", "Gemfile.lock", "random.lock"] {
            assert!(
                language_for_path_in_catalog(&catalog, Path::new(path)).is_none(),
                "{path} must not be classified by its .lock suffix"
            );
        }
    }
}
