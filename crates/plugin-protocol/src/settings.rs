//! Declarative plugin-owned settings: host authority and editor preferences are never configuration keys.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Settings default to user scope; a package must explicitly allow a project override.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    #[default]
    User,
    Project,
}

/// This capability revision applies changes by replacing only affected plugin instances.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Apply {
    #[default]
    RestartInstance,
}

/// A small native form vocabulary keeps validation independent of any particular plugin.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SettingType {
    Boolean,
    String {
        #[serde(default = "default_string_limit")]
        max_length: usize,
    },
    Integer {
        min: i64,
        max: i64,
    },
    Enum {
        choices: Vec<String>,
    },
}
fn default_string_limit() -> usize {
    4096
}

/// Defaults, scope and application behavior are declarations checked before installation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub title: String,
    pub value_type: SettingType,
    pub default: Value,
    #[serde(default)]
    pub scope: Scope,
    #[serde(default)]
    pub apply: Apply,
}
impl Definition {
    /// Bound schema-driven UI allocations independently of the package archive budget.
    pub fn valid(&self) -> bool {
        !self.title.is_empty()
            && self.title.len() <= 256
            && self.accepts(&self.default)
            && match &self.value_type {
                SettingType::Boolean => true,
                SettingType::String { max_length } => *max_length <= 4096,
                SettingType::Integer { min, max } => min <= max,
                SettingType::Enum { choices } => {
                    !choices.is_empty()
                        && choices.len() <= 32
                        && choices
                            .iter()
                            .all(|choice| !choice.is_empty() && choice.len() <= 128)
                        && choices
                            .iter()
                            .collect::<std::collections::BTreeSet<_>>()
                            .len()
                            == choices.len()
                }
            }
    }
    /// Explicit invalid values must fail, rather than falling through to discovered or default values.
    pub fn accepts(&self, value: &Value) -> bool {
        match &self.value_type {
            SettingType::Boolean => value.is_boolean(),
            SettingType::String { max_length } => {
                value.as_str().is_some_and(|text| text.len() <= *max_length)
            }
            SettingType::Integer { min, max } => value
                .as_i64()
                .is_some_and(|number| number >= *min && number <= *max),
            SettingType::Enum { choices } => value
                .as_str()
                .is_some_and(|text| choices.iter().any(|choice| choice == text)),
        }
    }
}

/// Provenance accompanies effective values so the UI never confuses an override with a default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Default,
    Discovered,
    User,
    Project,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectiveValue {
    pub value: Value,
    pub source: Source,
}
pub type Effective = BTreeMap<String, EffectiveValue>;

/// Validation is side-effect free; apply delivers the fully resolved values before activation.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Validate,
    Apply,
}

/// A hook may discover only unspecified values and report errors against explicit settings.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    #[serde(default)]
    pub discovered: BTreeMap<String, Value>,
    #[serde(default)]
    pub errors: BTreeMap<String, String>,
}
