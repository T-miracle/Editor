//! Host-local run configurations: validated, versioned and independent from active sessions.
//!
//! A run configuration describes what to launch; it is never a process, an active session, or a
//! grant of authority. Validation here is shared by the configuration form, the stored file and the
//! launch path so one meaning of the data is maintained instead of three drifting interpretations.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod store;
pub use store::{RunStoreError, default_root, load, save, storage_path};
#[cfg(test)]
mod tests;

/// Current stored format. A newer file is refused instead of being read with missing fields.
pub const RUN_CONFIG_VERSION: u32 = 1;
/// Bound on saved configurations per workspace; the menu is not an unbounded list.
pub const MAX_RUN_CONFIGS: usize = 128;
/// Arguments are a literal vector, bounded exactly like the execution contract they become.
pub const MAX_RUN_ARGUMENTS: usize = 128;
const MAX_NAME_BYTES: usize = 256;
const MAX_PROGRAM_BYTES: usize = 4096;
const MAX_ARGUMENT_BYTES: usize = 4096;
const MAX_DIRECTORY_BYTES: usize = 4096;
/// Environment entries are bounded exactly like the execution contract they become.
const MAX_ENV_ENTRIES: usize = 64;
const MAX_ENV_NAME_BYTES: usize = 128;
const MAX_ENV_VALUE_BYTES: usize = 32 * 1024;

/// What a configuration launches: a literal program, or an explicitly chosen interpreter.
///
/// Program arguments are never joined into a command line. Shell scripting is a separate, explicit
/// mode so metacharacters cannot be reinterpreted merely because a path contained a space.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum RunTarget {
    Program {
        program: String,
        #[serde(default)]
        args: Vec<String>,
    },
    Script {
        /// Explicit interpreter path or name; there is no implicit terminal profile.
        interpreter: String,
        #[serde(default)]
        args: Vec<String>,
        script: String,
    },
}

impl RunTarget {
    /// The executable the execution provider is asked to start.
    pub fn executable(&self) -> &str {
        match self {
            Self::Program { program, .. } => program,
            Self::Script { interpreter, .. } => interpreter,
        }
    }
    /// The complete literal argument vector, including a script body for interpreter mode.
    ///
    /// Public so a caller can show or store exactly what will be passed, without re-deriving it.
    pub fn arguments(&self) -> Vec<&str> {
        match self {
            Self::Program { args, .. } => args.iter().map(String::as_str).collect(),
            Self::Script { args, script, .. } => args
                .iter()
                .map(String::as_str)
                .chain(std::iter::once(script.as_str()))
                .collect(),
        }
    }
}

/// One saved configuration. Identity is stable across edits so sessions and menus keep their link.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunConfig {
    /// Host-generated stable identity; discovery cannot replace a configuration by matching a name.
    pub id: String,
    pub name: String,
    pub target: RunTarget,
    /// Absolute or project-relative directory; empty means the workspace root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
    /// Environment entries applied to this configuration's program, over what it would inherit.
    ///
    /// Values are host-local by default and never shared; the host neither interprets nor logs them.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Directories searched before the inherited path when this configuration's program is found.
    ///
    /// This is the local tool-path override: it affects only this configuration's launch and never
    /// changes the editor, the plugin platform or any other program's search order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_paths: Vec<String>,
    /// The configuration's own build actions, kept apart from starting the program.
    ///
    /// Build is a separate operation so Build can run without launching, and Launch can require it
    /// without duplicating the command.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub build: Vec<RunStep>,
    /// Steps that run in order before the program starts, each of which must succeed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prelaunch: Vec<RunStep>,
    /// Local-only configurations never modify project files; sharing is an explicit user action.
    #[serde(default = "crate::run::default_local")]
    pub local: bool,
}

/// One prepared action: a program, or an explicitly chosen interpreter running a script.
///
/// A step reuses the launch target so a build command is validated and stored by the same rules as
/// a program the user starts directly, instead of a second, weaker grammar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunStep {
    /// Shown while the step runs, and in the failure that stops the sequence.
    pub name: String,
    pub target: RunTarget,
}

/// Bound on prepared actions per list; the build page is a short sequence, not a task system.
pub const MAX_RUN_STEPS: usize = 16;
const MAX_STEP_NAME_BYTES: usize = 128;

