//! Portable native UI: plugins own state, while the host owns layout, input and theme.
//! Protocol 7 composes canvases with native controls through independently negotiated capabilities.
//! Documents require protocol 2; canvas controls use protocol 4, selectable dock edges protocol 5.
//! Editor-local file previews and color vector painting use protocol 6 in the canvas messages.
//! No GPUI objects cross this interface.

use serde::{Deserialize, Serialize};

mod controls;
pub use controls::*;
mod canvas;
pub use canvas::*;
mod events;

#[cfg(test)]
mod tests;
mod validate;

pub const VERSION: u32 = 1;

/// Replace a panel's complete view atomically. Revision is echoed in user events.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    /// Preview replies echo their input version; regular panels leave it absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<crate::api::DocumentVersion>,
    pub version: u32,
    pub revision: u64,
    pub root: Node,
    /// At most one modal per panel. Removing it closes the modal.
    #[serde(default)]
    pub dialog: Option<Dialog>,
}

impl Document {
    pub fn new(root: Node) -> Self {
        Self {
            source: None,
            version: VERSION,
            revision: 0,
            root,
            dialog: None,
        }
    }

    pub fn revision(mut self, revision: u64) -> Self {
        self.revision = revision;
        self
    }

    pub fn dialog(mut self, dialog: Dialog) -> Self {
        self.dialog = Some(dialog);
        self
    }

    /// Reject unsupported versions, duplicate identities and unbounded native workloads.
    pub fn validate(&self) -> Result<(), String> {
        validate::document(self)
    }

    /// Find an interactive node only in the active modal, or the panel when no modal exists.
    pub fn active_node(&self, id: &str) -> Option<&Node> {
        self.dialog
            .as_ref()
            .map_or(&self.root, |dialog| &dialog.content)
            .find(id)
    }
}

/// A modal uses the same tree and theme as its owning panel. IDs share one namespace.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dialog {
    pub id: String,
    pub title: String,
    pub width: f32,
    pub content: Node,
}

impl Dialog {
    pub fn new(id: impl Into<String>, title: impl Into<String>, content: Node) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            width: 480.,
            content,
        }
    }
}

/// Stable IDs preserve focus, IME composition and scroll position across tree replacements.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    /// Resolves `plugins[plugin_id].ui[role]` colors and `typography[role]` fonts.
    /// If empty, the host uses the node kind (e.g. `button`) as the role.
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub layout: Layout,
    pub kind: Kind,
}

