//! Persist host user policy and optional per-language overrides without plugin configuration authority.
use super::{REGISTRY, preferences::EditingPreferences};

/// Missing language overrides inherit the user-wide values; first use requires no settings file.
pub(crate) fn editing_preferences(language: &str) -> EditingPreferences {
    let registry = REGISTRY.read().unwrap();
    let mut effective = EditingPreferences::default();
    for override_value in registry
        .saved
        .editing
        .get("*")
        .into_iter()
        .chain(registry.saved.editing.get(language))
    {
        if let Some(value) = override_value.format_on_save {
            effective.format_on_save = value;
        }
        if let Some(value) = override_value.linked_editing {
            effective.linked_editing = value;
        }
    }
    effective
}

/// Settings can show which language values currently override the user's global preferences.
pub(crate) fn has_editing_override(language: &str) -> bool {
    REGISTRY
        .read()
        .unwrap()
        .saved
        .editing
        .contains_key(language)
}

/// Change one native preference after persistence succeeds; `None` removes a language override.
/// `format_on_save` selects which setting changes. This grants no process or workspace authority.
pub(crate) fn set_editing_preference(
    language: &str,
    format_on_save: bool,
    enabled: Option<bool>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !language.is_empty() && language.len() <= 128,
        "Invalid preference language"
    );
    let mut registry = REGISTRY.write().unwrap();
    anyhow::ensure!(
        registry.error.is_none(),
        "Language preferences are unavailable"
    );
    let mut saved = registry.saved.clone();
    if let Some(enabled) = enabled {
        let value = saved.editing.entry(language.into()).or_default();
        if format_on_save {
            value.format_on_save = Some(enabled);
        } else {
            value.linked_editing = Some(enabled);
        }
    } else {
        saved.editing.remove(language);
    }
    anyhow::ensure!(
        saved.editing.len() <= 512,
        "Too many language editing overrides"
    );
    saved.write(&registry.root)?;
    registry.saved = saved;
    Ok(())
}