impl RunStep {
    /// Validate a prepared action by the same rules the launch target obeys.
    fn validate(&self) -> Result<(), RunConfigError> {
        if self.name.trim().is_empty() {
            return Err(RunConfigError::EmptyStepName);
        }
        if self.name.len() > MAX_STEP_NAME_BYTES {
            return Err(RunConfigError::StepNameTooLong);
        }
        validate_target(&self.target)
    }
}

/// The program or interpreter a target starts, with its bounded literal arguments.
fn validate_target(target: &RunTarget) -> Result<(), RunConfigError> {
    let executable = target.executable();
    if executable.trim().is_empty() {
        return Err(RunConfigError::EmptyProgram);
    }
    if executable.len() > MAX_PROGRAM_BYTES || executable.contains('\0') {
        return Err(RunConfigError::ProgramTooLong);
    }
    let arguments = target.arguments();
    if arguments.len() > MAX_RUN_ARGUMENTS {
        return Err(RunConfigError::TooManyArguments);
    }
    if arguments
        .iter()
        .any(|argument| argument.len() > MAX_ARGUMENT_BYTES || argument.contains('\0'))
    {
        return Err(RunConfigError::ArgumentTooLong);
    }
    Ok(())
}

/// Sharing is off unless the user opts in, including for files written by older versions.
pub(crate) fn default_local() -> bool {
    true
}

/// Longest accepted tool directory, matching the bounded environment it becomes.
const MAX_TOOL_PATH_BYTES: usize = 4096;
/// Most directories one configuration may search before the inherited path.
const MAX_TOOL_PATHS: usize = 16;

/// The environment this configuration's launch needs beyond what the user typed.
///
/// Tool directories become a leading `PATH`, so the program resolves from the configuration's own
/// choice of tools. The value is derived rather than stored: the file keeps the directories, and a
/// user's own `PATH` entry is preserved behind them instead of being replaced.
pub fn launch_environment(
    env: &BTreeMap<String, String>,
    tool_paths: &[String],
) -> BTreeMap<String, String> {
    let mut entries = env.clone();
    if tool_paths.is_empty() {
        return entries;
    }
    let separator = if cfg!(windows) { ';' } else { ':' };
    let inherited = entries
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("PATH"))
        .map(|(_, value)| value.clone())
        .or_else(|| std::env::var("PATH").ok())
        .unwrap_or_default();
    let mut value = tool_paths.join(&separator.to_string());
    if !inherited.is_empty() {
        value.push(separator);
        value.push_str(&inherited);
    }
    // One `PATH` reaches the child: the override first, then whatever it was going to search.
    entries.retain(|name, _| !name.eq_ignore_ascii_case("PATH"));
    entries.insert("PATH".to_owned(), value);
    entries
}

/// Why a configuration cannot be launched; the form shows the same reason before starting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunConfigError {
    /// The stored file is newer than this build understands.
    UnsupportedVersion {
        found: u32,
    },
    EmptyName,
    EmptyProgram,
    NameTooLong,
    ProgramTooLong,
    ArgumentTooLong,
    TooManyArguments,
    DirectoryNotAbsolute,
    DirectoryTooLong,
    /// More environment entries than one launch may carry.
    TooManyEnvEntries,
    /// An entry whose name or value could not be passed to a native program.
    InvalidEnvEntry {
        name: String,
    },
    /// More tool directories than one launch may search before the inherited path.
    TooManyToolPaths,
    /// A tool directory that is not an absolute, searchable path.
    InvalidToolPath {
        path: String,
    },
    /// More prepared actions than one configuration may hold.
    TooManySteps,
    /// A prepared action without a name to show while it runs.
    EmptyStepName,
    StepNameTooLong,
    /// The identifier is empty or duplicated inside one set.
    InvalidIdentity {
        id: String,
    },
}

