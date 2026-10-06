//! The example owns zoom, labels and layouts through the public native UI/canvas contracts.
use plugin_protocol::{
    Environment, Paint, Rect,
    api::{self, ErrorCode, Failure},
    ui::{self, Action, CanvasEvent, Kind, Node},
};

pub(super) struct Demo {
    document: ui::Document,
    original: Node,
    zoom: f32,
}
impl Demo {
    pub(super) fn load() -> Result<Self, Failure> {
        let document: ui::Document = serde_json::from_slice(&api::guest::read_asset(
            "composed-ui.json",
        )?)
        .map_err(|error| {
            Failure::new(
                ErrorCode::InvalidRequest,
                format!("Invalid example UI: {error}"),
            )
        })?;
        Ok(Self {
            original: document.root.clone(),
            document,
            zoom: 1.,
        })
    }
    /// Stable native input identities keep composition intact when only canvas content changes.
    pub(super) fn event(&mut self, event: &ui::UiEvent) {
        match &event.action {
            // Dismiss only the current overlay identity; dialogs retain priority over background menus.
            Action::Dismiss
                if self
                    .document
                    .dialog
                    .as_ref()
                    .is_some_and(|dialog| dialog.id == event.node) =>
            {
                self.document.dialog = None;
            }
            Action::Dismiss
                if self.document.dialog.is_none()
                    && self
                        .document
                        .menu
                        .as_ref()
                        .is_some_and(|menu| menu.id == event.node) =>
            {
                self.document.menu = None;
            }
            Action::Click if event.node == "zoom" => {
                self.zoom = (self.zoom * 1.25).min(4.);
                let zoom = self.zoom;
                visit(&mut self.document.root, &mut |node| {
                    if let Kind::Canvas(canvas) = &mut node.kind {
                        for paint in &mut canvas.paint {
                            if let Paint::Svg { rect, .. } = paint {
                                rect.w = 160. * zoom;
                                rect.h = 100. * zoom;
                            }
                        }
                    }
                });
            }
            Action::Change(text) if event.node == "caption" => self.caption(text),
            Action::Canvas(CanvasEvent::Text { text }) => self.caption(text),
            _ => return,
        }
        self.document.revision += 1;
    }
    fn caption(&mut self, text: &str) {
        visit(&mut self.document.root, &mut |node| {
            if let Kind::Canvas(canvas) = &mut node.kind {
                canvas
                    .paint
                    .retain(|paint| !matches!(paint, Paint::Text { .. }));
                canvas.paint.push(Paint::Text {
                    x: 8.,
                    y: 120.,
                    text: text.into(),
                    color: 0x5599bb,
                    size: 14.,
                    bold: false,
                    font: None,
                });
            }
        });
    }
    /// Layout switching changes only the guest's document, never host slots or identity branches.
    pub(super) fn layout(&mut self, mode: &str) {
        let mut root = self.original.clone();
        if mode == "canvas" {
            let mut canvas = None;
            root.visit(&mut |node| {
                if matches!(node.kind, Kind::Canvas(_)) {
                    canvas = Some(node.clone());
                }
            });
            if let Some(canvas) = canvas {
                root = canvas;
            }
        } else if mode == "form" {
            if let Kind::Column { children } = &mut root.kind {
                children.retain(|node| !matches!(node.kind, Kind::Canvas(_)));
            }
        }
        self.document.root = root;
        self.document.revision += 1;
    }
    pub(super) fn theme(&mut self, environment: &Environment) {
        visit(&mut self.document.root, &mut |node| {
            if let Kind::Canvas(canvas) = &mut node.kind {
                for paint in &mut canvas.paint {
                    if let Paint::Text { color, size, .. } = paint {
                        *color = environment.foreground;
                        *size = environment.ui_font.size_px.unwrap_or(14.);
                    }
                }
            }
        });
        self.document.revision += 1;
    }
    /// The preview uses unsaved source supplied by the host, never a disk reread.
    pub(super) fn preview(&mut self, source: Option<api::DocumentVersion>, text: &str) {
        self.document.source = source.clone();
        visit(&mut self.document.root, &mut |node| {
            if let Kind::NativeEditor { document } = &mut node.kind
                && let Some(source) = &source
            {
                *document = source.clone();
            }
            if let Kind::Canvas(canvas) = &mut node.kind {
                canvas.paint = if text.trim_start().starts_with("<svg") {
                    vec![Paint::Svg {
                        rect: Rect {
                            x: 0.,
                            y: 0.,
                            w: 160.,
                            h: 100.,
                        },
                        clip: Rect {
                            x: 0.,
                            y: 0.,
                            w: 800.,
                            h: 600.,
                        },
                        source: text.into(),
                    }]
                } else {
                    vec![Paint::Text {
                        x: 8.,
                        y: 8.,
                        text: text.into(),
                        color: 0x5599bb,
                        size: 14.,
                        bold: false,
                        font: None,
                    }]
                };
            }
        });
        self.document.revision += 1;
    }
    /// Source ranges become meaningful only after a versioned preview notification arrives.
    pub(super) fn document(&self) -> ui::Document {
        let mut document = self.document.clone();
        if document.source.is_none() {
            document.editor_layout = false;
            // Source controls cannot publish until their immutable document identity is available.
            document.editor_toolbar = None;
            document.editor_image_input = false;
            document.code_highlighting = false;
            document.editor_viewport = None;
            // Preserve the asset's mapping template for later previews, but never publish unbound ranges.
            visit(&mut document.root, &mut unbind_source);
            if let Some(dialog) = &mut document.dialog {
                visit(&mut dialog.content, &mut unbind_source);
            }
        }
        document
    }
    /// Diagnostics reuse the same public document so native image metadata and typed request failures stay observable.
    pub(super) fn diagnostic(&mut self, text: &str) {
        self.document.root = Node::text("scope-probe-result", text);
        self.document.revision += 1;
    }
}

/// Hide source-bound resources only in the publication clone; preserve the asset for later previews.
fn unbind_source(node: &mut Node) {
    if matches!(node.kind, Kind::NativeEditor { .. }) {
        node.kind = Kind::Spacer;
    }
    node.source_range = None;
    if let Kind::Image { alt, .. } = &node.kind {
        node.kind = Kind::Text { text: alt.clone() };
    }
}

/// Recursive layout editing stays entirely inside this guest example.
fn visit(node: &mut Node, f: &mut impl FnMut(&mut Node)) {
    f(node);
    match &mut node.kind {
        Kind::Column { children } | Kind::Row { children } => {
            for child in children {
                visit(child, f);
            }
        }
        Kind::Scroll { content } => visit(content, f),
        Kind::Tabs { tabs, .. } => {
            for tab in tabs {
                visit(&mut tab.content, f);
            }
        }
        _ => {}
    }
}
