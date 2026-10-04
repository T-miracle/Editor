//! Sharing run configurations through the project.
//!
//! A shared file is deliberately narrower than the host-local store: it carries what is portable —
//! a name, a target and its literal arguments, the directory relative to the project, and the build
//! and pre-launch actions — and it never carries what belongs to one machine. Environment values,
//! personal tool directories and absolute paths stay host-local, so sharing a configuration cannot
//! leak a key, a private path or an authorization decision into a repository.
//!
//! The host resolves a shared configuration by merging it with this machine's own overrides for the
//! same identity. Both the file and the form are validated by the same rules, so editing the file by
//! hand cannot mean something different from editing the form.
use super::{
    MAX_RUN_CONFIGS, MAX_RUN_STEPS, RunConfig, RunConfigError, RunConfigSet, RunConfigSource,
    RunStep, RunTarget,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[cfg(test)]
#[path = "shared_tests.rs"]
mod tests;

/// Directory of project-owned editor files, created only when a user chooses to share.
const PROJECT_DIRECTORY: &str = ".editor";
/// File holding the configurations a project shares with everyone who opens it.
const PROJECT_FILE: &str = "runs.json";
/// Format of the shared file; only this module decides which versions are readable.
pub const SHARED_CONFIG_VERSION: u32 = 1;

/// The project-relative directory a shared configuration uses when it names no directory.
///
/// A stored absolute path would be wrong on another machine, so the project root is expressed as a
/// token the host resolves instead of as a path one machine happened to have.
pub const WORKSPACE_TOKEN: &str = "${workspace}";

/// Failures a caller must explain rather than silently sharing nothing or sharing too much.
#[derive(Debug)]
pub enum SharedStoreError {
    /// Reading or writing the project file failed.
    Io(std::io::Error),
    /// The shared bytes are not a valid shared file.
    Malformed(serde_json::Error),
    /// The shared file is structurally valid but violates a run configuration rule.
    Invalid(RunConfigError),
    /// The file was written by a newer build, which must not silently rewrite it.
    UnsupportedVersion { found: u32 },
}

impl std::fmt::Display for SharedStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "Shared run configuration failed: {error}"),
            Self::Malformed(error) => {
                write!(
                    formatter,
                    "Shared run configuration file is unreadable: {error}"
                )
            }
            Self::Invalid(error) => write!(formatter, "{error}"),
            Self::UnsupportedVersion { found } => write!(
                formatter,
                "Shared run configuration file is version {found}, newer than this build understands"
            ),
        }
    }
}

impl std::error::Error for SharedStoreError {}

/// One configuration as the project stores it: portable values only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedConfig {
    /// Stable identity, so an edit updates the entry it came from instead of adding another.
    pub id: String,
    pub name: String,
    pub target: RunTarget,
    /// Project-relative directory, or [`WORKSPACE_TOKEN`] for the project root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub build: Vec<RunStep>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prelaunch: Vec<RunStep>,
}

/// The whole shared document, versioned so a later format can be read deliberately.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedSet {
    #[serde(default = "current_version")]
    version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub configurations: Vec<SharedConfig>,
}

fn current_version() -> u32 {
    SHARED_CONFIG_VERSION
}

impl Default for SharedSet {
    fn default() -> Self {
        Self {
            version: SHARED_CONFIG_VERSION,
            configurations: Vec::new(),
        }
    }
}

/// Path of the shared file inside one project.
pub fn project_path(workspace: &Path) -> PathBuf {
    workspace.join(PROJECT_DIRECTORY).join(PROJECT_FILE)
}

impl SharedConfig {
    /// The portable half of one configuration.
    ///
    /// An absolute directory is replaced by the workspace token, because a path that is meaningful on
    /// this machine means nothing on another one; everything else that is machine-specific — the
    /// environment, the tool directories and the chosen provider — is left out entirely.
    pub fn from_config(config: &RunConfig, workspace: &Path) -> Self {
        Self {
            id: config.id.clone(),
            name: config.name.clone(),
            target: config.target.clone(),
            directory: config.directory.as_ref().map(|directory| {
                relative_to_project(directory, workspace).unwrap_or_else(|| directory.clone())
            }),
            build: config.build.clone(),
            prelaunch: config.prelaunch.clone(),
        }
    }

    /// Resolve this entry for one workspace, applying that machine's own overrides.
    ///
    /// The overrides are keyed by identity, so a shared configuration keeps its personal environment
    /// and tool directories while its portable half comes from the project.
    pub fn resolve(self, workspace: &Path, overrides: Option<&RunConfig>) -> RunConfig {
        let overrides = overrides.filter(|local| local.id == self.id);
        RunConfig {
            id: self.id,
            name: self.name,
            target: self.target,
            directory: self.directory.map(|directory| {
                if directory == WORKSPACE_TOKEN {
                    workspace.display().to_string()
                } else {
                    directory
                }
            }),
            env: overrides.map(|local| local.env.clone()).unwrap_or_default(),
            tool_paths: overrides
                .map(|local| local.tool_paths.clone())
                .unwrap_or_default(),
            build: self.build,
            prelaunch: self.prelaunch,
            // A project entry stays a project entry: saving it writes back to the file it came from.
            source: RunConfigSource::Project,
            local: false,
        }
    }
}