impl std::fmt::Display for RunConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedVersion { found } => write!(
                formatter,
                "Run configuration format {found} is newer than this editor supports"
            ),
            Self::EmptyName => write!(formatter, "A run configuration needs a name"),
            Self::EmptyProgram => write!(formatter, "A run configuration needs a program"),
            Self::NameTooLong => write!(formatter, "Run configuration name is too long"),
            Self::ProgramTooLong => write!(formatter, "Run program or interpreter is too long"),
            Self::ArgumentTooLong => write!(formatter, "A run argument is too long"),
            Self::TooManyArguments => write!(formatter, "Too many run arguments"),
            Self::DirectoryNotAbsolute => write!(
                formatter,
                "Working directory must be absolute; relative paths are resolved from the workspace"
            ),
            Self::DirectoryTooLong => write!(formatter, "Working directory is too long"),
            Self::TooManyEnvEntries => {
                write!(
                    formatter,
                    "Too many environment entries for one configuration"
                )
            }
            Self::InvalidEnvEntry { name } => {
                write!(formatter, "Invalid environment entry: {name}")
            }
            Self::TooManyToolPaths => {
                write!(formatter, "Too many tool directories for one configuration")
            }
            Self::InvalidToolPath { path } => {
                write!(formatter, "Invalid tool directory: {path}")
            }
            Self::TooManySteps => write!(formatter, "Too many build or pre-launch steps"),
            Self::EmptyStepName => write!(formatter, "A build or pre-launch step needs a name"),
            Self::StepNameTooLong => write!(formatter, "A step name is too long"),
            Self::InvalidIdentity { id } => write!(formatter, "Invalid run configuration id: {id}"),
        }
    }
}

impl std::error::Error for RunConfigError {}

impl RunConfig {
    /// Validate every field the launch path depends on, with no filesystem probe.
    pub fn validate(&self) -> Result<(), RunConfigError> {
        if self.id.trim().is_empty() || self.id.len() > 128 {
            return Err(RunConfigError::InvalidIdentity {
                id: self.id.clone(),
            });
        }
        if self.name.trim().is_empty() {
            return Err(RunConfigError::EmptyName);
        }
        if self.name.len() > MAX_NAME_BYTES {
            return Err(RunConfigError::NameTooLong);
        }
        validate_target(&self.target)?;
        if self.build.len() > MAX_RUN_STEPS || self.prelaunch.len() > MAX_RUN_STEPS {
            return Err(RunConfigError::TooManySteps);
        }
        for step in self.build.iter().chain(self.prelaunch.iter()) {
            step.validate()?;
        }
        if let Some(directory) = &self.directory {
            if directory.len() > MAX_DIRECTORY_BYTES || directory.contains('\0') {
                return Err(RunConfigError::DirectoryTooLong);
            }
            // An absolute directory is portable across providers; a relative path would silently
            // depend on whatever directory a provider happened to inherit.
            if !std::path::Path::new(directory).is_absolute() {
                return Err(RunConfigError::DirectoryNotAbsolute);
            }
        }
        if self.tool_paths.len() > MAX_TOOL_PATHS {
            return Err(RunConfigError::TooManyToolPaths);
        }
        for directory in &self.tool_paths {
            // A directory a native program could not search is refused while the form is open. An
            // entry containing a separator would silently become two search directories.
            let valid = !directory.is_empty()
                && directory.len() <= MAX_TOOL_PATH_BYTES
                && !directory.contains('\0')
                && !directory.contains(';')
                && std::path::Path::new(directory).is_absolute();
            if !valid {
                return Err(RunConfigError::InvalidToolPath {
                    path: directory.clone(),
                });
            }
        }
        if self.env.len() > MAX_ENV_ENTRIES {
            return Err(RunConfigError::TooManyEnvEntries);
        }
        for (name, value) in &self.env {
            // A name that could not be passed to a native child is refused while the form is open,
            // rather than failing after a console window has already appeared.
            let valid_name = !name.is_empty()
                && name.len() <= MAX_ENV_NAME_BYTES
                && !name.contains('=')
                && !name.chars().any(char::is_control);
            if !valid_name || value.len() > MAX_ENV_VALUE_BYTES || value.contains('\0') {
                return Err(RunConfigError::InvalidEnvEntry { name: name.clone() });
            }
        }
        Ok(())
    }

    /// Literal program arguments for a provider that starts an executable directly.
    pub fn literal_arguments(&self) -> Vec<String> {
        self.target
            .arguments()
            .into_iter()
            .map(str::to_owned)
            .collect()
    }
}

/// Facts about a configuration that changes its visible explanation rather than its behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunConfigReadiness {
    /// Stored settings are complete enough to launch through an execution provider.
    Launchable,
    /// The configuration itself is valid but no build action is configured yet.
    WithoutBuild,
}

/// Stored configurations plus the format version they were read from.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunConfigSet {
    /// Format of this set; only this module decides which versions are writable.
    #[serde(default = "current_version")]
    version: u32,
    #[serde(default)]
    pub configurations: Vec<RunConfig>,
    /// Selected configuration identity; the top bar shows this target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<String>,
}

