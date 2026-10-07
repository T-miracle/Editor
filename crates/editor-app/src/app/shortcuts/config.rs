//! Stores user overrides separately from package defaults and workspace data.

use editor_core::DocumentStore;
use gpui_kit::Keystroke;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};

/// A sequence contains one or two canonical GPUI keystroke strings.
pub(super) type Sequence = Vec<String>;

/// Missing entries inherit defaults; an explicit empty list disables all bindings.
/// Unknown operation IDs survive package removal and later installation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UserBindings {
    schema: u32,
    pub(super) overrides: BTreeMap<String, Vec<Sequence>>,
}

impl Default for UserBindings {
    fn default() -> Self {
        Self {
            schema: 1,
            overrides: BTreeMap::new(),
        }
    }
}

impl UserBindings {
    /// Read a user profile without silently replacing unreadable or newer data.
    /// Only a missing file means that the user has not configured bindings yet.
    pub(super) fn load(path: &Path) -> Result<Self, String> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(format!("{}: {error}", path.display())),
        };
        let mut config: Self = serde_json::from_slice(&bytes)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        if config.schema != 1 {
            return Err(format!(
                "{}: unsupported shortcut schema {}",
                path.display(),
                config.schema
            ));
        }
        for (id, bindings) in &mut config.overrides {
            // Explicit conflict replacement can retain untouched upstream defaults in an
            // override. Reload must preserve such keys; only newly edited drafts restrict text.
            *bindings = normalize(bindings, false)
                .map_err(|error| format!("{}: {id}: {error:?}", path.display()))?;
        }
        Ok(config)
    }

    /// Use the existing native atomic replacement so failure leaves the prior file intact.
    pub(super) fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        let contents = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
        platform_windows::NativeFileStore
            .write_utf8(path, &contents)
            .map_err(|error| error.to_string())
    }
}

/// Structured validation lets the UI translate errors without parsing diagnostic strings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum InvalidBinding {
    /// The sequence is empty or contains more than the supported two steps.
    StepCount,
    /// GPUI cannot parse this step; the string is safe to show in the draft row.
    Keystroke(String),
    /// Modifiers alone cannot act as a sequence step.
    ModifierOnly(String),
    /// Letters and digits need a modifier beyond Shift to preserve normal typing.
    TextNeedsModifier(String),
    /// Plain Escape belongs to cancellation; existing upstream defaults may retain it.
    ReservedEscape,
    /// One binding in the same operation consumes another binding's first step.
    PrefixOverlap,
}

/// Canonicalize complete candidates, retaining order and removing exact duplicates.
/// Restoring upstream defaults may bypass the new-text-key restriction, never syntax checks.
pub(super) fn normalize(
    bindings: &[Sequence],
    restrict_text: bool,
) -> Result<Vec<Sequence>, InvalidBinding> {
    let mut result = Vec::<Sequence>::new();
    for sequence in bindings {
        if !(1..=2).contains(&sequence.len()) {
            return Err(InvalidBinding::StepCount);
        }
        let mut canonical = Vec::with_capacity(sequence.len());
        for source in sequence {
            let key =
                Keystroke::parse(source).map_err(|_| InvalidBinding::Keystroke(source.clone()))?;
            if restrict_text
                && matches!(key.key.as_str(), "escape" | "esc")
                && key.modifiers == Default::default()
            {
                return Err(InvalidBinding::ReservedEscape);
            }
            if key.key.is_empty()
                || matches!(
                    key.key.as_str(),
                    "shift"
                        | "control"
                        | "ctrl"
                        | "alt"
                        | "cmd"
                        | "platform"
                        | "super"
                        | "fn"
                        | "function"
                )
            {
                return Err(InvalidBinding::ModifierOnly(source.clone()));
            }
            let mut letters = key.key.chars();
            let textual =
                letters.next().is_some_and(char::is_alphanumeric) && letters.next().is_none();
            if restrict_text
                && textual
                && !(key.modifiers.control
                    || key.modifiers.alt
                    || key.modifiers.platform
                    || key.modifiers.function)
            {
                return Err(InvalidBinding::TextNeedsModifier(source.clone()));
            }
            canonical.push(key.unparse());
        }
        if !result.contains(&canonical) {
            if result
                .iter()
                .any(|existing| prefixes_overlap(existing, &canonical))
            {
                return Err(InvalidBinding::PrefixOverlap);
            }
            result.push(canonical);
        }
    }
    Ok(result)
}

/// A shared first step conflicts only when one sequence ends at that step.
pub(super) fn prefixes_overlap(left: &[String], right: &[String]) -> bool {
    !left.is_empty()
        && !right.is_empty()
        && left.iter().zip(right).all(|(left, right)| left == right)
}
