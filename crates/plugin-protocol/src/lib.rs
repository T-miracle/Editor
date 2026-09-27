//! Versioned, platform-independent messages shared by plugins and their host.
//! No editor, terminal-emulator or native UI implementation belongs in this crate.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// A package's identity, compatibility range and explicitly requested capabilities.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub protocol: u32,
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
}

/// Opaque plugin-owned data; the host never interprets or migrates its contents.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema: u32,
    pub data: String,
}

/// Host theme and workspace context are supplied explicitly, without ambient access.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Environment {
    pub workspace: String,
    pub os: String,
    pub background: u32,
    pub foreground: u32,
    pub muted: u32,
    pub border: u32,
    pub accent: u32,
    pub selection: u32,
}

/// Physical surface coordinates let a guest lay out its own interface.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
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
    Fill {
        rect: Rect,
        color: u32,
    },
    Text {
        x: f32,
        y: f32,
        text: String,
        color: u32,
        size: f32,
        bold: bool,
    },
}

/// Standard widgets add native text editing and click semantics to a drawing list.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Widget {
    pub id: String,
    pub rect: Rect,
    pub label: String,
    pub edit: bool,
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
    /// Declared panel receiving this scene; multiple panels can publish independently.
    #[serde(default)]
    pub panel: String,
    pub paint: Vec<Paint>,
    pub widgets: Vec<Widget>,
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
        id: String,
        cwd: Option<String>,
        text: Option<String>,
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
