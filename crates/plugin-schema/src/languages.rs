//! Language identities and grammar providers are independent, bounded declarations.
use super::{ManifestError, PluginManifest};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LanguageDefinition {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub filenames: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Highlighter {
    pub id: String,
    pub language: String,
    /// Exported Tree-sitter name is independent of the editor's language identity.
    pub grammar_name: String,
    pub grammar: PathBuf,
    pub highlights: PathBuf,
    /// Optional Tree-sitter query composing other explicitly selected WASM grammar providers.
    #[serde(default)]
    pub injections: Option<PathBuf>,
    /// Only these language identities may be resolved by this provider's injection query.
    #[serde(default)]
    pub injection_languages: Vec<String>,
    pub tree_sitter_abi: u32,
}

/// Reject unsafe names and excessive declaration lists before installing third-party packages.
pub(super) fn validate(manifest: &PluginManifest) -> Result<(), ManifestError> {
    let invalid = || ManifestError::Parse("Invalid dynamic language declaration".into());
    let id = |s: &str| {
        !s.is_empty()
            && s.len() <= 100
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
    };
    let selector = |s: &str| {
        !s.is_empty()
            && s.len() <= 128
            && s != "."
            && s != ".."
            && !s.contains(['/', '\\', '*', '?', ':', '[', ']'])
    };
    if manifest.language_definitions.len() > 64 || manifest.highlighters.len() > 64 {
        return Err(invalid());
    }
    let mut ids = BTreeSet::new();
    for language in &manifest.language_definitions {
        let distinct = |values: &[String]| {
            values
                .iter()
                .map(|value| value.to_lowercase())
                .collect::<BTreeSet<_>>()
                .len()
                == values.len()
        };
        if !id(&language.id)
            || language.id == "text"
            || !ids.insert(&language.id)
            || language.name.is_empty()
            || language.name.len() > 128
            || language.extensions.len() + language.filenames.len() > 128
            || language.extensions.is_empty() && language.filenames.is_empty()
            || !language
                .extensions
                .iter()
                .all(|s| selector(s) && !s.contains('.'))
            || !language.filenames.iter().all(|s| selector(s))
            || !distinct(&language.extensions)
            || !distinct(&language.filenames)
        {
            return Err(invalid());
        }
    }
    ids.clear();
    for provider in &manifest.highlighters {
        if !id(&provider.id)
            || provider.language == "text"
            || !id(&provider.language)
            || !id(&provider.grammar_name)
            || !ids.insert(&provider.id)
            || !(14..=15).contains(&provider.tree_sitter_abi)
        {
            return Err(invalid());
        }
        for path in [&provider.grammar, &provider.highlights] {
            super::validate_relative_path(path).map_err(|_| invalid())?;
        }
        if let Some(path) = &provider.injections {
            super::validate_relative_path(path).map_err(|_| invalid())?;
        }
        if provider.injection_languages.len() > 64
            || !provider
                .injection_languages
                .iter()
                .all(|name| id(name) && name != "text")
            || provider.injections.is_none() && !provider.injection_languages.is_empty()
        {
            return Err(invalid());
        }
    }
    Ok(())
}
