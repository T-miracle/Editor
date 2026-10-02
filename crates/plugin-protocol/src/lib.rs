//! Versioned, platform-independent messages shared by plugins and their host.
//! No editor, terminal-emulator or native UI implementation belongs in this crate.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Capability-based API; legacy messages remain only during the staged migration.
pub mod api;
pub mod process;
pub mod settings;
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

/// Dock and command contributions are data, not host-side feature branches.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Panel {
    pub id: String,
    pub title: String,
    pub position: String,
    /// Protocol 6 editor-local previews match these case-insensitive extensions without dots.
    /// Other panel positions leave this list empty and retain their independent dock behavior.
    #[serde(default)]
    pub file_extensions: Vec<String>,
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
    /// Plugin-owned text roles, updated together with colors by Event::Theme.
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Paint {
    /// Protocol 6 draws a full-color vector above preceding operations, preserving transparency.
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

/// Standard widgets add native text editing and click semantics to a drawing list.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Widget {
    pub id: String,
    pub rect: Rect,
    pub label: String,
    pub edit: bool,
    #[serde(default)]
    pub style: WidgetStyle,
}

/// Optional native-control styling resolved by the plugin from its current theme.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WidgetStyle {
    #[serde(default)]
    pub font: FontStyle,
    #[serde(default)]
    pub foreground: Option<u32>,
    #[serde(default)]
    pub background: Option<u32>,
    #[serde(default)]
    pub hover_background: Option<u32>,
    #[serde(default)]
    pub active_background: Option<u32>,
}

/// Scrollbars are rendered with the same host control used by the file explorer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScrollInfo {
    pub id: String,
    pub rect: Rect,
    pub content: f32,
    pub offset: f32,
    /// Optional idle timeout for a native overlay; scrolling remains available while hidden.
    #[serde(default)]
    pub hide_after_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Scene {
    /// Protocol 2 native view tree. Omit to use the legacy canvas surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<ui::Document>,
    /// Protocol 4 canvas controls; accept `chrome` from existing protocol 3 plugins.
    #[serde(default, alias = "chrome", skip_serializing_if = "Option::is_none")]
    pub controls: Option<ui::CanvasControls>,
    /// Declared panel receiving this scene; multiple panels can publish independently.
    #[serde(default)]
    pub panel: String,
    pub paint: Vec<Paint>,
    pub widgets: Vec<Widget>,
    /// Invisible guest-defined hit areas that use the native column-resize cursor.
    #[serde(default)]
    pub column_resize_regions: Vec<Rect>,
    pub scroll: Option<ScrollInfo>,
    pub font: String,
    pub font_size: f32,
    pub cursor: Rect,
}

/// Messages enter one serial guest event loop; prepare must not acquire OS resources.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Message {
    Prepare {
        environment: Environment,
        snapshot: Option<Snapshot>,
    },
    Activate,
    Event(Event),
    Snapshot,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Event {
    /// Host-only bridge for independently versioned capability notifications during migration.
    Capability(api::Notification),
    /// Protocol 6 supplies the active in-memory document to a permission-checked editor preview.
    /// A missing path clears the previous document when its file no longer matches the panel.
    Document {
        path: Option<String>,
        text: String,
    },
    /// Native UI events are scoped by the enclosing Surface message.
    Ui(ui::UiEvent),
    /// Native events retain their originating panel when a plugin declares several surfaces.
    Surface {
        panel: String,
        event: Box<Event>,
    },
    Resize {
        width: f32,
        height: f32,
        cell_width: f32,
        cell_height: f32,
    },
    Theme(Environment),
    ProcessOutput {
        handle: u64,
        bytes: Vec<u8>,
    },
    ProcessExit {
        handle: u64,
    },
    Command {
        /// Declared command, host reply, or `panel.opened` lifecycle event scoped by Surface.
        id: String,
        cwd: Option<String>,
        text: Option<String>,
        /// Plugin-owned structured parameters; omitted by older hosts and parameterless actions.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        arguments: Option<serde_json::Value>,
    },
    Key {
        key: String,
        ctrl: bool,
        alt: bool,
        shift: bool,
    },
    Text(String),
    Paste(String),
    Pointer {
        kind: String,
        x: f32,
        y: f32,
        button: u8,
        clicks: u8,
        shift: bool,
    },
    Wheel {
        delta: f32,
        shift: bool,
        x: f32,
        y: f32,
    },
    Scroll {
        id: String,
        offset: f32,
    },
    Edit {
        id: String,
        text: String,
    },
    Focus(bool),
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Reply {
    /// Migration adapter only; new guests expose this through api::Output.
    #[serde(default)]
    pub configuration: Option<settings::Proposal>,
    pub scene: Option<Scene>,
    #[serde(default)]
    pub scenes: Vec<Scene>,
    pub snapshot: Option<Snapshot>,
    pub error: Option<String>,
}

/// Every privileged call is checked by the host for this plugin and its live handles.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Request {
    /// Read a resource shipped inside this exact installed package version.
    ReadAsset {
        path: String,
    },
    Spawn {
        program: String,
        args: Vec<String>,
        cwd: String,
        columns: u16,
        rows: u16,
    },
    Write {
        handle: u64,
        bytes: Vec<u8>,
    },
    Resize {
        handle: u64,
        columns: u16,
        rows: u16,
    },
    Close {
        handle: u64,
    },
    ReadWorkspace {
        path: String,
    },
    ReadData {
        path: String,
    },
    WriteData {
        path: String,
        text: String,
    },
    ClipboardWrite(String),
    ClipboardRead,
    /// Native editor operations, including `hide_panel:<declared-panel-id>` for this plugin only.
    /// Panel hiding is asynchronous and uses the existing `editor.commands` permission.
    Editor {
        command: String,
    },
}
impl Request {
    /// PTY child programs inherit native OS rights; granting this capability is explicit.
    pub fn permission(&self) -> &'static str {
        match self {
            Self::ReadAsset { .. } => "assets",
            Self::Spawn { .. } | Self::Write { .. } | Self::Resize { .. } | Self::Close { .. } => {
                "process.pty"
            }
            Self::ReadWorkspace { .. } => "workspace.read",
            Self::ReadData { .. } | Self::WriteData { .. } => "storage",
            Self::ClipboardWrite(_) | Self::ClipboardRead => "clipboard",
            Self::Editor { .. } => "editor.commands",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_accepts_legacy_chrome_but_emits_controls() {
        // Existing WASM packages may still send `chrome`; newly built plugins emit `controls`.
        let scene = Scene {
            controls: Some(ui::CanvasControls::default()),
            ..Scene::default()
        };
        let mut value = serde_json::to_value(&scene).unwrap();
        assert!(value.get("controls").is_some());
        assert!(value.get("chrome").is_none());
        let legacy = value.as_object_mut().unwrap().remove("controls").unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("chrome".into(), legacy);
        let restored: Scene = serde_json::from_value(value).unwrap();
        assert!(restored.controls.is_some());
    }

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
