//! Versioned, platform-independent messages shared by plugins and their host.
//! No editor, terminal-emulator or native UI implementation belongs in this crate.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Capability-based lifecycle, notifications and correlated host operations.
pub mod api;
/// Public command-template, native-form and validation service contract.
pub mod configurations;
pub mod debug;
pub mod dependencies;
pub mod execution;
pub mod language;
pub mod process;
pub mod service;
pub mod settings;
/// Dynamic target discovery and build preparation are provider policy, independent of execution.
pub mod targets;
pub mod ui;

/// Guest imports and exports use the WIT contract supplied by the building editor.
#[cfg(feature = "guest")]
pub mod bindings {
    wit_bindgen::generate!({
        path: "wit",
        world: "plugin",
        pub_export_macro: true,
        default_bindings_module: "::plugin_protocol::bindings",
    });
}

/// A package's identity, compatibility range and explicitly requested capabilities.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Opt-in private-data format; the host never interprets the plugin's business fields.
    #[serde(default)]
    pub data_format: Option<DataFormat>,
    /// Versioned plugin-to-plugin contracts, separate from native executable declarations.
    #[serde(default)]
    pub plugin_services: service::Declarations,
    /// Standard LSP bindings consume native service declarations without requiring a lifecycle component.
    #[serde(default)]
    pub language_servers: Vec<language::Provider>,
    /// Fixed native service definitions; each key requires its own installation grant.
    #[serde(default)]
    pub services: std::collections::BTreeMap<String, process::Service>,
    /// Plugin namespace only; these declarations cannot alter host trust, grants or editor preferences.
    #[serde(default)]
    pub settings: std::collections::BTreeMap<String, settings::Definition>,
    /// Opt in to a bounded, preparation-only validation/discovery callback.
    #[serde(default)]
    pub settings_hook: bool,
    pub id: String,
    pub name: String,
    pub version: String,
    pub protocol: u32,
    /// Protocol 7 selects the capability transport; interface versions negotiate independently.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<api::Requirements>,
    /// Execution ownership is independent of package-wide installation and enablement.
    #[serde(default)]
    pub scope: api::InstanceScope,
    /// Executable plugins provide a WASM component; declarative packages use host lifecycle only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    /// Optional package-relative declarative language, theme, and file-icon manifest.
    #[serde(default)]
    pub contributions: Option<String>,
    #[serde(default)]
    pub permissions: BTreeSet<String>,
    #[serde(default)]
    pub panels: Vec<Panel>,
    #[serde(default)]
    pub commands: Vec<Command>,
    /// Maximum opaque persisted snapshot size, also bounded by the host quota.
    pub storage_limit: usize,
}

/// Version zero is reserved for a new scope; changed versions require an explicit WASM migration hook.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataFormat {
    pub version: u32,
    #[serde(default)]
    pub migration_hook: bool,
}

/// Dock and command contributions are data, not host-side feature branches.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Panel {
    pub id: String,
    pub title: String,
    pub position: String,
    /// Editor-local previews match these case-insensitive extensions without dots.
    /// Other panel positions leave this list empty and retain their independent dock behavior.
    #[serde(default)]
    pub file_extensions: Vec<String>,
    /// Opt-in source/split/preview controls for workspace-owned editor previews.
    /// All three SVG paths belong to this package; declaration requires `editor.presentation`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_modes: Option<PreviewModes>,
    /// Used only when this panel has no saved visibility preference yet.
    #[serde(default = "panel_visible_by_default")]
    pub default_visible: bool,
    /// Optional status-bar position among plugin panels; lower values appear first.
    #[serde(default)]
    pub status_order: Option<i32>,
    /// Optional package-relative SVG used when the editor has a light theme.
    #[serde(default)]
    pub icon_light: Option<String>,
    /// Optional package-relative SVG used when the editor has a dark theme.
    #[serde(default)]
    pub icon_dark: Option<String>,
}

/// Package-owned artwork for the host's three fixed editor presentation modes.
/// Paths use package-relative `/` separators and reference safe geometric SVGs under 64 KiB.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewModes {
    /// Package-relative SVG for showing only the source editor.
    pub source: String,
    /// Package-relative SVG for showing source on the left and preview on the right.
    pub split: String,
    /// Package-relative SVG for showing only the preview.
    pub preview: String,
}

/// Workspace-persisted editor layout; the initial presentation shows both source and preview.
/// This changes host layout only and never creates a second mutable document or undo stack.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewMode {
    /// Hide the preview while retaining the current source document session.
    Source,
    #[default]
    /// Show the source editor and preview together.
    Split,
    /// Hide the source editor without discarding its text, selection or undo history.
    Preview,
}

