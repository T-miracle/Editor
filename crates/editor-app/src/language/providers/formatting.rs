//! Finite verification of explicit formatter values separates configuration errors from normal withdrawal.
use super::*;
use serde::{Deserialize, Serialize};
#[cfg(test)]
mod tests;

/// Each scope/key retains only its current verified value, bounded by the existing preference file quota.
/// No list of historical packages is retained, and an automatic fallback never overwrites this proof.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct ValidatedFormatterChoices {
    user: BTreeMap<String, String>,
    projects: BTreeMap<String, BTreeMap<String, String>>,
}

impl ValidatedFormatterChoices {
    /// Clear deleted or manually changed entries, so proofs cannot outlive their explicit setting.
    pub(super) fn prune(
        &mut self,
        user: &BTreeMap<String, String>,
        projects: &BTreeMap<String, BTreeMap<String, String>>,
    ) {
        self.user
            .retain(|key, id| key.starts_with("formatter:") && user.get(key) == Some(id));
        self.projects.retain(|workspace, choices| {
            let Some(explicit) = projects.get(workspace) else {
                return false;
            };
            choices
                .retain(|key, id| key.starts_with("formatter:") && explicit.get(key) == Some(id));
            !choices.is_empty()
        });
    }

    /// Called only after a live candidate check, or to remove the corresponding scope's explicit value.
    pub(super) fn record(
        &mut self,
        scope: Scope,
        workspace: &str,
        key: &str,
        provider: Option<&str>,
    ) {
        if !key.starts_with("formatter:") {
            return;
        }
        let layer = match scope {
            Scope::User => &mut self.user,
            Scope::Project => self.projects.entry(workspace.into()).or_default(),
        };
        if let Some(provider) = provider {
            layer.insert(key.into(), provider.into());
        } else {
            layer.remove(key);
        }
        self.projects.retain(|_, choices| !choices.is_empty());
    }
}

/// The original explicit ID and its effective scope are visible even when no formatter is selected.
#[derive(Clone, Debug)]
pub(crate) struct FormatterPreferenceError {
    pub provider: String,
    pub source: &'static str,
}

impl FormatterPreferenceError {
    /// Settings and native commands share the same localized error; it never labels a typo automatic.
    pub(crate) fn message(&self) -> String {
        rust_i18n::t!(
            "settings.provider_invalid",
            provider = &self.provider,
            source = rust_i18n::t!(format!("settings.provider_scope_{}", self.source))
        )
        .to_string()
    }
}

impl Registry {
    /// Reject unknown explicit formatters without changing recognition, grammar or main LSP rules.
    /// A previously verified missing value is normal withdrawal and continues through generic fallback.
    pub(super) fn validate_formatter_explicit(&mut self, key: &str, candidates: &[String]) -> bool {
        if !key.starts_with("formatter:") {
            return true;
        }
        // Record both layers when their values are actual candidates, including a shadowed user choice.
        let user = self.saved.user.get(key).cloned();
        let project = self
            .saved
            .projects
            .get(&self.workspace)
            .and_then(|layer| layer.get(key))
            .cloned();
        for (scope, explicit) in [(Scope::User, &user), (Scope::Project, &project)] {
            if let Some(id) = explicit.as_ref().filter(|id| candidates.contains(id)) {
                self.saved
                    .formatter_choices
                    .record(scope, &self.workspace, key, Some(id));
            }
        }
        let (explicit, verified, source) = if let Some(id) = project {
            (
                id,
                self.saved
                    .formatter_choices
                    .projects
                    .get(&self.workspace)
                    .and_then(|layer| layer.get(key)),
                "project",
            )
        } else if let Some(id) = user {
            (id, self.saved.formatter_choices.user.get(key), "user")
        } else {
            return true;
        };
        if candidates.contains(&explicit) || verified == Some(&explicit) {
            return true;
        }
        self.formatter_preference_errors.insert(
            key.into(),
            FormatterPreferenceError {
                provider: explicit,
                source,
            },
        );
        false
    }
}

/// Consult current registry policy before using a cached formatter, including a freshly reloaded store.
pub(crate) fn formatter_error(language: &str) -> Option<FormatterPreferenceError> {
    REGISTRY
        .read()
        .unwrap()
        .formatter_preference_errors
        .get(&format!("formatter:{language}"))
        .cloned()
}

/// One selected formatter never replaces the independent primary analysis service.
pub(crate) fn formatters() -> BTreeMap<String, Option<String>> {
    let registry = REGISTRY.read().unwrap();
    registry
        .formatters
        .keys()
        .map(|language| {
            (
                language.clone(),
                registry
                    .selected
                    .get(&format!("formatter:{language}"))
                    .cloned(),
            )
        })
        .collect()
}
