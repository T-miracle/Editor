//! Compares bindings using GPUI predicates and source-verified focus paths.

use super::{catalog::Target, config::Sequence};
use gpui_kit::{KeyBindingContextPredicate as Predicate, KeyContext};

/// Reports the exact binding to remove after the user explicitly chooses replacement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Conflict {
    pub(super) operation: String,
    pub(super) title: String,
    pub(super) binding: Sequence,
}

/// Focus paths are topology metadata, not names guessed from actions or plugin IDs.
/// Additional observations can prove overlap; only audited predicates prove disjointness.
pub(super) struct ContextProfiles {
    profiles: Vec<Vec<KeyContext>>,
}

impl Default for ContextProfiles {
    fn default() -> Self {
        // EditorDockPanel does not add a context. Explorer rename is a shell sibling,
        // whereas the run-configuration tree actually embeds its rename Input in Tree.
        // PluginSurface encloses package-native Input/Textarea/editor and button controls.
        let paths: &[&[&str]] = &[
            &["Root"],
            &["Root", "EditorShell"],
            &["Root", "Input"],
            &["Root", "Tree"],
            &["Root", "Tree", "Input"],
            &["Root", "TextView"],
            &["Root", "EditorActivate"],
            &["Root", "EditorShell", "Input"],
            &["Root", "EditorShell", "Tree"],
            &["Root", "EditorShell", "TextView"],
            &["Root", "EditorShell", "EditorActivate"],
            &["Root", "EditorShell", "PluginSurface"],
            &["Root", "EditorShell", "PluginSurface", "Input"],
            &["Root", "EditorShell", "PluginSurface", "TextView"],
            &["Root", "EditorShell", "PluginSurface", "EditorActivate"],
            &["Root", "EditorShell", "ShortcutPanel"],
            &["Root", "EditorShell", "ShortcutPanel", "Input"],
            &["Root", "EditorShell", "ShortcutPanel", "EditorActivate"],
        ];
        Self {
            profiles: paths
                .iter()
                .map(|path| {
                    path.iter()
                        .map(|name| KeyContext::parse(name).expect("audited key context"))
                        .collect()
                })
                .collect(),
        }
    }
}

impl ContextProfiles {
    /// Remember a real captured stack; its presence can only strengthen conflict detection.
    pub(super) fn observe(&mut self, contexts: &[KeyContext]) -> bool {
        if contexts.is_empty() || self.profiles.iter().any(|profile| profile == contexts) {
            return false;
        }
        self.profiles.push(contexts.to_vec());
        true
    }

    /// Reject ambiguous unknown contexts instead of assuming unrelated names are exclusive.
    /// GPUI handles stack-wide negation itself; Input and Tree intentionally overlap.
    pub(super) fn overlap(&self, left: &Target, right: &Target) -> bool {
        let left = predicate(left);
        let right = predicate(right);
        if self
            .profiles
            .iter()
            .any(|profile| matches(left.as_ref(), profile) && matches(right.as_ref(), profile))
        {
            return true;
        }
        !(audited(left.as_ref()) && audited(right.as_ref()))
    }
}

/// A manifest command currently has the same shell scope as its public invocation route.
fn predicate(target: &Target) -> Option<Predicate> {
    match target {
        Target::Native { predicate, .. } => predicate.as_deref().cloned(),
        Target::Plugin { .. } => Some(Predicate::Identifier("EditorShell".into())),
    }
}

/// Depth matching respects both ancestors and GPUI's whole-stack Not semantics.
fn matches(predicate: Option<&Predicate>, contexts: &[KeyContext]) -> bool {
    predicate.is_none_or(|predicate| predicate.depth_of(contexts).is_some())
}

/// Complex predicates remain conservative until their reachable topology is explicitly audited.
/// Merely observing one such context cannot prove that every other context is impossible.
fn audited(predicate: Option<&Predicate>) -> bool {
    match predicate {
        None => true,
        Some(Predicate::Identifier(name)) => audited_name(name.as_ref()),
        Some(Predicate::Not(inner)) => {
            matches!(inner.as_ref(), Predicate::Identifier(name) if audited_name(name.as_ref()))
        }
        Some(Predicate::And(left, right) | Predicate::Or(left, right)) => {
            audited(Some(left)) && audited(Some(right))
        }
        Some(_) => false,
    }
}

/// These names come from the actual local roots and their upstream leaf control contracts.
fn audited_name(name: &str) -> bool {
    matches!(
        name,
        "Root"
            | "EditorShell"
            | "PluginSurface"
            | "Input"
            | "Tree"
            | "TextView"
            | "EditorActivate"
            | "ShortcutPanel"
    )
}
