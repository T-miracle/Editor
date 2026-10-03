//! Portable native UI: plugins own state, while the host owns layout, input and theme.
//! Protocol 7 composes canvases with native controls through independently negotiated capabilities.
//! No GPUI objects cross this interface.

use serde::{Deserialize, Serialize};
use std::ops::Range;

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
    /// Native controls above the source editor, bound to `source` and requiring `editor.toolbar`.
    /// Node identities and quotas share the panel's root/dialog/menu namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor_toolbar: Option<Node>,
    /// At most one modal per panel. Removing it closes the modal.
    #[serde(default)]
    pub dialog: Option<Dialog>,
    /// Optional native popup in root-local coordinates; it owns input until dismissed.
    #[serde(default)]
    pub menu: Option<PopupMenu>,
}

impl Document {
    pub fn new(root: Node) -> Self {
        Self {
            source: None,
            version: VERSION,
            revision: 0,
            root,
            editor_toolbar: None,
            dialog: None,
            menu: None,
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

    /// Find a live root/toolbar target, while dialogs and popups retain exclusive input ownership.
    pub fn active_node(&self, id: &str) -> Option<&Node> {
        if let Some(dialog) = &self.dialog {
            return dialog.content.find(id);
        }
        if self.menu.is_some() {
            return None;
        }
        self.root.find(id).or_else(|| {
            self.editor_toolbar
                .as_ref()
                .and_then(|toolbar| toolbar.find(id))
        })
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
    /// Optional UTF-8 source bytes in the immutable version echoed by `Document.source`.
    /// Mapping does not grant document access or editing authority; it requires `ui.richtext`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_range: Option<SourceRange>,
    /// Localized hover/accessibility text; this counts toward the ordinary UI text budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
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

/// Half-open UTF-8 byte offsets for one rendered block in its source document version.
/// `start <= end <= 1 MiB`; the host must check actual source length and character boundaries
/// before using the range. Blocks retain their identity through the enclosing node ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRange {
    /// Inclusive byte offset in the current immutable source text.
    pub start: usize,
    /// Exclusive byte offset in the same source text.
    pub end: usize,
}

/// Layout contains geometry only. Color and typography always come from the active theme.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub width: Option<f32>,
    pub height: Option<f32>,
    #[serde(default)]
    pub grow: bool,
    /// Rows may wrap children into additional lines; the host derives height from their content.
    #[serde(default)]
    pub wrap: bool,
    #[serde(default)]
    pub padding: f32,
    #[serde(default)]
    pub gap: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Kind {
    /// A keyed, reorderable native item list can be placed anywhere in the ordinary layout tree.
    SideTabs(SideTabs),
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
    /// Read-only native rich markup, requiring `ui.richtext` in addition to `ui.native`.
    /// The host renders a restricted HTML subset and disables implicit URL/image access;
    /// this is neither a WebView nor a request to parse Markdown in the host.
    RichText {
        html: String,
    },
    /// Read-only literal code with preserved whitespace and a monospace presentation.
    /// The optional language is an informational ID, not authority to start a language tool.
    CodeBlock {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        language: Option<String>,
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
            source_range: None,
            tooltip: None,
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
    /// Create a read-only rich block with stable `id` and restricted `html` markup.
    /// The returned node needs `ui.richtext`; text quotas are checked by `Document::validate`.
    pub fn rich_text(id: impl Into<String>, html: impl Into<String>) -> Self {
        Self::new(id, Kind::RichText { html: html.into() })
    }
    /// Create a literal code block from `text` and an optional ASCII language ID.
    /// Returns a node requiring `ui.richtext`; the language does not imply highlighting support.
    pub fn code_block(
        id: impl Into<String>,
        text: impl Into<String>,
        language: Option<String>,
    ) -> Self {
        Self::new(
            id,
            Kind::CodeBlock {
                text: text.into(),
                language,
            },
        )
    }
    /// Attach half-open UTF-8 `range` offsets into the version in `Document.source`.
    /// Returns the mapped node; invalid bounds are rejected by `Document::validate`.
    /// Even ordinary text or layout nodes need `ui.richtext` when carrying this metadata.
    pub fn source_range(mut self, range: Range<usize>) -> Self {
        self.source_range = Some(SourceRange {
            start: range.start,
            end: range.end,
        });
        self
    }

    /// Attach localized hover/accessibility text; `Document::validate` enforces shared text quotas.
    pub fn tooltip(mut self, text: impl Into<String>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    /// Let a row wrap its children instead of clipping a toolbar at a narrow editor width.
    pub fn wrap(mut self) -> Self {
        self.layout.wrap = true;
        self
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
            Kind::SideTabs(_) => "tab_bar",
            Kind::Canvas(_) => "canvas",
            Kind::Column { .. } | Kind::Row { .. } => "container",
            Kind::Scroll { .. } => "scroll",
            Kind::Text { .. } => "text",
            Kind::RichText { .. } => "rich_text",
            Kind::CodeBlock { .. } => "code_block",
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
