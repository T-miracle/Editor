//! Private dependency plans; native installation requires separate permission and concrete host consent.
use serde::{Deserialize, Serialize};

/// Artifacts form a bounded DAG. Paths start with an artifact ID, followed by a relative entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub artifacts: Vec<Artifact>,
    pub executable: String,
}

/// Immutable identity includes the version, platform and exact bytes, not a moving release label.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub id: String,
    pub version: String,
    /// Host OS and architecture, such as windows-x86_64; alternative platforms use different services.
    pub platform: String,
    pub sha256: String,
    pub source: Source,
    pub format: Format,
    #[serde(default)]
    pub requires: Vec<String>,
    /// Optional native step runs only after verification and explicit approval, before cache publication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installer: Option<Installer>,
}

/// Paths are relative to the verified artifact; only whole `${target}`/`${source}` arguments expand.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Installer {
    pub program: String,
    pub args: Vec<String>,
    pub target: String,
    pub purpose: String,
    pub kind: InstallerKind,
}

/// Project toolchains require a separate affirmative choice beyond ordinary service installation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallerKind {
    Service,
    ProjectSdk,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Source {
    Url { url: String },
    Package { path: String },
    Local { path: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Format {
    Zip,
    File { path: String },
}
