//! Host-owned identity around opaque plugin values; executable projections never authorize a run.
use serde::{Deserialize, Serialize};

/// Last visible business result. Execution always obtains a new receipt, including after a restart.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "reason", rename_all = "snake_case")]
pub enum ConfigurationValidation {
    #[default]
    Unchecked,
    Valid,
    Invalid(String),
    Unavailable(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RunConfigSet, RunTarget};

    /// Opaque native values and stable provider associations survive local storage and independent copies.
    #[test]
    fn plugin_configuration_storage_retains_values_and_removes_only_the_selected_identity() {
        let mut set = RunConfigSet::default();
        let mut first: crate::RunConfig = serde_json::from_value(serde_json::json!({
            "id":"one","name":"same name","target":{"mode":"program","program":"probe","args":["two words", "\"中文\" ; &"]}
        })).unwrap();
        let data = PluginConfiguration {
            provider: "independent-provider".into(),
            template: "program".into(),
            values: r#"{"arguments":["two words","\"中文\" ; &"],"custom":{"nested":true}}"#.into(),
            name: first.name.clone(),
            program: "probe".into(),
            pending_events: vec![r#"{"node":"name","text":"尚未确认的输入"}"#.into()],
            revision: 7,
            validation: ConfigurationValidation::Invalid("plugin reason".into()),
        };
        set.upsert(first.clone()).unwrap();
        set.plugin_configurations
            .insert(first.id.clone(), data.clone());
        first.id = "two".into();
        set.upsert(first).unwrap();
        set.plugin_configurations.insert("two".into(), data.clone());
        let mut restored = RunConfigSet::from_json(&set.to_json().unwrap()).unwrap();
        assert_eq!(restored.plugin_configurations["one"], data);
        assert!(matches!(
            restored.find("two").unwrap().target,
            RunTarget::Program { .. }
        ));
        assert!(restored.remove("one"));
        assert_eq!(restored.plugin_configurations["two"].values, data.values);
        assert!(!restored.plugin_configurations.contains_key("one"));
        restored.plugin_configurations.insert("orphan".into(), data);
        assert!(
            RunConfigSet::from_json(&restored.to_json().unwrap()).is_err(),
            "unowned metadata is not a configuration"
        );
    }
}

/// One template instance, owned by a configuration identity rather than the provider's template ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginConfiguration {
    pub provider: String,
    pub template: String,
    /// Canonical plugin-owned JSON. The host stores but never parses command-specific fields.
    pub values: String,
    /// Ordered opaque form events not yet acknowledged by the provider. Faults must not erase input.
    /// Reopening replays these events before validation; the host never interprets their business data.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_events: Vec<String>,
    pub name: String,
    pub program: String,
    /// Every accepted native edit invalidates older validation and executable preparation receipts.
    pub revision: u64,
    #[serde(default)]
    pub validation: ConfigurationValidation,
}

impl PluginConfiguration {
    /// Only generic storage bounds are checked here; the provider owns business validation.
    pub fn storage_valid(&self) -> bool {
        !self.provider.is_empty()
            && self.provider.len() <= 256
            && !self.template.is_empty()
            && self.template.len() <= 256
            && self.name.len() <= 256
            && self.program.len() <= 4096
            && !self.program.is_empty()
            && !self.program.contains('\0')
            && self.values.len() <= 16 * 1024
            && serde_json::from_str::<serde_json::Value>(&self.values).is_ok()
            && self.pending_events.len() <= 512
            && self.pending_events.iter().map(String::len).sum::<usize>() <= 64 * 1024
            && self
                .pending_events
                .iter()
                .all(|event| serde_json::from_str::<serde_json::Value>(event).is_ok())
    }
}
