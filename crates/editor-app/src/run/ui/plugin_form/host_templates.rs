//! Built-in configuration contributors use the same form envelopes and validation projection as guests.

use super::*;

/// Reserved built-in identities are host-owned features, not package IDs or plugin permission shortcuts.
pub(super) fn owns(provider: &str) -> bool {
    matches!(
        provider,
        crate::plugin_development::configuration::PROVIDER
            | crate::terminal::configurations::PROVIDER
    )
}

pub(super) fn templates(workspace: &str) -> Vec<(String, contract::Template)> {
    let mut templates = crate::plugin_development::configuration::templates(workspace);
    templates.extend(crate::terminal::configurations::templates(workspace));
    templates
}

/// Dispatch only pure form/validation policy; process admission remains in the run coordinator.
pub(super) fn invoke(
    provider: &str,
    method: &str,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    if provider == crate::terminal::configurations::PROVIDER {
        crate::terminal::configurations::invoke(method, arguments).map_err(|error| error.message)
    } else if provider == crate::plugin_development::configuration::PROVIDER {
        match method {
            "form" => crate::plugin_development::configuration::form(arguments),
            "validate" => crate::plugin_development::configuration::validate(arguments),
            _ => Err(t!("run.plugin_provider_unavailable").to_string()),
        }
    } else {
        Err(t!("run.plugin_provider_unavailable").to_string())
    }
}
