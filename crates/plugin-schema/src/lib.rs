use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginManifest {
    pub plugin: PluginMetadata,
    #[serde(default)]
    pub languages: Vec<LanguageContribution>,
    #[serde(default)]
    pub theme: Option<ThemeContribution>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ThemeContribution {
    /// A JSON theme file, relative to the plugin root.
    pub file: PathBuf,
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

/// A versioned JSON file containing one or more named editor themes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ThemeFile {
    pub schema_version: u32,
    pub themes: Vec<ThemeDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ThemeDefinition {
    pub id: String,
    pub name: String,
    pub mode: ThemeMode,
    pub colors: ThemeColors,
    #[serde(default)]
    pub components: BTreeMap<ThemeComponent, ComponentStyles>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    Light,
    Dark,
}

/// Component identifiers are part of the theme file's stable host contract.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeComponent {
    AppShell,
    WindowTitleBar,
    ProjectBadge,
    ExplorerTree,
    ExplorerRow,
    EditorTabs,
    EditorTab,
    EditorTabClose,
    EditorTabDragPreview,
    Editor,
    Scrollbar,
    OutputPanel,
    PanelToggle,
    DockTitleBar,
    DockTab,
    DockDragPreview,
    StatusBar,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ComponentStyles {
    pub base: StyleProperties,
    pub hover: Option<StyleProperties>,
    pub selected: Option<StyleProperties>,
    pub active: Option<StyleProperties>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct StyleProperties {
    pub background: Option<String>,
    pub foreground: Option<String>,
    pub border: Option<String>,
    pub radius_px: Option<f32>,
    pub font_size_px: Option<f32>,
    pub padding_x_px: Option<f32>,
    pub padding_y_px: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ThemeColors {
    pub background: String,
    pub surface: String,
    pub hover: String,
    pub border: String,
    pub foreground: String,
    pub muted_foreground: String,
    pub selection: String,
    pub accent: String,
    pub accent_hover: String,
    pub accent_active: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("invalid TOML: {0}")]
    Parse(String),
    #[error("plugin id must contain only lowercase ASCII letters, digits, dots, or hyphens")]
    InvalidPluginId,
    #[error("language {0} does not declare an extension")]
    MissingExtension(String),
    #[error("plugin asset path is absolute: {0}")]
    AbsolutePath(String),
    #[error("plugin asset path must stay inside the plugin directory: {0}")]
    UnsafeAssetPath(String),
    #[error("theme file path is empty")]
    EmptyThemePath,
}

#[derive(Debug, Error, PartialEq)]
pub enum ThemeFileError {
    #[error("invalid JSON: {0}")]
    Parse(String),
    #[error("unsupported theme schema version {0}")]
    UnsupportedSchemaVersion(u32),
    #[error("theme file must contain at least one theme")]
    Empty,
    #[error("theme id must contain only lowercase ASCII letters, digits, dots, or hyphens: {0}")]
    InvalidThemeId(String),
    #[error("theme id is duplicated: {0}")]
    DuplicateThemeId(String),
    #[error("theme name cannot be empty for {0}")]
    EmptyThemeName(String),
    #[error("invalid color {1} at {0}; expected #RRGGBB")]
    InvalidColor(String, String),
    #[error("invalid metric {1} at {0}; expected a finite value from 0 to 128 pixels")]
    InvalidMetric(String, f32),
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
            for path in [&language.grammar, &language.highlights] {
                if path.components().any(|component| {
                    matches!(
                        component,
                        std::path::Component::ParentDir
                            | std::path::Component::RootDir
                            | std::path::Component::Prefix(_)
                    )
                }) {
                    return Err(ManifestError::UnsafeAssetPath(
                        path.to_string_lossy().into_owned(),
                    ));
                }
            }
        }
        if let Some(theme) = &self.theme {
            if theme.file.as_os_str().is_empty() {
                return Err(ManifestError::EmptyThemePath);
            }
            if theme.file.is_absolute() {
                return Err(ManifestError::AbsolutePath(
                    theme.file.to_string_lossy().into_owned(),
                ));
            }
            if theme.file.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            }) {
                return Err(ManifestError::UnsafeAssetPath(
                    theme.file.to_string_lossy().into_owned(),
                ));
            }
        }
        Ok(())
    }
}

impl ThemeFile {
    pub fn parse(source: &str) -> Result<Self, ThemeFileError> {
        let file = serde_json::from_str::<Self>(source)
            .map_err(|error| ThemeFileError::Parse(error.to_string()))?;
        file.validate()?;
        Ok(file)
    }

