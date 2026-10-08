//! Versioned project descriptions keep archive policy reproducible across GUI and CLI.
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Shared filename intentionally differs from the installed plugin manifest.
pub const DESCRIPTION: &str = "nanobug-plugin.json";

/// Reading the shared description failed before any build could run. The path and original
/// I/O kind let GUI clients localize missing-file guidance without parsing platform error text.
#[derive(Debug)]
pub struct DescriptionReadError {
    /// Exact description path, rather than a compiler, asset or test-workspace path.
    pub path: PathBuf,
    /// Original filesystem cause; permission failures must not be reported as missing files.
    pub source: std::io::Error,
}
impl std::fmt::Display for DescriptionReadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Cannot read plugin project description: {}",
            self.path.display()
        )
    }
}
impl std::error::Error for DescriptionReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Files/directories copied into an archive. Destinations are always package-relative.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    /// Source relative to the project, or `{build}/...` for a declared native artifact.
    pub source: String,
    /// Portable path inside the plugin package.
    pub destination: String,
}
/// Optional Rust guest build; the artifact is identified from Cargo compiler messages.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WasmBuild {
    /// Cargo manifest relative to the plugin project.
    pub manifest: String,
    /// Optional library target name, needed only for ambiguous multi-library workspaces.
    #[serde(default)]
    pub library: Option<String>,
}
/// A native tool invocation, never a shell command string or repository helper script.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildStep {
    /// Executable resolved from PATH or an explicit tool path.
    pub program: String,
    /// Independent argv entries; supports `{project}`, `{build}`, `{profile}`.
    pub args: Vec<String>,
    /// Native steps require an explicit platform to prevent mislabeled foreign artifacts.
    pub platform: String,
}
/// Installed manifest, optional guest/native builds and explicit archive inputs.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectDescription {
    /// Description format version, independent of package/protocol/capability versions.
    pub version: u32,
    /// Manifest path relative to the project root.
    #[serde(default = "manifest_default")]
    pub manifest: String,
    /// Relative shared default; "." means the plugin project root.
    #[serde(default = "output_default")]
    pub output: String,
    #[serde(default)]
    pub wasm: Option<WasmBuild>,
    #[serde(default)]
    pub native: Vec<BuildStep>,
    pub assets: Vec<Asset>,
}
fn manifest_default() -> String {
    "manifest.json".into()
}
fn output_default() -> String {
    ".".into()
}

/// Canonical project identity. Loading never executes a build or changes project files.
pub struct Project {
    /// Canonical project directory used as the build's working directory and source boundary.
    pub root: PathBuf,
    /// Validated shared policy; local overrides are supplied separately through BuildOptions.
    pub description: ProjectDescription,
}
impl Project {
    /// Read a project directory or its description file, validate paths and bounded declarations.
    /// Returns DescriptionReadError for description I/O and rejects missing/unknown versions;
    /// no implicit archive policy is inferred from arbitrary project sources.
    pub fn read(path: &Path) -> anyhow::Result<Self> {
        let path = path
            .canonicalize()
            .with_context(|| format!("Plugin project does not exist: {}", path.display()))?;
        let (root, descriptor) = if path.is_dir() {
            (path.clone(), path.join(DESCRIPTION))
        } else {
            (
                path.parent()
                    .context("Missing project directory")?
                    .to_owned(),
                path,
            )
        };
        let metadata = descriptor
            .metadata()
            .map_err(|source| DescriptionReadError {
                path: descriptor.clone(),
                source,
            })?;
        anyhow::ensure!(
            metadata.len() <= 1024 * 1024,
            "Project description exceeds quota"
        );
        let bytes = std::fs::read(&descriptor).map_err(|source| DescriptionReadError {
            path: descriptor.clone(),
            source,
        })?;
        let description: ProjectDescription =
            serde_json::from_slice(&bytes).with_context(|| {
                format!(
                    "Invalid plugin project description: {}",
                    descriptor.display()
                )
            })?;
        anyhow::ensure!(
            description.version == 1,
            "Unsupported plugin project description version"
        );
        anyhow::ensure!(
            description.assets.len() <= 512 && description.native.len() <= 32,
            "Project declaration quota exceeded"
        );
        crate::package::validate_relative(&description.manifest)?;
        anyhow::ensure!(
            description.output == "."
                || crate::package::validate_relative(&description.output).is_ok(),
            "Shared output must be project-relative"
        );
        for asset in &description.assets {
            crate::package::validate_relative(&asset.destination)?;
            crate::package::validate_relative(
                asset
                    .source
                    .strip_prefix("{build}/")
                    .unwrap_or(&asset.source),
            )?;
        }
        if let Some(wasm) = &description.wasm {
            crate::package::validate_relative(&wasm.manifest)?;
        }
        for step in &description.native {
            anyhow::ensure!(
                !step.platform.is_empty() && step.args.len() <= 256,
                "Native step needs an explicit platform and bounded argv"
            );
        }
        Ok(Self { root, description })
    }
    /// Inputs for a watcher. Generated artifacts/output are deliberately absent.
    pub fn inputs(&self) -> Vec<PathBuf> {
        let mut paths = vec![
            self.root.join(DESCRIPTION),
            self.root.join(&self.description.manifest),
        ];
        if let Some(wasm) = &self.description.wasm {
            let manifest = self.root.join(&wasm.manifest);
            if let Some(root) = manifest.parent() {
                paths.extend([manifest.clone(), root.join("src"), root.join("Cargo.lock")]);
            }
        }
        paths.extend(
            self.description
                .assets
                .iter()
                .filter(|asset| !asset.source.starts_with("{build}/"))
                .map(|asset| self.root.join(&asset.source)),
        );
        // Native source changes are watched through their explicit project-relative argv.
        for step in &self.description.native {
            paths.extend(
                step.args
                    .iter()
                    .map(|arg| arg.replace("{project}", &self.root.display().to_string()))
                    .filter(|arg| !arg.contains('{') && self.root.join(arg).is_file())
                    .map(|arg| self.root.join(arg)),
            );
        }
        paths
    }
}
