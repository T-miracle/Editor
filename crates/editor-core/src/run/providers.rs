//! Which execution provider a workspace runs its configurations through.
//!
//! The platform selects a provider by the contract it implements and the scope it serves, never by
//! plugin identity. A user's choice is therefore recorded as the same kind of fact: a provider's
//! identity, kept beside the workspace it applies to, and applied only to launches that start after
//! it was made. An existing session keeps the provider that started it — a switch is never allowed to
//! retarget a program that is already running.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

/// Format of the stored choices; a newer file is refused rather than read with missing fields.
pub const RUN_PROVIDER_VERSION: u32 = 1;
/// Most workspaces one machine may record a choice for.
const MAX_WORKSPACES: usize = 512;
/// Longest accepted provider identity.
const MAX_PROVIDER_BYTES: usize = 128;

/// The provider choice for one scope: this machine's, or a project's.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunProviderChoices {
    #[serde(default = "current_version")]
    version: u32,
    /// Explicit choices by workspace; a workspace with no entry follows the default.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    workspaces: BTreeMap<String, String>,
}

fn current_version() -> u32 {
    RUN_PROVIDER_VERSION
}

/// Why a stored choice could not be used.
#[derive(Debug)]
pub enum RunProviderError {
    Io(std::io::Error),
    Malformed(serde_json::Error),
    /// The file was written by a newer build, which must not silently rewrite it.
    UnsupportedVersion {
        found: u32,
    },
    /// A stored identity is not a provider identity.
    Invalid {
        provider: String,
    },
}

impl std::fmt::Display for RunProviderError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "Run provider storage failed: {error}"),
            Self::Malformed(error) => write!(formatter, "Run provider file is unreadable: {error}"),
            Self::UnsupportedVersion { found } => write!(
                formatter,
                "Run provider file is version {found}, newer than this build understands"
            ),
            Self::Invalid { provider } => {
                write!(formatter, "Invalid provider identity: {provider}")
            }
        }
    }
}

impl std::error::Error for RunProviderError {}

impl RunProviderChoices {
    /// The provider this workspace runs through, or `None` to follow the default.
    pub fn chosen(&self, workspace: &str) -> Option<&str> {
        self.workspaces.get(workspace).map(String::as_str)
    }

    /// Record a choice, or clear it so the workspace follows the default again.
    pub fn choose(
        &mut self,
        workspace: &str,
        provider: Option<&str>,
    ) -> Result<(), RunProviderError> {
        match provider {
            Some(provider) => {
                self.validate_provider(provider)?;
                if self.workspaces.len() >= MAX_WORKSPACES
                    && !self.workspaces.contains_key(workspace)
                {
                    return Err(RunProviderError::Invalid {
                        provider: "too many recorded workspaces".into(),
                    });
                }
                self.workspaces
                    .insert(workspace.to_owned(), provider.to_owned());
            }
            None => {
                self.workspaces.remove(workspace);
            }
        }
        Ok(())
    }

    /// A provider identity is a plugin id: bounded and never empty.
    fn validate_provider(&self, provider: &str) -> Result<(), RunProviderError> {
        let valid = !provider.trim().is_empty()
            && provider.len() <= MAX_PROVIDER_BYTES
            && !provider.chars().any(char::is_control);
        if valid {
            Ok(())
        } else {
            Err(RunProviderError::Invalid {
                provider: provider.to_owned(),
            })
        }
    }

    /// Read a stored file, treating a missing one as no choices at all.
    pub fn from_json(bytes: &[u8]) -> Result<Self, RunProviderError> {
        let choices: Self = serde_json::from_slice(bytes).map_err(RunProviderError::Malformed)?;
        if choices.version > RUN_PROVIDER_VERSION {
            return Err(RunProviderError::UnsupportedVersion {
                found: choices.version,
            });
        }
        for provider in choices.workspaces.values() {
            choices.validate_provider(provider)?;
        }
        Ok(choices)
    }

    pub fn to_json(&self) -> Result<Vec<u8>, RunProviderError> {
        serde_json::to_vec_pretty(self).map_err(RunProviderError::Malformed)
    }
}

/// File holding this machine's provider choices, beside its other host-local run state.
pub fn providers_path(base: &Path) -> PathBuf {
    base.join("providers.json")
}

/// Read this machine's choices, or an empty set when nothing has been chosen yet.
pub fn load_providers(base: &Path) -> Result<RunProviderChoices, RunProviderError> {
    let path = providers_path(base);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RunProviderChoices {
                version: RUN_PROVIDER_VERSION,
                workspaces: BTreeMap::new(),
            });
        }
        Err(error) => return Err(RunProviderError::Io(error)),
    };
    RunProviderChoices::from_json(&bytes)
}

/// Write the choices atomically, leaving the previous file intact when any step fails.
pub fn save_providers(base: &Path, choices: &RunProviderChoices) -> Result<(), RunProviderError> {
    for provider in choices.workspaces.values() {
        choices.validate_provider(provider)?;
    }
    fs::create_dir_all(base).map_err(RunProviderError::Io)?;
    let path = providers_path(base);
    let bytes = choices.to_json()?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, bytes).map_err(RunProviderError::Io)?;
    fs::rename(&temporary, &path).map_err(RunProviderError::Io)
}
