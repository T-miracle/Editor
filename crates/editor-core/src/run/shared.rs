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
    MAX_RUN_CONFIGS, MAX_RUN_STEPS, RunBreakpoints, RunConfig, RunConfigError, RunConfigSet,
    RunConfigSource, RunStep, RunTarget, StepTarget,
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
pub const SHARED_CONFIG_VERSION: u32 = 2;

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
                "Unsupported shared run configuration version {found}"
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
    /// Where this configuration's program should stop, which is the same on every machine.
    #[serde(default, skip_serializing_if = "RunBreakpoints::is_empty")]
    pub breakpoints: RunBreakpoints,
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
    /// Paths inside the project become portable. Paths outside it remain visibly invalid for sharing,
    /// so saving fails instead of exporting private paths or silently changing the user's command.
    pub fn from_config(config: &RunConfig, workspace: &Path) -> Self {
        Self {
            id: config.id.clone(),
            name: config.name.clone(),
            target: map_target(&config.target, &|value| portable(value, workspace)),
            directory: config.directory.as_ref().map(|directory| {
                relative_to_project(directory, workspace).unwrap_or_else(|| directory.clone())
            }),
            build: map_steps(&config.build, &|value| portable(value, workspace)),
            prelaunch: map_steps(&config.prelaunch, &|value| portable(value, workspace)),
            // A source location travels only after its path passes the same project boundary.
            breakpoints: config
                .breakpoints
                .map_sources(&|source| portable(source, workspace)),
        }
    }

    /// Resolve this entry for one workspace, applying that machine's own overrides.
    ///
    /// The overrides are keyed by identity, so a shared configuration keeps its personal environment
    /// and tool directories while its portable half comes from the project.
    pub fn resolve(self, workspace: &Path, overrides: Option<&RunConfig>) -> RunConfig {
        let overrides = overrides.filter(|local| local.id == self.id);
        let target = map_target(&self.target, &|value| resolved(value, workspace));
        // Discovery provenance is machine-local, and belongs to a target identity rather than its args.
        // Replacing the shared target cannot carry the previous source's missing/error blocker forward.
        let from_target = overrides
            .filter(|local| same_target_identity(&local.target, &target))
            .and_then(|local| local.from_target.clone());
        RunConfig {
            id: self.id,
            name: self.name,
            target,
            directory: self.directory.map(|directory| {
                if directory == WORKSPACE_TOKEN
                    || directory.starts_with(&format!("{WORKSPACE_TOKEN}/"))
                {
                    resolved(&directory, workspace)
                } else {
                    workspace.join(directory).display().to_string()
                }
            }),
            env: overrides.map(|local| local.env.clone()).unwrap_or_default(),
            tool_paths: overrides
                .map(|local| local.tool_paths.clone())
                .unwrap_or_default(),
            build: map_steps(&self.build, &|value| resolved(value, workspace)),
            prelaunch: map_steps(&self.prelaunch, &|value| resolved(value, workspace)),
            // A project entry stays a project entry: saving it writes back to the file it came from.
            source: RunConfigSource::Project,
            // A shared entry is a definition, not a pointer at a discovered target: that link belongs
            // to the machine whose discovery offered it.
            from_target,
            // Which provider runs a program is this machine's fact, not the project's: another
            // machine may have a different provider installed.
            provider: overrides.and_then(|local| local.provider.clone()),
            // Breakpoints are source and line, so they are the project's: someone else's program stops
            // in the same place, while the environment and tool directories stay this machine's.
            breakpoints: self
                .breakpoints
                .map_sources(&|source| resolved(source, workspace)),
            local: false,
        }
    }
}

/// Editable arguments and display names do not change discovery identity; opaque bindings do.
fn same_target_identity(left: &RunTarget, right: &RunTarget) -> bool {
    match (left, right) {
        (RunTarget::Program { program: a, .. }, RunTarget::Program { program: b, .. }) => a == b,
        (
            RunTarget::Script {
                interpreter: a,
                script: x,
                ..
            },
            RunTarget::Script {
                interpreter: b,
                script: y,
                ..
            },
        ) => a == b && x == y,
        (
            RunTarget::Provided {
                provider: a,
                binding: x,
                ..
            },
            RunTarget::Provided {
                provider: b,
                binding: y,
                ..
            },
        ) => a == b && x == y,
        _ => false,
    }
}

