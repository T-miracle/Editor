use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
use thiserror::Error;

mod languages;
mod theme_effects;
pub use languages::{Highlighter, LanguageDefinition};
pub use theme_effects::{ThemeWindow, ThemeWindowBackground};

/// Read pre-rename identifiers without retaining their prefix in current manifests or UI state.
pub fn canonical_plugin_id(id: &str) -> &str {
    match id.strip_prefix("me.").unwrap_or(id) {
        "svg-preview" => "svg",
        id => id,
    }
}

/// Migrate a flattened color/font role while preserving everything after its plugin namespace.
pub fn canonical_plugin_token(token: &str) -> String {
    let token = token.strip_prefix("me.").unwrap_or(token);
    if let Some(role) = token.strip_prefix("svg-preview.") {
        format!("svg.{role}")
    } else {
        token.to_owned()
    }
}

/// Current independent contributions; obsolete combined declarations are rejected explicitly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub plugin: PluginMetadata,
    /// Independent recognition and highlighting contributions for the capability protocol.
    #[serde(default)]
    pub language_definitions: Vec<LanguageDefinition>,
    #[serde(default)]
    pub highlighters: Vec<Highlighter>,
    #[serde(default)]
    /// JSON file that maps plugin file types and names to icon assets.
    pub file_icons: Option<PathBuf>,
    #[serde(default)]
    pub theme: Option<ThemeContribution>,
}

/// Identifies a theme's JSON file containing forced file icon overrides.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ThemeContribution {
    /// A JSON theme file, relative to the plugin root.
    pub file: PathBuf,
    /// A JSON file icon map, relative to the plugin root.
    #[serde(default)]
    pub file_icons: Option<PathBuf>,
}

/// A versioned JSON mapping from file selectors to light and dark icon assets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileIconConfig {
    pub schema_version: u32,
    #[serde(default)]
    pub icons: Vec<FileIconRule>,
}