/// Layout contains geometry only. Color and typography always come from the active theme.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub width: Option<f32>,
    pub height: Option<f32>,
    #[serde(default)]
    pub grow: bool,
    #[serde(default)]
    pub padding: f32,
    #[serde(default)]
    pub gap: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Kind {
    /// Any position in the same layout tree can hold a drawing surface; it has no implicit grid.
    Canvas(Canvas),
    Column {
        children: Vec<Node>,
    },
    Row {
        children: Vec<Node>,
    },
    Scroll {
        content: Box<Node>,
    },
    Text {
        text: String,
    },
    Button {
        label: String,
    },
    Input(Input),
    Checkbox {
        label: String,
        checked: bool,
    },
    /// A native radio group, selected by stable option ID rather than array index.
    Choice {
        options: Vec<OptionItem>,
        selected: Option<String>,
    },
    Tabs {
        tabs: Vec<Tab>,
        selected: String,
    },
    /// Read-only text rows. Use ordinary buttons in a column for action lists.
    List {
        items: Vec<String>,
    },
    /// Read-only table. Each row must have exactly as many cells as headers.
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Separator,
    /// Determinate percentage, from zero to one hundred.
    Progress {
        label: String,
        value: f32,
    },
    Spacer,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Input {
    pub value: String,
    /// Increment to replace the native draft intentionally (clear/reset/load).
    /// Ordinary Change replies leave this unchanged, preventing stale echoes from erasing typing.
    #[serde(default)]
    pub value_revision: u64,
    #[serde(default)]
    pub placeholder: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OptionItem {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub disabled: bool,
}

impl OptionItem {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            disabled: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tab {
    pub id: String,
    pub label: String,
    pub content: Node,
}

impl Tab {
    pub fn new(id: impl Into<String>, label: impl Into<String>, content: Node) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            content,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UiEvent {
    pub revision: u64,
    pub node: String,
    pub action: Action,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Action {
    Canvas(CanvasEvent),
    Click,
    Change(String),
    Submit(String),
    Toggle(bool),
    Select(String),
    Close(String),
    Rename {
        id: String,
        value: String,
    },
    Move {
        from: String,
        to: String,
    },
    Resize(f32),
    /// Pointer coordinates relative to the side tab list's top-left corner.
    Context {
        id: String,
        x: f32,
        y: f32,
    },
    /// Escape, backdrop or close button. The plugin should remove Document.dialog.
    Dismiss,
}

impl Node {
    pub fn new(id: impl Into<String>, kind: Kind) -> Self {
        Self {
            id: id.into(),
            role: String::new(),
            disabled: false,
            layout: Layout::default(),
            kind,
        }
    }
    pub fn column(id: impl Into<String>, children: Vec<Node>) -> Self {
        Self::new(id, Kind::Column { children })
    }
    pub fn row(id: impl Into<String>, children: Vec<Node>) -> Self {
        Self::new(id, Kind::Row { children })
    }
    pub fn scroll(id: impl Into<String>, content: Node) -> Self {
        Self::new(
            id,
            Kind::Scroll {
                content: Box::new(content),
            },
        )
    }
    pub fn text(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self::new(id, Kind::Text { text: text.into() })
    }
    pub fn button(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(
            id,
            Kind::Button {
                label: label.into(),
            },
        )
    }
    pub fn input(id: impl Into<String>, input: Input) -> Self {
        Self::new(id, Kind::Input(input))
    }
    pub fn checkbox(id: impl Into<String>, label: impl Into<String>, checked: bool) -> Self {
        Self::new(
            id,
            Kind::Checkbox {
                label: label.into(),
                checked,
            },
        )
    }
    pub fn role(mut self, role: impl Into<String>) -> Self {
        self.role = role.into();
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    pub fn gap(mut self, gap: f32) -> Self {
        self.layout.gap = gap;
        self
    }
    pub fn padding(mut self, padding: f32) -> Self {
        self.layout.padding = padding;
        self
    }
    pub fn width(mut self, width: f32) -> Self {
        self.layout.width = Some(width);
        self
    }
    pub fn height(mut self, height: f32) -> Self {
        self.layout.height = Some(height);
        self
    }
    pub fn grow(mut self) -> Self {
        self.layout.grow = true;
        self
    }

    pub fn theme_role(&self) -> &str {
        if !self.role.is_empty() {
            return &self.role;
        }
        match &self.kind {
            Kind::Canvas(_) => "canvas",
            Kind::Column { .. } | Kind::Row { .. } => "container",
            Kind::Scroll { .. } => "scroll",
            Kind::Text { .. } => "text",
            Kind::Button { .. } => "button",
            Kind::Input(_) => "input",
            Kind::Checkbox { .. } => "checkbox",
            Kind::Choice { .. } => "choice",
            Kind::Tabs { .. } => "tabs",
            Kind::List { .. } => "list",
            Kind::Table { .. } => "table",
            Kind::Separator => "separator",
            Kind::Progress { .. } => "progress",
            Kind::Spacer => "spacer",
        }
    }

    /// Walk all nodes, including inactive tabs, so identities can be validated together.
    pub fn visit(&self, visitor: &mut impl FnMut(&Node)) {
        visitor(self);
        match &self.kind {
            Kind::Column { children } | Kind::Row { children } => {
                children.iter().for_each(|n| n.visit(visitor))
            }
            Kind::Scroll { content } => content.visit(visitor),
            Kind::Tabs { tabs, .. } => tabs.iter().for_each(|t| t.content.visit(visitor)),
            _ => {}
        }
    }

    fn find(&self, id: &str) -> Option<&Self> {
        if self.disabled {
            return None;
        }
        if self.id == id {
            return Some(self);
        }
        match &self.kind {
            Kind::Column { children } | Kind::Row { children } => {
                children.iter().find_map(|n| n.find(id))
            }
            Kind::Scroll { content } => content.find(id),
            Kind::Tabs { tabs, selected } => tabs
                .iter()
                .find(|t| &t.id == selected)
                .and_then(|t| t.content.find(id)),
            _ => None,
        }
    }
}
