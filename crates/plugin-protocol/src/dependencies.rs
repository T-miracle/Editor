//! Data-only private service preparation; no shell, installer, or global environment mutation.
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