/// Existing manifests continue to open their panels unless they opt out.
fn panel_visible_by_default() -> bool {
    true
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Command {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub shortcut: Option<String>,
    #[serde(default)]
    pub menu: bool,
    #[serde(default)]
    pub toolbar: Option<String>,
    /// Optional icon path from the host's shared `gpui-kit-assets` catalog.
    #[serde(default)]
    pub toolbar_icon: Option<String>,
}

/// Opaque plugin-owned data; the host never interprets or migrates its contents.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema: u32,
    pub data: String,
}

/// Host theme and workspace context are supplied explicitly, without ambient access.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Environment {
    pub workspace: String,
    pub os: String,
    /// Current user interface locale, supplied at preparation and in Theme notifications.
    /// Missing or empty values preserve the existing Simplified Chinese default.
    #[serde(default)]
    pub locale: String,
    pub background: u32,
    pub foreground: u32,
    pub muted: u32,
    /// Editor text color used for subdued terminal text and metadata.
    #[serde(default)]
    pub muted_foreground: u32,
    pub border: u32,
    pub accent: u32,
    pub selection: u32,
    /// Whether the active editor theme requests a dark terminal palette.
    #[serde(default)]
    pub dark: bool,
    /// Generic theme color tokens; plugins interpret only keys they own.
    #[serde(default)]
    pub theme_colors: std::collections::BTreeMap<String, u32>,
    /// Resolved editor fonts are the fallback for every plugin's own text roles.
    #[serde(default)]
    pub ui_font: FontStyle,
    #[serde(default)]
    pub mono_font: FontStyle,
    /// Plugin-owned text roles, updated together with colors by api::Notification::Theme.
    #[serde(default)]
    pub theme_text_styles: std::collections::BTreeMap<String, FontStyle>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FontStyle {
    pub family: Option<String>,
    pub size_px: Option<f32>,
    pub bold: Option<bool>,
}

impl FontStyle {
    pub fn over(self, base: Self) -> Self {
        Self {
            family: self.family.or(base.family),
            size_px: self.size_px.or(base.size_px),
            bold: self.bold.or(base.bold),
        }
    }
}

impl Environment {
    /// A plugin consumes its own role without knowing how themes are installed or switched.
    pub fn font_style(&self, plugin: &str, role: &str, monospace: bool) -> FontStyle {
        let base = if monospace {
            &self.mono_font
        } else {
            &self.ui_font
        };
        self.theme_text_styles
            .get(&format!("{plugin}.{role}"))
            .cloned()
            .unwrap_or_default()
            .over(base.clone())
    }

    pub fn color(&self, plugin: &str, role: &str) -> Option<u32> {
        self.theme_colors.get(&format!("{plugin}.{role}")).copied()
    }
}

/// Physical surface coordinates let a guest lay out its own interface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Rect {
    /// Hit testing is shared by arbitrary plugin widgets.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}

/// A declarative drawing list; text is shaped by the native host text system.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Paint {
    /// Draw a full-color vector above preceding operations, preserving transparency.
    /// The explicit clip bounds raster allocation when the image is zoomed beyond its viewport.
    Svg {
        rect: Rect,
        clip: Rect,
        source: String,
    },
    Fill {
        rect: Rect,
        color: u32,
        /// Fill through the current native canvas bottom while resize events catch up.
        #[serde(default)]
        extend_to_bottom: bool,
    },
    Text {
        x: f32,
        y: f32,
        text: String,
        color: u32,
        size: f32,
        bold: bool,
        #[serde(default)]
        font: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_roles_override_only_declared_font_properties() {
        let mut environment = Environment {
            ui_font: FontStyle {
                family: Some("Editor UI".into()),
                size_px: Some(14.),
                bold: Some(false),
            },
            ..Default::default()
        };
        environment.theme_text_styles.insert(
            "example.body".into(),
            FontStyle {
                size_px: Some(18.),
                ..Default::default()
            },
        );
        environment
            .theme_colors
            .insert("example.body.foreground".into(), 0x123456);
        assert_eq!(
            environment.font_style("example", "body", false),
            FontStyle {
                family: Some("Editor UI".into()),
                size_px: Some(18.),
                bold: Some(false),
            }
        );
        assert_eq!(
            environment.color("example", "body.foreground"),
            Some(0x123456)
        );
        assert_eq!(environment.color("another.plugin", "body.foreground"), None);
    }
}
