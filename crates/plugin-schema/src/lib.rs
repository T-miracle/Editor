use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginManifest {
    pub plugin: PluginMetadata,
    #[serde(default)]
    pub languages: Vec<LanguageContribution>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginMetadata {
    pub id: String,
    pub name: String,
    pub version: String,
    pub host_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LanguageContribution {
    pub id: String,
    pub extensions: Vec<String>,
    pub grammar: PathBuf,
    pub highlights: PathBuf,
    #[serde(default)]
    pub lsp_command: Option<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("invalid TOML: {0}")]
    Parse(String),
    #[error("plugin id must contain only lowercase ASCII letters, digits, dots, or hyphens")]
    InvalidPluginId,
    #[error("language {0} does not declare an extension")]
    MissingExtension(String),
    #[error("language {0} uses an absolute plugin path")]
    AbsolutePath(String),
}

impl PluginManifest {
    pub fn parse(source: &str) -> Result<Self, ManifestError> {
        let manifest = toml::from_str::<Self>(source)
            .map_err(|error| ManifestError::Parse(error.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.plugin.id.is_empty()
            || !self.plugin.id.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
            })
        {
            return Err(ManifestError::InvalidPluginId);
        }

        for language in &self.languages {
            if language.extensions.is_empty() {
                return Err(ManifestError::MissingExtension(language.id.clone()));
            }
            if language.grammar.is_absolute() || language.highlights.is_absolute() {
                return Err(ManifestError::AbsolutePath(language.id.clone()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_declarative_language_plugin() {
        let manifest = PluginManifest::parse(
            r#"
                [plugin]
                id = "me.rust"
                name = "Rust"
                version = "0.1.0"
                host_version = ">=0.1.0"

                [[languages]]
                id = "rust"
                extensions = ["rs"]
                grammar = "grammar/rust.wasm"
                highlights = "queries/highlights.scm"
                lsp_command = "rust-analyzer"
            "#,
        )
        .unwrap();

        assert_eq!(manifest.plugin.id, "me.rust");
        assert_eq!(manifest.languages[0].extensions, ["rs"]);
    }
}