/// Selects files and points to icon assets relative to the owning plugin or theme.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileIconRule {
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub folders: bool,
    /// A file that must occur in the selected file's directory or an ancestor.
    #[serde(default)]
    pub project_markers: Vec<String>,
    pub light: PathBuf,
    pub dark: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginMetadata {
    pub id: String,
    pub name: String,
    pub version: String,
    pub host_version: String,
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
    /// Native backdrop options; omitted settings preserve older opaque themes.
    #[serde(default)]
    pub window: ThemeWindow,
    #[serde(default)]
    pub typography: ThemeTypography,
    #[serde(default)]
    pub components: BTreeMap<ThemeComponent, ComponentStyles>,
    /// Plugin-owned color trees, grouped by plugin ID and local role.
    #[serde(default)]
    pub plugins: BTreeMap<String, PluginTheme>,
    /// Read older theme packages without keeping their flat layout in new files.
    #[serde(default, rename = "plugin_colors", skip_serializing)]
    pub legacy_plugin_colors: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeTypography {
    pub ui: PluginTextStyle,
    pub mono: PluginTextStyle,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct PluginTextStyle {
    pub family: Option<String>,
    pub size_px: Option<f32>,
    pub bold: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PluginTheme {
    #[serde(default)]
    pub typography: BTreeMap<String, PluginTextStyle>,
    #[serde(flatten)]
    pub colors: BTreeMap<String, PluginThemeColor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum PluginThemeColor {
    Color(String),
    Group(BTreeMap<String, PluginThemeColor>),
}

impl ThemeDefinition {
    /// Flatten the theme-file tree into the stable runtime plugin token API.
    pub fn plugin_colors(&self) -> BTreeMap<String, String> {
        let mut colors = self
            .legacy_plugin_colors
            .iter()
            .map(|(key, value)| (canonical_plugin_token(key), value.clone()))
            .collect();
        for (plugin, theme) in &self.plugins {
            for (name, value) in &theme.colors {
                flatten_plugin_color(
                    &mut colors,
                    format!("{}.{name}", canonical_plugin_id(plugin)),
                    value,
                );
            }
        }
        colors
    }

    pub fn plugin_text_styles(&self) -> BTreeMap<String, PluginTextStyle> {
        self.plugins
            .iter()
            .flat_map(|(plugin, theme)| {
                theme.typography.iter().map(move |(role, style)| {
                    (
                        format!("{}.{role}", canonical_plugin_id(plugin)),
                        style.clone(),
                    )
                })
            })
            .collect()
    }
}

fn flatten_plugin_color(
    colors: &mut BTreeMap<String, String>,
    path: String,
    value: &PluginThemeColor,
) {
    match value {
        PluginThemeColor::Color(color) => {
            colors.insert(path, color.clone());
        }
        PluginThemeColor::Group(group) => {
            for (name, value) in group {
                flatten_plugin_color(colors, format!("{path}.{name}"), value);
            }
        }
    }
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
    ExplorerMenu,
    EditorTabs,
    EditorTab,
    EditorTabClose,
    EditorTabDragPreview,
    Editor,
    Scrollbar,
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
    #[error("language {0} does not declare an extension or file name")]
    MissingLanguageSelector(String),
    #[error("invalid language file name selector: {0}")]
    InvalidLanguageFilename(String),
    #[error("plugin asset path is absolute: {0}")]
    AbsolutePath(String),
    #[error("plugin asset path must stay inside the plugin directory: {0}")]
    UnsafeAssetPath(String),
    #[error("theme file path is empty")]
    EmptyThemePath,
    #[error("file icon JSON path is empty")]
    EmptyFileIconPath,
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
    #[error("invalid color {1} at {0}; expected #RRGGBB (host styles also allow #RRGGBBAA)")]
    InvalidColor(String, String),
    #[error("invalid plugin color key at {0}")]
    InvalidPluginColorKey(String),
    #[error("invalid font style at {0}: {1}")]
    InvalidFontStyle(String, String),
    #[error("invalid metric {1} at {0}; expected a finite value from 0 to 128 pixels")]
    InvalidMetric(String, f32),
    #[error("file icon JSON path is empty")]
    EmptyFileIconPath,
    #[error("unsafe file icon asset path: {0}")]
    UnsafeFileIconPath(String),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FileIconConfigError {
    #[error("invalid JSON: {0}")]
    Parse(String),
    #[error("unsupported file icon schema version {0}")]
    UnsupportedSchemaVersion(u32),
    #[error("file icon rule must match an extension, file name, or folder")]
    MissingSelector,
    #[error("unsafe file icon asset path: {0}")]
    UnsafeAssetPath(String),
}

impl PluginManifest {
    pub fn parse(source: &str) -> Result<Self, ManifestError> {
        let manifest = toml::from_str::<Self>(source)
            .map_err(|error| ManifestError::Parse(error.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        languages::validate(self)?;
        if self.plugin.id.is_empty()
            || !self.plugin.id.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
            })
        {
            return Err(ManifestError::InvalidPluginId);
        }

        if let Some(file_icons) = &self.file_icons {
            if file_icons.as_os_str().is_empty() {
                return Err(ManifestError::EmptyFileIconPath);
            }
            validate_relative_path(file_icons).map_err(|_| {
                ManifestError::UnsafeAssetPath(file_icons.to_string_lossy().into_owned())
            })?;
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
            if let Some(file_icons) = &theme.file_icons {
                if file_icons.as_os_str().is_empty() {
                    return Err(ManifestError::EmptyFileIconPath);
                }
                validate_relative_path(file_icons).map_err(|_| {
                    ManifestError::UnsafeAssetPath(file_icons.to_string_lossy().into_owned())
                })?;
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

fn validate_relative_path(path: &std::path::Path) -> Result<(), ()> {
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(());
    }
    Ok(())
}

impl FileIconConfig {
    pub fn parse(source: &str) -> Result<Self, FileIconConfigError> {
        let config = serde_json::from_str::<Self>(source)
            .map_err(|error| FileIconConfigError::Parse(error.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), FileIconConfigError> {
        if self.schema_version != 1 {
            return Err(FileIconConfigError::UnsupportedSchemaVersion(
                self.schema_version,
            ));
        }
        for rule in &self.icons {
            if rule.extensions.is_empty() && rule.files.is_empty() && !rule.folders {
                return Err(FileIconConfigError::MissingSelector);
            }
            for path in [&rule.light, &rule.dark] {
                if path.as_os_str().is_empty() || validate_relative_path(path).is_err() {
                    return Err(FileIconConfigError::UnsafeAssetPath(
                        path.to_string_lossy().into_owned(),
                    ));
                }
            }
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
        if !theme_effects::is_theme_color(value) {
            return Err(ThemeFileError::InvalidColor(path, value.to_owned()));
        }
    }
    validate_text_style(&theme.typography.ui, "typography.ui")?;
    validate_text_style(&theme.typography.mono, "typography.mono")?;
    for (plugin, plugin_theme) in &theme.plugins {
        if plugin.is_empty()
            || !plugin.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
            })
        {
            return Err(ThemeFileError::InvalidPluginColorKey(format!(
                "plugins.{plugin}"
            )));
        }
        validate_plugin_color_keys(&plugin_theme.colors, &format!("plugins.{plugin}"))?;
        for (role, style) in &plugin_theme.typography {
            if !valid_plugin_role(role) {
                return Err(ThemeFileError::InvalidFontStyle(
                    format!("plugins.{plugin}.typography.{role}"),
                    "invalid role name".into(),
                ));
            }
            validate_text_style(style, &format!("plugins.{plugin}.typography.{role}"))?;
        }
    }
    for (key, value) in &theme.legacy_plugin_colors {
        if !is_hex_color(value) {
            return Err(ThemeFileError::InvalidColor(
                format!("plugin_colors.{key}"),
                value.clone(),
            ));
        }
    }
    for (key, value) in theme.plugin_colors() {
        if !is_hex_color(&value) {
            return Err(ThemeFileError::InvalidColor(
                format!("plugins.{key}"),
                value,
            ));
        }
    }
    Ok(())
}

fn validate_plugin_color_keys(
    roles: &BTreeMap<String, PluginThemeColor>,
    prefix: &str,
) -> Result<(), ThemeFileError> {
    for (name, value) in roles {
        let path = format!("{prefix}.{name}");
        if !valid_plugin_role(name) {
            return Err(ThemeFileError::InvalidPluginColorKey(path));
        }
        if let PluginThemeColor::Group(group) = value {
            validate_plugin_color_keys(group, &path)?;
        }
    }
    Ok(())
}

fn valid_plugin_role(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
}

fn validate_text_style(style: &PluginTextStyle, path: &str) -> Result<(), ThemeFileError> {
    if let Some(family) = &style.family
        && (family.trim().is_empty() || family.len() > 128)
    {
        return Err(ThemeFileError::InvalidFontStyle(
            format!("{path}.family"),
            "family must contain 1 to 128 characters".into(),
        ));
    }
    if let Some(size) = style.size_px
        && (!size.is_finite() || !(8. ..=64.).contains(&size))
    {
        return Err(ThemeFileError::InvalidFontStyle(
            format!("{path}.size_px"),
            "size must be between 8 and 64 pixels".into(),
        ));
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
            Self::ExplorerMenu => "explorer-menu",
            Self::EditorTabs => "editor-tabs",
            Self::EditorTab => "editor-tab",
            Self::EditorTabClose => "editor-tab-close",
            Self::EditorTabDragPreview => "editor-tab-drag-preview",
            Self::Editor => "editor",
            Self::Scrollbar => "scrollbar",
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

    /// Recognition and highlighting remain independently declared and validated.
    #[test]
    fn parses_a_declarative_language_plugin() {
        let manifest = PluginManifest::parse(
            r#"
                [plugin]
                id = "rust"
                name = "Rust"
                version = "0.1.0"
                host_version = ">=0.1.0"

                [[language_definitions]]
                id = "rust"
                name = "Rust"
                extensions = ["rs"]
                [[highlighters]]
                id = "syntax"
                language = "rust"
                grammar_name = "rust"
                grammar = "grammar/rust.wasm"
                highlights = "queries/highlights.scm"
                tree_sitter_abi = 15
            "#,
        )
        .unwrap();

        assert_eq!(manifest.plugin.id, "rust");
        assert_eq!(manifest.language_definitions[0].extensions, ["rs"]);
        assert!(manifest.language_definitions[0].filenames.is_empty());
        assert_eq!(manifest.highlighters[0].tree_sitter_abi, 15);
    }

    /// Exact filenames can be the only selector, but wildcard lockfile rules are rejected.
    #[test]
    fn validates_exact_language_filenames() {
        let source = r#"
            [plugin]
            id = "lockfile"
            name = "Lockfile"
            version = "0.1.0"
            host_version = ">=0.1.0"

            [[language_definitions]]
            id = "toml"
            name = "TOML"
            filenames = ["Cargo.lock"]
        "#;
        let manifest = PluginManifest::parse(source).unwrap();
        assert!(manifest.language_definitions[0].extensions.is_empty());
        assert_eq!(manifest.language_definitions[0].filenames, ["Cargo.lock"]);
        assert!(matches!(
            PluginManifest::parse(&source.replace("Cargo.lock", "*.lock")),
            Err(ManifestError::Parse(_))
        ));
    }

    /// Old executable language fields cannot silently bypass the new service declarations.
    #[test]
    fn rejects_combined_legacy_language_declarations() {
        let source = r#"[plugin]
id = "legacy"
name = "Legacy"
version = "1.0.0"
host_version = "^0.1"
[[languages]]
id = "legacy"
lsp_command = "unmanaged-server"
"#;
        let error = PluginManifest::parse(source).unwrap_err().to_string();
        assert!(
            error.contains("unknown field") && error.contains("languages"),
            "{error}"
        );
    }

    /// Tests supply their own user overrides rather than depend on host-bundled plugin defaults.
    fn theme_with_plugin_overrides() -> ThemeFile {
        let mut wire: serde_json::Value =
            serde_json::from_str(include_str!("../../editor-app/assets/themes/default.json"))
                .unwrap();
        wire["themes"][0]["plugins"] = serde_json::json!({"fixture":{
            "ansi":{"yellow":"#795100"},
            "typography":{"tab":{"family":"Segoe UI","size_px":14}}
        }});
        ThemeFile::parse(&wire.to_string()).unwrap()
    }

    /// Nested plugin colors flatten for the runtime and reject malformed overrides.
    #[test]
    fn validates_plugin_color_tokens() {
        let mut file = theme_with_plugin_overrides();
        {
            let PluginThemeColor::Group(ansi) = file.themes[0]
                .plugins
                .get_mut("fixture")
                .unwrap()
                .colors
                .get_mut("ansi")
                .unwrap()
            else {
                panic!("ANSI palette must be a group");
            };
            ansi.insert("yellow".into(), PluginThemeColor::Color("#795100".into()));
        }
        file.validate().unwrap();
        file.themes[0]
            .legacy_plugin_colors
            .insert("fixture.ansi.yellow".into(), "#123456".into());
        assert_eq!(
            file.themes[0].plugin_colors()["fixture.ansi.yellow"],
            "#795100"
        );
        let PluginThemeColor::Group(ansi) = file.themes[0]
            .plugins
            .get_mut("fixture")
            .unwrap()
            .colors
            .get_mut("ansi")
            .unwrap()
        else {
            panic!("ANSI palette must be a group");
        };
        ansi.insert("yellow".into(), PluginThemeColor::Color("yellow".into()));
        assert!(matches!(
            file.validate(),
            Err(ThemeFileError::InvalidColor(path, _))
                if path == "plugins.fixture.ansi.yellow"
        ));
    }

    #[test]
    fn legacy_flat_plugin_colors_remain_readable() {
        let source = include_str!("../../editor-app/assets/themes/default.json");
        let mut old_file: serde_json::Value = serde_json::from_str(source).unwrap();
        old_file["themes"][0]
            .as_object_mut()
            .unwrap()
            .remove("plugins");
        old_file["themes"][0]["plugin_colors"] =
            serde_json::json!({ "terminal.ansi.yellow": "#123456" });
        let file = ThemeFile::parse(&old_file.to_string()).unwrap();
        assert_eq!(
            file.themes[0].plugin_colors()["terminal.ansi.yellow"],
            "#123456"
        );
        assert!(
            !serde_json::to_string(&file)
                .unwrap()
                .contains("plugin_colors")
        );
    }

    /// Dotted nested keys are rejected independently of any installed plugin identity.
    #[test]
    fn nested_plugin_keys_cannot_hide_flattened_paths() {
        let mut file = theme_with_plugin_overrides();
        file.themes[0]
            .plugins
            .get_mut("fixture")
            .unwrap()
            .colors
            .insert("ansi.red".into(), PluginThemeColor::Color("#123456".into()));
        assert!(matches!(
            file.validate(),
            Err(ThemeFileError::InvalidPluginColorKey(path))
                if path == "plugins.fixture.ansi.red"
        ));
    }

    /// User role fonts still flatten and validate after package defaults leave the host stylesheet.
    #[test]
    fn plugin_text_roles_flatten_and_validate() {
        let mut file = theme_with_plugin_overrides();
        assert_eq!(
            file.themes[0].plugin_text_styles()["fixture.tab"]
                .family
                .as_deref(),
            Some("Segoe UI")
        );
        file.themes[0]
            .plugins
            .get_mut("fixture")
            .unwrap()
            .typography
            .get_mut("tab")
            .unwrap()
            .size_px = Some(0.);
        assert!(matches!(
            file.validate(),
            Err(ThemeFileError::InvalidFontStyle(path, _))
                if path == "plugins.fixture.typography.tab.size_px"
        ));
    }
}