fn current_version() -> u32 {
    RUN_CONFIG_VERSION
}

/// A fresh set is written in the current format rather than as an unversioned document.
impl Default for RunConfigSet {
    fn default() -> Self {
        Self {
            version: RUN_CONFIG_VERSION,
            configurations: Vec::new(),
            selected: None,
        }
    }
}

impl RunConfigSet {
    /// Format version this set is written in; newer files are refused instead of guessed at.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Read a stored set, refusing a format this build cannot interpret.
    pub fn from_json(bytes: &[u8]) -> Result<Self, RunStoreError> {
        let set: Self = serde_json::from_slice(bytes).map_err(RunStoreError::Malformed)?;
        if set.version > RUN_CONFIG_VERSION {
            return Err(RunStoreError::Invalid(RunConfigError::UnsupportedVersion {
                found: set.version,
            }));
        }
        for configuration in &set.configurations {
            configuration.validate().map_err(RunStoreError::Invalid)?;
        }
        if set.configurations.len() > MAX_RUN_CONFIGS {
            return Err(RunStoreError::Invalid(RunConfigError::InvalidIdentity {
                id: format!("more than {MAX_RUN_CONFIGS} configurations"),
            }));
        }
        Ok(set)
    }

    pub fn to_json(&self) -> Result<Vec<u8>, RunStoreError> {
        serde_json::to_vec_pretty(self).map_err(RunStoreError::Malformed)
    }

    /// Save or replace one configuration, keeping its identity and the current selection.
    pub fn upsert(&mut self, configuration: RunConfig) -> Result<(), RunConfigError> {
        configuration.validate()?;
        match self
            .configurations
            .iter_mut()
            .find(|existing| existing.id == configuration.id)
        {
            Some(existing) => *existing = configuration,
            None => {
                if self.configurations.len() >= MAX_RUN_CONFIGS {
                    return Err(RunConfigError::InvalidIdentity {
                        id: format!("more than {MAX_RUN_CONFIGS} configurations"),
                    });
                }
                self.configurations.push(configuration);
            }
        }
        Ok(())
    }

    /// Remove one configuration; removing the selection clears it instead of pointing at a ghost.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.configurations.len();
        self.configurations.retain(|entry| entry.id != id);
        if self.selected.as_deref() == Some(id) {
            self.selected = None;
        }
        self.configurations.len() != before
    }

    /// Select a stored configuration; an unknown identity is not selected.
    pub fn select(&mut self, id: &str) -> bool {
        if self.configurations.iter().any(|entry| entry.id == id) {
            self.selected = Some(id.to_owned());
            true
        } else {
            false
        }
    }

    pub fn selected(&self) -> Option<&RunConfig> {
        let id = self.selected.as_deref()?;
        self.configurations.iter().find(|entry| entry.id == id)
    }

    /// A stored configuration for the same identity, used before creating a discovered duplicate.
    pub fn find(&self, id: &str) -> Option<&RunConfig> {
        self.configurations.iter().find(|entry| entry.id == id)
    }

    /// Whether the configuration is complete enough to launch, and why not when it is not.
    pub fn readiness(&self, id: &str) -> Option<RunConfigReadiness> {
        let configuration = self.find(id)?;
        configuration
            .validate()
            .ok()
            .map(|()| RunConfigReadiness::WithoutBuild)
    }

    /// Identity for a new host-created configuration.
    ///
    /// The identity is derived from the workspace and a monotonically increasing counter so that it
    /// is stable for a given creation order and never depends on the executable path, which the user
    /// is expected to change.
    pub fn generate_id(&self, workspace: &str) -> String {
        let mut counter = self.configurations.len();
        loop {
            let candidate = format!("{}-{counter}", short_digest(workspace));
            if self.find(&candidate).is_none() {
                return candidate;
            }
            counter += 1;
        }
    }

    /// Count configurations by launch mode, for diagnostics and menu grouping.
    pub fn counts(&self) -> BTreeMap<&'static str, usize> {
        let mut counts = BTreeMap::new();
        for configuration in &self.configurations {
            let key = match configuration.target {
                RunTarget::Program { .. } => "program",
                RunTarget::Script { .. } => "script",
            };
            *counts.entry(key).or_insert(0) += 1;
        }
        counts
    }
}

/// Short, stable, non-secret digest used for host-local identities and file names.
pub(crate) fn short_digest(value: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