    pub fn validate(&self) -> Result<(), ThemeFileError> {
        if self.schema_version != 1 {
            return Err(ThemeFileError::UnsupportedSchemaVersion(
                self.schema_version,
            ));
        }
        if self.themes.is_empty() {
            return Err(ThemeFileError::Empty);
        }
        let mut ids = std::collections::BTreeSet::new();
        for theme in &self.themes {
            if theme.id.is_empty()
                || !theme.id.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'.' | b'-')
                })
            {
                return Err(ThemeFileError::InvalidThemeId(theme.id.clone()));
            }
            if !ids.insert(theme.id.as_str()) {
                return Err(ThemeFileError::DuplicateThemeId(theme.id.clone()));
            }
            if theme.name.trim().is_empty() {
                return Err(ThemeFileError::EmptyThemeName(theme.id.clone()));
            }
            validate_palette_colors(theme)?;
            validate_component_metrics(theme)?;
        }
        Ok(())
    }
}

fn validate_palette_colors(theme: &ThemeDefinition) -> Result<(), ThemeFileError> {
    let mut colors: Vec<(String, &str)> = vec![
        ("colors.background".into(), &theme.colors.background),
        ("colors.surface".into(), &theme.colors.surface),
        ("colors.hover".into(), &theme.colors.hover),
        ("colors.border".into(), &theme.colors.border),
        ("colors.foreground".into(), &theme.colors.foreground),
        (
            "colors.muted_foreground".into(),
            &theme.colors.muted_foreground,
        ),
        ("colors.selection".into(), &theme.colors.selection),
        ("colors.accent".into(), &theme.colors.accent),
        ("colors.accent_hover".into(), &theme.colors.accent_hover),
        ("colors.accent_active".into(), &theme.colors.accent_active),
    ];
    for (component, styles) in &theme.components {
        let prefix = format!("components.{}", component.as_str());
        push_style_colors(&mut colors, &prefix, &styles.base);
        for (state_name, state) in [
            ("hover", styles.hover.as_ref()),
            ("selected", styles.selected.as_ref()),
            ("active", styles.active.as_ref()),
        ] {
            if let Some(state) = state {
                push_style_colors(&mut colors, &format!("{prefix}.{state_name}"), state);
            }
        }
    }
    for (path, value) in colors {
        if !is_hex_color(value) {
            return Err(ThemeFileError::InvalidColor(path, value.to_owned()));
        }
    }
    Ok(())
}

fn push_style_colors<'a>(
    colors: &mut Vec<(String, &'a str)>,
    prefix: &str,
    style: &'a StyleProperties,
) {
    if let Some(value) = &style.background {
        colors.push((format!("{prefix}.background"), value));
    }
    if let Some(value) = &style.foreground {
        colors.push((format!("{prefix}.foreground"), value));
    }
    if let Some(value) = &style.border {
        colors.push((format!("{prefix}.border"), value));
    }
}

fn validate_component_metrics(theme: &ThemeDefinition) -> Result<(), ThemeFileError> {
    for (component, styles) in &theme.components {
        let prefix = format!("components.{}", component.as_str());
        validate_style_metrics(&prefix, &styles.base)?;
        for (state_name, state) in [
            ("hover", styles.hover.as_ref()),
            ("selected", styles.selected.as_ref()),
            ("active", styles.active.as_ref()),
        ] {
            if let Some(state) = state {
                validate_style_metrics(&format!("{prefix}.{state_name}"), state)?;
            }
        }
    }
    Ok(())
}

fn validate_style_metrics(prefix: &str, style: &StyleProperties) -> Result<(), ThemeFileError> {
    for (name, value) in [
        ("radius_px", style.radius_px),
        ("font_size_px", style.font_size_px),
        ("padding_x_px", style.padding_x_px),
        ("padding_y_px", style.padding_y_px),
    ] {
        if let Some(value) = value
            && (!value.is_finite() || !(0. ..=128.).contains(&value))
        {
            return Err(ThemeFileError::InvalidMetric(
                format!("{prefix}.{name}"),
                value,
            ));
        }
    }
    Ok(())
}

fn is_hex_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

impl ThemeComponent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AppShell => "app-shell",
            Self::WindowTitleBar => "window-title-bar",
            Self::ProjectBadge => "project-badge",
            Self::ExplorerTree => "explorer-tree",
            Self::ExplorerRow => "explorer-row",
            Self::EditorTabs => "editor-tabs",
            Self::EditorTab => "editor-tab",
            Self::EditorTabClose => "editor-tab-close",
            Self::EditorTabDragPreview => "editor-tab-drag-preview",
            Self::Editor => "editor",
            Self::Scrollbar => "scrollbar",
            Self::OutputPanel => "output-panel",
            Self::PanelToggle => "panel-toggle",
            Self::DockTitleBar => "dock-title-bar",
            Self::DockTab => "dock-tab",
            Self::DockDragPreview => "dock-drag-preview",
            Self::StatusBar => "status-bar",
        }
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
