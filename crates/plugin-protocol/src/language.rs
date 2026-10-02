//! Language-service declarations and bounded optional hook data, independent of concrete language names.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Recognition, highlighting, and this LSP provider may be contributed by different packages.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub id: String,
    pub language: String,
    pub service: String,
    /// A hook may select another approved startup plan, not invent an executable or argument vector.
    #[serde(default)]
    pub alternatives: Vec<String>,
    /// Only an explicitly saved user/project value overrides native executable resolution.
    #[serde(default)]
    pub executable_setting: Option<String>,
    #[serde(default)]
    pub hook: bool,
    #[serde(default)]
    pub initialization_options: Value,
    /// Section names are opaque protocol data; the host never prefixes a concrete server name.
    #[serde(default)]
    pub configuration: BTreeMap<String, Value>,
    #[serde(default)]
    pub readiness: Option<Readiness>,
    #[serde(default)]
    pub completion_triggers: Vec<String>,
    #[serde(default)]
    pub completion_after_whitespace: Vec<String>,
}

/// Without this optional condition, a successful initialize response plus initialized means ready.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Readiness {
    pub notification: String,
    /// RFC 6901 pointer into notification params.
    pub pointer: String,
    pub expected: Value,
    pub timeout_ms: u32,
}

/// Hooks see resolved settings and fixed candidate declarations within one trusted workspace.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Context {
    pub provider: String,
    pub workspace: String,
    pub settings: crate::settings::Effective,
    pub candidates: BTreeMap<String, crate::process::Service>,
}

/// Omitted fields retain declarations. Project roots remain relative to the owning workspace.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    /// Dynamic executables/arguments require the separate process.exec grant; fixed plans do not.
    pub program: Option<String>,
    pub args: Option<Vec<String>>,
    pub service: Option<String>,
    pub project_root: Option<String>,
    pub initialization_options: Option<Value>,
    pub configuration: Option<BTreeMap<String, Value>>,
}

impl Provider {
    /// Package inspection rejects unbounded protocol data before invoking a guest or starting native code.
    pub fn valid(&self) -> bool {
        let id = |text: &str| {
            !text.is_empty()
                && text.len() <= 100
                && text.bytes().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
                })
        };
        id(&self.id)
            && id(&self.language)
            && id(&self.service)
            && self.alternatives.len() <= 32
            && self.alternatives.iter().all(|value| id(value))
            && self.configuration.len() <= 64
            && self.configuration.keys().all(|key| key.len() <= 256)
            && self.readiness.as_ref().is_none_or(|ready| {
                !ready.notification.is_empty()
                    && ready.notification.len() <= 256
                    && (ready.pointer.is_empty() || ready.pointer.starts_with('/'))
                    && ready.pointer.len() <= 256
                    && (1..=300_000).contains(&ready.timeout_ms)
            })
            && self.completion_triggers.len() <= 64
            && self.completion_after_whitespace.len() <= 64
            && self
                .completion_triggers
                .iter()
                .chain(&self.completion_after_whitespace)
                .all(|value| value.len() <= 256)
            && serde_json::to_vec(self).is_ok_and(|bytes| bytes.len() <= 256 * 1024)
    }
}