impl SharedSet {
    /// Read the shared file, treating a missing one as a project that shares nothing.
    pub fn from_json(bytes: &[u8]) -> Result<Self, SharedStoreError> {
        let set: Self = serde_json::from_slice(bytes).map_err(SharedStoreError::Malformed)?;
        set.validate()?;
        Ok(set)
    }

    /// Validate a complete proposal before a caller commits either its project or local file.
    ///
    /// Returns the same version, bounds and field errors as parsing or saving the shared document.
    pub fn validate(&self) -> Result<(), SharedStoreError> {
        // Only the published v1 predecessor and current v2 format have a defined migration.
        // Unknown older values are not a license to reinterpret or clear a project definition.
        if !(1..=SHARED_CONFIG_VERSION).contains(&self.version) {
            return Err(SharedStoreError::UnsupportedVersion {
                found: self.version,
            });
        }
        if self.configurations.len() > MAX_RUN_CONFIGS {
            return Err(SharedStoreError::Invalid(RunConfigError::InvalidIdentity {
                id: "too many shared configurations".to_owned(),
            }));
        }
        // Every entry is held to the same rules as one created in the form, so a hand-edited file
        // cannot introduce a configuration the form would have refused. The entry's own values are
        // checked here rather than a resolved copy, so what is refused is what the file says.
        for entry in &self.configurations {
            validate_shared(entry).map_err(SharedStoreError::Invalid)?;
        }
        Ok(())
    }

    pub fn to_json(&self) -> Result<Vec<u8>, SharedStoreError> {
        serde_json::to_vec_pretty(self).map_err(SharedStoreError::Malformed)
    }