impl SharedSet {
    /// Read the shared file, treating a missing one as a project that shares nothing.
    pub fn from_json(bytes: &[u8]) -> Result<Self, SharedStoreError> {
        let set: Self = serde_json::from_slice(bytes).map_err(SharedStoreError::Malformed)?;
        if set.version > SHARED_CONFIG_VERSION {
            return Err(SharedStoreError::UnsupportedVersion { found: set.version });
        }
        if set.configurations.len() > MAX_RUN_CONFIGS {
            return Err(SharedStoreError::Invalid(RunConfigError::InvalidIdentity {
                id: "too many shared configurations".to_owned(),
            }));
        }
        // Every entry is held to the same rules as one created in the form, so a hand-edited file
        // cannot introduce a configuration the form would have refused. The entry's own values are
        // checked here rather than a resolved copy, so what is refused is what the file says.
        for entry in &set.configurations {
            validate_shared(entry).map_err(SharedStoreError::Invalid)?;
        }
        Ok(set)
    }

    pub fn to_json(&self) -> Result<Vec<u8>, SharedStoreError> {
        serde_json::to_vec_pretty(self).map_err(SharedStoreError::Malformed)
    }

    /// Replace or add one entry, keeping the file's own order for everything else.
    pub fn upsert(&mut self, entry: SharedConfig) {
        match self
            .configurations
            .iter_mut()
            .find(|existing| existing.id == entry.id)
        {
            Some(existing) => *existing = entry,
            None => self.configurations.push(entry),
        }
    }
}

/// Hold one shared entry to the rules a form-created configuration obeys.
///
/// The entry is checked as it stands, before any workspace is known, so the reason a file was refused
/// is about the file rather than about which machine happened to read it.
fn validate_shared(entry: &SharedConfig) -> Result<(), RunConfigError> {
    if entry.id.trim().is_empty() || entry.id.len() > 128 {
        return Err(RunConfigError::InvalidIdentity {
            id: entry.id.clone(),
        });
    }
    if entry.name.trim().is_empty() {
        return Err(RunConfigError::EmptyName);
    }
    if entry.build.len() > MAX_RUN_STEPS || entry.prelaunch.len() > MAX_RUN_STEPS {
        return Err(RunConfigError::TooManySteps);
    }
    // A step's target rules are the same ones the form applies, so a reference and an action are
    // both checked by the single definition this crate already owns.
    // A shared directory is project-relative by design, so the directory rule that belongs to a
    // resolved host-local configuration is not applied to the file; everything else is.
    super::validate_target(&entry.target)?;
    for step in entry.build.iter().chain(entry.prelaunch.iter()) {
        step.validate().map_err(|error| error)?;
    }
    Ok(())
}

/// Read the shared configurations a project holds.
pub fn load(workspace: &Path) -> Result<SharedSet, SharedStoreError> {
    let path = project_path(workspace);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SharedSet::default());
        }
        Err(error) => return Err(SharedStoreError::Io(error)),
    };
    SharedSet::from_json(&bytes)
}

/// Write the shared file atomically, preserving the previous file when any step fails.
///
/// The project directory is created here and nowhere else: nothing writes into a project until the
/// user has chosen to share something with it.
pub fn save(workspace: &Path, set: &SharedSet) -> Result<(), SharedStoreError> {
    for entry in &set.configurations {
        validate_shared(entry).map_err(SharedStoreError::Invalid)?;
    }
    let path = project_path(workspace);
    let parent = path.parent().ok_or_else(|| {
        SharedStoreError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Shared configuration path has no parent directory",
        ))
    })?;
    fs::create_dir_all(parent).map_err(SharedStoreError::Io)?;
    let bytes = set.to_json()?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, bytes).map_err(SharedStoreError::Io)?;
    fs::rename(&temporary, &path).map_err(SharedStoreError::Io)
}

/// Merge a project's shared configurations into one machine's own set.
///
/// The machine's own entry of the same identity supplies the overrides; a project entry whose
/// identity is already known locally keeps that identity, so sessions and menus hold their link.
pub fn merge(workspace: &Path, local: &RunConfigSet, shared: &SharedSet) -> RunConfigSet {
    let overrides = local
        .configurations
        .iter()
        .map(|config| (config.id.clone(), config))
        .collect::<BTreeMap<_, _>>();
    let mut set = local.clone();
    for entry in &shared.configurations {
        let resolved = entry
            .clone()
            .resolve(workspace, overrides.get(&entry.id).copied());
        match set
            .configurations
            .iter_mut()
            .find(|existing| existing.id == resolved.id)
        {
            Some(existing) => *existing = resolved,
            None => {
                if set.configurations.len() >= MAX_RUN_CONFIGS {
                    break;
                }
                set.configurations.push(resolved);
            }
        }
    }
    set
}

/// Express an absolute directory relative to the project when it lies inside it.
///
/// A directory outside the project has no portable form, and the caller keeps the path as written
/// rather than inventing a location inside the project.
fn relative_to_project(directory: &str, workspace: &Path) -> Option<String> {
    let directory = Path::new(directory);
    let relative = directory.strip_prefix(workspace).ok()?;
    if relative.as_os_str().is_empty() {
        return Some(WORKSPACE_TOKEN.to_owned());
    }
    Some(relative.display().to_string())
}
