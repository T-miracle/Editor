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
mod tools;
pub use tools::*;

#[cfg(test)]
mod images_tests;
#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tools_tests;
mod validate;

pub const VERSION: u32 = 1;

/// Replace a panel's complete view atomically. Revision is echoed in user events.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    /// Bottom-bar functions remain guest-owned, independently of this surface's layout contribution.
    /// Requires `ui.tools`; file targets also require the exact `Document.file` context.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolButton>,
    /// The selected file provider owns this complete center tree. Requires `editor.layout`.
    /// NativeEditor nodes borrow the existing source session; omitting them hides its input surface.
    #[serde(default)]
    pub editor_layout: bool,
    /// File-resource authority echoed by `editor.files`; no text session or mutable bytes implied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<crate::api::FileVersion>,
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
    /// Claim user-initiated image paste/drop on the source pane through `editor.images`.
    /// This additive declaration requires `source`; ordinary panels and older guests leave it false.
    #[serde(default)]
    pub editor_image_input: bool,
    /// Emit clicked native rich-text links through `ui.links`; false retains inert link defaults.
    #[serde(default)]
    pub link_events: bool,
    /// Readonly CodeBlock language/text requests use the selected plugin WASM highlighter.
    /// Requires ui.code_highlighting, editor.read and the owning preview's exact source version.
    #[serde(default)]
    pub code_highlighting: bool,
    /// Bind one active source-mapped Scroll to native source viewport notifications and locate requests.
    /// Requires editor.viewport, ui.richtext, editor.read and this preview's exact source identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor_viewport: Option<String>,
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
            tools: Vec::new(),
            editor_layout: false,
            file: None,
            source: None,
            version: VERSION,
            revision: 0,
            root,
            editor_toolbar: None,
            editor_image_input: false,
            link_events: false,
            code_highlighting: false,
            editor_viewport: None,
            dialog: None,
            menu: None,
        }
    }

    pub fn revision(mut self, revision: u64) -> Self {
        self.revision = revision;
        self
    }

    /// Enable source-pane image input for this versioned preview without exposing native pixel bytes.
    pub fn editor_image_input(mut self, enabled: bool) -> Self {
        self.editor_image_input = enabled;
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

    /// Return the editor reference actually mounted in the active root, excluding disabled/inactive trees.
    /// Validation separately requires exact source ownership and at most one reference in the full tree.
    pub fn active_native_editor(&self) -> Option<&crate::api::DocumentVersion> {
        fn active(node: &Node) -> Option<&crate::api::DocumentVersion> {
            if node.disabled {
                return None;
            }
            match &node.kind {
                Kind::NativeEditor { document } => Some(document),
                Kind::Column { children } | Kind::Row { children } => {
                    children.iter().find_map(active)
                }
                Kind::Scroll { content } => active(content),
                Kind::Tabs { tabs, selected } => tabs
                    .iter()
                    .find(|tab| tab.id == *selected)
                    .and_then(|tab| active(&tab.content)),
                _ => None,
            }
        }
        self.editor_layout.then(|| active(&self.root)).flatten()
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
    /// Explicit read-only link targets for native keyboard focus and linked images/alternative text.
    /// URIs use rendered href spelling, not a host-parsed domain format; requires `ui.links`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<LinkTarget>,
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

/// One bounded native activation target. The guest supplies its visible caption and destination;
/// `Document.link_events` enables events, while navigation remains a separately authorized request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkTarget {
    /// Exact rendered href, at most 4096 bytes; no controls or implicit URL effect.
    pub uri: String,
    /// Author/user-localized accessible caption, at most 256 UTF-8 bytes; empty uses host locale.
    pub label: String,
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
    /// Borrow the existing editor of this exact source version; never creates mutable document state.
    /// Only one reference is permitted, in the root of a negotiated file layout.
    NativeEditor {
        document: crate::api::DocumentVersion,
    },
    /// Display this context's file using controlled decoding. Requires `ui.file_images` and
    /// `workspace.read`; the guest chooses sizing, while the host performs native layout/painting.
    FileImage {
        alt: String,
        sizing: ImageSizing,
    },
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
    /// Source-bound native image, requiring `ui.images`; loading is a controlled host resource task.
    /// Document-relative paths require `workspace.read`, HTTP(S) requires `network.images`.
    /// The host limits reads to 8 MiB and 30 seconds and never follows redirects or uses credentials.
    /// Failures affect this image only; markup image tags remain unable to perform ambient reads.
    Image {
        /// Relative percent-encoded URI or credential-free HTTP(S) URL, at most 4096 UTF-8 bytes.
        source: String,
        /// Localized alternative text used while loading or displaying a per-image failure.
        alt: String,
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

/// Generic image geometry policy chosen by a plugin; this has no file-format business rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageSizing {
    /// Preserve intrinsic dimensions when they fit; otherwise contain both axes without enlarging.
    OriginalContain,
    /// Contain the available area, permitting an explicit enlargement of small images.
    Contain,
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
    /// Coalesced live geometry of the bound preview scroll; it never mutates its derived block.
    Viewport(crate::api::PreviewViewport),
    /// URI supplied by a clicked native rich-text link, never by parsing or layout alone.
    Link {
        uri: String,
    },
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
            links: Vec::new(),
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
    /// Declare a native image with stable `id`, resource `source` and localized alternative text.
    /// `Document.source` and negotiated `ui.images` are mandatory. Loading never grants WASM bytes
    /// or file/network authority; unavailable grants become a failure for this node only.
    pub fn image(id: impl Into<String>, source: impl Into<String>, alt: impl Into<String>) -> Self {
        Self::new(
            id,
            Kind::Image {
                source: source.into(),
                alt: alt.into(),
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
            Kind::NativeEditor { .. } => "editor",
            Kind::SideTabs(_) => "tab_bar",
            Kind::Canvas(_) => "canvas",
            Kind::Column { .. } | Kind::Row { .. } => "container",
            Kind::Scroll { .. } => "scroll",
            Kind::Text { .. } => "text",
            Kind::RichText { .. } => "rich_text",
            Kind::CodeBlock { .. } => "code_block",
            Kind::Image { .. } | Kind::FileImage { .. } => "image",
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

    /// A bound viewport cannot report a block owned by a nested independent Scroll.
    fn viewport_block(&self, id: &str) -> Option<&Self> {
        if self.disabled || matches!(self.kind, Kind::Scroll { .. }) {
            return None;
        }
        if self.id == id {
            return Some(self);
        }
        match &self.kind {
            Kind::Column { children } | Kind::Row { children } => {
                children.iter().find_map(|node| node.viewport_block(id))
            }
            Kind::Tabs { tabs, selected } => tabs
                .iter()
                .find(|tab| &tab.id == selected)
                .and_then(|tab| tab.content.viewport_block(id)),
            _ => None,
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
