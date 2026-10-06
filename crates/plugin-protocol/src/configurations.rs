//! Plugin-owned command templates, native forms and validation over public versioned services.
//! The host owns configuration identity and storage; executable policy belongs to each provider.

use serde::{Deserialize, Serialize};
use serde_json::json;

/// Every compatible plugin can provide this contract; provider identity comes from authenticated routing.
pub const CONTRACT: &str = "run.configurations";
/// Opaque editable data is bounded independently from the native view's drawing budget.
pub const MAX_VALUES_BYTES: usize = 16 * 1024;

/// Exact service signatures consumed by the host and independently built plugins.
/// Methods return bounded JSON envelopes so native UI and launch types retain their existing versions.
pub fn declaration() -> crate::service::Contract {
    let text = |max| json!({"type":"string", "max_bytes":max});
    let common = json!({"workspace":text(4096), "locale":text(64), "os":text(64)});
    let method = |fields: serde_json::Value, maximum, permissions: Vec<&str>| {
        json!({"parameters":{"type":"record","fields":fields},
            "result":{"type":"record","fields":{"payload":text(maximum)}},
            "permissions":permissions})
    };
    let mut form = common.clone();
    form["template"] = text(256);
    form["values"] = text(MAX_VALUES_BYTES);
    form["event"] = text(64 * 1024);
    let mut validation = common.clone();
    validation["template"] = text(256);
    validation["values"] = text(MAX_VALUES_BYTES);
    // The same editable command can need different preparation for Save, Run, Build or Debug.
    // Policy stays in the provider rather than in executable-name branches in the host.
    validation["intent"] = text(64);
    serde_json::from_value(json!({"version":"1.0.0","methods":{
        // Tool availability is discovered through the same permission-checked resolver as execution.
        "catalog":method(common, 64 * 1024, vec!["workspace.read", "process.exec"]),
        "form":method(form, 64 * 1024, vec!["workspace.read", "ui.panels"]),
        "validate":method(validation, 64 * 1024, vec!["workspace.read", "process.exec"])
    }}))
    .expect("published run.configurations signatures")
}

/// One reusable template, distinct from an instantiated configuration or an active session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Template {
    pub id: String,
    pub group: String,
    pub label: String,
    /// Self-contained SVG (up to 16 KiB) or stock code, terminal, play, build or debug artwork.
    /// No external resources are fetched; unknown names use the ordinary command icon.
    pub icon: String,
    /// Provider-defined JSON defaults; the host preserves this string without interpreting its fields.
    pub defaults: String,
    /// Ordinary unavailable templates remain visible; environment-specific providers may omit entries.
    #[serde(default)]
    pub unavailable: Option<String>,
}

/// Catalog order belongs to the provider; hidden environment-specific entries are not returned.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub templates: Vec<Template>,
}

/// Canonical values and a provider-owned native layout after creation or one serialized UI event.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Form {
    pub values: String,
    pub name: String,
    /// Display-only executable identity; edits are sent through provider-owned native controls.
    pub program: String,
    pub document: crate::ui::Document,
}

/// Plugin business validation cannot grant execution permissions or replace host configuration identity.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Validation {
    pub valid: bool,
    pub message: String,
    /// A valid receipt contains structured launch data, checked again by the existing launch boundary.
    pub launch: Option<Launch>,
}

/// Structured configuration preparation; JSON field spelling matches the documented target format.
/// No Shell string is inferred from arguments, and permissions are never included in this value.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    pub target: serde_json::Value,
    #[serde(default)]
    pub directory: Option<String>,
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub tool_paths: Vec<String>,
    #[serde(default)]
    pub build: Vec<serde_json::Value>,
    #[serde(default)]
    pub prelaunch: Vec<serde_json::Value>,
    #[serde(default)]
    pub provider: Option<String>,
}

/// Decode one public envelope; callers additionally enforce native UI and configuration quotas.
pub fn decode<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T, String> {
    let payload = value["payload"]
        .as_str()
        .ok_or("Missing configuration payload")?;
    serde_json::from_str(payload).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Independent consumers negotiate identical signatures, including the complete editable value budget.
    #[test]
    fn configuration_contract_and_envelopes_are_versioned() {
        let contract = declaration();
        crate::service::Declarations {
            provides: [(CONTRACT.into(), contract.clone())].into(),
            requires: Default::default(),
        }
        .validate()
        .unwrap();
        assert_eq!(contract.methods.len(), 3);
        assert!(contract.methods.contains_key("form"));
        let catalog: Catalog = decode(json!({"payload":"{\"templates\":[]}"})).unwrap();
        assert!(catalog.templates.is_empty());
        assert!(
            decode::<Catalog>(json!({"payload":"{\"templates\":[],\"unexpected\":1}"})).is_err()
        );
        assert!(decode::<Validation>(json!({"payload":"broken"})).is_err());
    }
}