    /// Replace or add one entry, keeping the file's own order for everything else.
    pub fn upsert(&mut self, entry: SharedConfig) {
        // Preserve older readable data while writing new fields under their actual format version.
        self.version = SHARED_CONFIG_VERSION;
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
    validate_portable_target(&entry.target)?;
    if let Some(directory) = &entry.directory {
        validate_portable_path(directory)?;
    }
    for step in entry.build.iter().chain(entry.prelaunch.iter()) {
        step.validate()?;
        if let StepTarget::Action { target } = &step.target {
            validate_portable_target(target)?;
        }
    }
    entry
        .breakpoints
        .validate()
        .map_err(RunConfigError::InvalidBreakpoint)?;
    for breakpoint in entry.breakpoints.entries() {
        validate_portable_path(&breakpoint.source)?;
    }
    Ok(())
}

/// An absolute path is portable only when it names a location inside this project.
fn portable(value: &str, workspace: &Path) -> String {
    if Path::new(value).is_absolute() {
        if let Some(relative) = relative_to_project(value, workspace) {
            return if relative == WORKSPACE_TOKEN {
                relative
            } else {
                format!("{WORKSPACE_TOKEN}/{}", relative.replace('\\', "/"))
            };
        }
    }
    value.into()
}

/// Expand only the explicitly supported project token, never environment variables or Shell syntax.
fn resolved(value: &str, workspace: &Path) -> String {
    if value == WORKSPACE_TOKEN {
        return workspace.display().to_string();
    }
    value
        .strip_prefix(&format!("{WORKSPACE_TOKEN}/"))
        .map(|relative| workspace.join(relative).display().to_string())
        .unwrap_or_else(|| value.into())
}

/// Convert executable and argument path values while preserving literal argument boundaries and scripts.
fn map_target(target: &RunTarget, map: &impl Fn(&str) -> String) -> RunTarget {
    match target {
        RunTarget::Provided {
            provider,
            binding,
            label,
            args,
        } => RunTarget::Provided {
            provider: provider.clone(),
            binding: binding.clone(),
            label: label.clone(),
            args: args.iter().map(|arg| map_argument(arg, map)).collect(),
        },
        RunTarget::Program { program, args } => RunTarget::Program {
            program: map(program),
            args: args.iter().map(|arg| map_argument(arg, map)).collect(),
        },
        RunTarget::Script {
            interpreter,
            args,
            script,
        } => RunTarget::Script {
            interpreter: map(interpreter),
            args: args.iter().map(|arg| map_argument(arg, map)).collect(),
            script: script.clone(),
        },
    }
}

/// An option assignment stays one argv item; only its literal value crosses the path boundary.
fn map_argument(argument: &str, map: &impl Fn(&str) -> String) -> String {
    match argument.split_once('=') {
        Some((option, value)) if option.starts_with('-') => format!("{option}={}", map(value)),
        _ => map(argument),
    }
}

/// Prepared actions obey the same portability rules as the final program; build references keep identity.
fn map_steps(steps: &[RunStep], map: &impl Fn(&str) -> String) -> Vec<RunStep> {
    steps
        .iter()
        .map(|step| RunStep {
            name: step.name.clone(),
            target: match &step.target {
                StepTarget::Action { target } => StepTarget::Action {
                    target: map_target(target, map),
                },
                reference => reference.clone(),
            },
        })
        .collect()
}

/// Reject machine paths, unknown variables and traversal before writing or resolving a shared definition.
fn validate_portable_path(value: &str) -> Result<(), RunConfigError> {
    let relative = if value == WORKSPACE_TOKEN {
        ""
    } else {
        value
            .strip_prefix(&format!("{WORKSPACE_TOKEN}/"))
            .unwrap_or(value)
    };
    if value.contains('\0')
        || relative.contains("${")
        || relative.contains(':')
        || relative.starts_with(['/', '\\'])
        || relative.split(['/', '\\']).any(|part| part == "..")
        || Path::new(relative).is_absolute()
    {
        return Err(RunConfigError::InvalidSharedPath { path: value.into() });
    }
    Ok(())
}

/// Script bodies are explicit user code; the host validates path fields without trying to parse that code.
fn validate_portable_target(target: &RunTarget) -> Result<(), RunConfigError> {
    if let RunTarget::Provided { binding, .. } = target {
        // Portable bindings may contain nested provider fields, but never machine paths or secrets.
        let value: serde_json::Value =
            serde_json::from_str(binding).map_err(|_| RunConfigError::ProgramTooLong)?;
        validate_portable_binding(&value)?;
    } else {
        validate_portable_path(target.executable())?;
    }
    let args = match target {
        RunTarget::Program { args, .. }
        | RunTarget::Script { args, .. }
        | RunTarget::Provided { args, .. } => args,
    };
    for arg in args {
        let value = match arg.split_once('=') {
            Some((option, value)) if option.starts_with('-') => value,
            _ => arg.as_str(),
        };
        // Ordinary literal options are not paths. Path values and variables must be portable.
        if Path::new(value).is_absolute()
            || value.contains("${")
            || value.starts_with(['/', '\\'])
            || value.starts_with("../")
            || value.starts_with("..\\")
            || value.as_bytes().get(1) == Some(&b':')
        {
            validate_portable_path(value)?;
        }
    }
    Ok(())
}

/// The host validates only generic portable values; it does not interpret a provider's binding keys.
fn validate_portable_binding(value: &serde_json::Value) -> Result<(), RunConfigError> {
    match value {
        serde_json::Value::String(text) => validate_portable_path(text),
        serde_json::Value::Array(values) => {
            for value in values {
                validate_portable_binding(value)?;
            }
            Ok(())
        }
        serde_json::Value::Object(fields) => {
            for value in fields.values() {
                validate_portable_binding(value)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
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
    set.validate()?;
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
    // Project entries in the local store are overrides, not an executable fallback. Deleted or
    // unreadable project definitions must disappear instead of reviving an old cached command.
    set.configurations.retain(|config| config.local);
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
    if set
        .selected
        .as_deref()
        .is_some_and(|id| set.find(id).is_none())
    {
        set.selected = None;
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
