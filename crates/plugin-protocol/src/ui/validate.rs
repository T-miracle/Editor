//! Validate before allocating native controls; quotas include generated list/table cells.
use super::*;
use std::collections::BTreeSet;

pub(super) fn document(document: &Document) -> Result<(), String> {
    if document.version != VERSION {
        return Err("Unsupported UI protocol version".into());
    }
    if document.content_colors.len() > 64
        || document.content_colors.iter().any(|(role, color)| {
            role.is_empty()
                || role.len() > 128
                || !role
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
                || *color > 0xffffff
        })
    {
        return Err("Content color defaults require bounded role keys and RGB values".into());
    }
    if let Some(file) = &document.file {
        file.validate().map_err(|error| error.to_string())?;
    }
    if document.editor_layout && document.source.is_none() && document.file.is_none() {
        return Err("File layout requires a current source or file context".into());
    }
    if document.editor_image_input && document.source.is_none() {
        return Err("Editor image input requires Document.source".into());
    }
    if document.code_highlighting && document.source.is_none() {
        return Err("Code highlighting requires Document.source".into());
    }
    if let Some(scroll) = &document.editor_viewport {
        if document.source.is_none() {
            return Err("Editor viewport requires Document.source".into());
        }
        if document
            .root
            .find(scroll)
            .is_none_or(|node| !matches!(node.kind, Kind::Scroll { .. }))
        {
            return Err("Editor viewport requires an active root Scroll".into());
        }
    }
    let mut validator = Validator {
        ids: BTreeSet::new(),
        count: 0,
        bytes: 0,
        drawings: 0,
        vectors: 0,
        images: 0,
        icons: 0,
        has_source: document.source.is_some(),
        has_file: document.file.is_some(),
        editor_source: document.source.clone(),
        allow_editor: document.editor_layout,
        editors: 0,
    };
    validator.node(&document.root, 0)?;
    if document.tools.len() > 32 {
        return Err("Too many toolbar contributions".into());
    }
    for tool in &document.tools {
        validator.id(&tool.id)?;
        validator.budget(1)?;
        for text in [
            &tool.label.zh_cn,
            &tool.label.en,
            &tool.tooltip.zh_cn,
            &tool.tooltip.en,
        ] {
            if text.is_empty() || text.len() > 512 || text.chars().any(char::is_control) {
                return Err("Tool labels and tooltips require bounded bilingual text".into());
            }
            validator.text(text)?;
        }
        tool.icon.validate()?;
        match &tool.target {
            ToolTarget::File { version } if document.file.as_ref() == Some(version) => {
                version.validate().map_err(|error| error.to_string())?;
            }
            ToolTarget::Window { panel }
                if !panel.is_empty()
                    && panel.len() <= 100
                    && panel.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                    }) => {}
            _ => return Err("Tool target requires this file or an owned window".into()),
        }
    }
    // Auxiliary surfaces never mount a second native input/IME target.
    validator.allow_editor = false;
    if let Some(toolbar) = &document.editor_toolbar {
        if document.source.is_none() {
            return Err("Editor toolbar requires Document.source".into());
        }
        // The same validator preserves identities and budgets across the host's source/preview surfaces.
        validator.node(toolbar, 0)?;
    }
    if let Some(menu) = &document.menu {
        validator.id(&menu.id)?;
        menu.validate()?;
        validator.budget(menu.items.len())?;
        for item in &menu.items {
            validator.text(&item.label)?;
        }
    }
    if let Some(dialog) = &document.dialog {
        validator.id(&dialog.id)?;
        validator.text(&dialog.title)?;
        dimension(dialog.width, 240., 1200.)?;
        validator.node(&dialog.content, 0)?;
    }
    if serde_json::to_vec(document)
        .map_err(|error| error.to_string())?
        .len()
        > 2 * 1024 * 1024
    {
        return Err("UI document encoding quota exceeded".into());
    }
    Ok(())
}

fn dimension(value: f32, min: f32, max: f32) -> Result<(), String> {
    if !value.is_finite() || value < min || value > max {
        Err("Invalid UI dimension/value".into())
    } else {
        Ok(())
    }
}

struct Validator {
    ids: BTreeSet<String>,
    count: usize,
    bytes: usize,
    drawings: usize,
    vectors: usize,
    /// One document owns the combined image budget across its root, toolbar and dialog.
    images: usize,
    /// Small inline icons have their own shared budget, independent of document image resources.
    icons: usize,
    /// Source mappings are meaningful only in a version-bound preview document.
    has_source: bool,
    /// Binary image nodes require an independent file-resource authority.
    has_file: bool,
    /// Root-only native borrowing is exact and unique across the complete published tree.
    editor_source: Option<crate::api::DocumentVersion>,
    allow_editor: bool,
    editors: usize,
}
impl Validator {
    fn budget(&mut self, count: usize) -> Result<(), String> {
        self.count += count;
        if self.count > 2048 {
            Err("UI node quota exceeded".into())
        } else {
            Ok(())
        }
    }
    fn id(&mut self, id: &str) -> Result<(), String> {
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        {
            return Err("UI IDs must use 1..128 ASCII letters, digits, '.', '_' or '-'".into());
        }
        if !self.ids.insert(id.into()) {
            return Err(format!("Duplicate UI ID: {id}"));
        }
        Ok(())
    }
    fn text(&mut self, text: &str) -> Result<(), String> {
        self.bytes += text.len();
        if text.len() > 65536 || self.bytes > 1024 * 1024 {
            Err("UI text quota exceeded".into())
        } else {
            Ok(())
        }
    }
    fn node(&mut self, node: &Node, depth: usize) -> Result<(), String> {
        if let Some(viewport) = &node.viewport {
            viewport.validate(&node.kind)?;
        }
        if depth > 24 {
            return Err("UI nesting quota exceeded".into());
        }
        self.budget(1)?;
        self.id(&node.id)?;
        if node.layout.resizable
            && (node.layout.wrap
                || !matches!(&node.kind,
            Kind::Row {children}|Kind::Column{children} if (2..=16).contains(&children.len())))
        {
            return Err(
                "Resizable panes require 2..16 Row/Column children without wrapping".into(),
            );
        }
        if !node.links.is_empty() {
            if !matches!(
                node.kind,
                Kind::RichText { .. } | Kind::Image { .. } | Kind::Text { .. }
            ) {
                return Err("Only read-only content can declare native links".into());
            }
            if !matches!(node.kind, Kind::RichText { .. }) && node.links.len() > 1 {
                return Err("An image or alternative text has one native link target".into());
            }
            // Focus targets consume the same whole-tree control budget, even inside one rich block.
            self.budget(node.links.len())?;
            for link in &node.links {
                if link.uri.is_empty()
                    || link.uri.len() > 4096
                    || link.uri.chars().any(char::is_control)
                    || link.label.len() > 256
                {
                    return Err("Invalid native link target".into());
                }
                self.text(&link.uri)?;
                self.text(&link.label)?;
            }
        }
        if let Some(tooltip) = &node.tooltip {
            self.text(tooltip)?;
        }
        if let Some(svg) = &node.button_icon {
            if !matches!(&node.kind, Kind::Button { label } if !label.trim().is_empty()) {
                return Err("Button icons require a Button with an accessible label".into());
            }
            self.icons += 1;
            if svg.is_empty() || svg.len() > 4096 || self.icons > 64 {
                return Err("UI icon quota exceeded".into());
            }
            self.text(svg)?;
        }
        if let Some(range) = node.source_range {
            if !self.has_source {
                return Err("UI source ranges require Document.source".into());
            }
            if range.start > range.end || range.end > 1024 * 1024 {
                return Err("Invalid UI source range".into());
            }
        }
        if node.role.len() > 128 {
            return Err("UI theme role too long".into());
        }
        let layout = &node.layout;
        for value in [layout.width, layout.height].into_iter().flatten() {
            dimension(value, 0., 10000.)?;
        }
        dimension(layout.gap, 0., 256.)?;
        dimension(layout.padding, 0., 256.)?;
        match &node.kind {
            Kind::NativeEditor { document } => {
                self.editors += 1;
                if !self.allow_editor
                    || self.editors > 1
                    || self.editor_source.as_ref() != Some(document)
                {
                    return Err(
                        "Native editor must uniquely reference this layout's exact source".into(),
                    );
                }
            }
            Kind::SideTabs(tabs) => {
                if tabs.id != node.id {
                    return Err("Item list identity differs from its node".into());
                }
                tabs.validate()?;
                self.budget(tabs.items.len())?;
                for item in &tabs.items {
                    self.text(&item.label)?;
                    if let Some(status) = &item.status {
                        self.text(status)?;
                    }
                }
            }
            Kind::Canvas(canvas) => {
                canvas.validate()?;
                // Drawings count toward this whole document, preventing many small canvases bypassing quotas.
                self.drawings += canvas.paint.len();
                if self.drawings > 32_000 {
                    return Err("UI drawing quota exceeded".into());
                }
                for paint in &canvas.paint {
                    match paint {
                        crate::Paint::Text { text, .. } => self.text(text)?,
                        crate::Paint::Svg { .. } => {
                            self.vectors += 1;
                            if self.vectors > 16 {
                                return Err("UI vector quota exceeded".into());
                            }
                        }
                        _ => {}
                    }
                }
            }
            Kind::Column { children } | Kind::Row { children } => {
                for child in children {
                    self.node(child, depth + 1)?;
                }
            }
            Kind::Scroll { content } => self.node(content, depth + 1)?,
            Kind::Text { text } => self.text(text)?,
            Kind::RichText { html } => {
                self.text(html)?;
                // Bound the generated native markup tree too, including dense empty/table tags.
                // Counting delimiters is deliberately conservative and never interprets Markdown.
                self.budget(html.bytes().filter(|byte| *byte == b'<').count())?;
            }
            Kind::CodeBlock { text, language } => {
                self.text(text)?;
                self.budget(text.lines().count())?;
                if let Some(language) = language
                    && (language.is_empty()
                        || language.len() > 100
                        || !language
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"._+-#".contains(&byte)))
                {
                    return Err(
                        "Code language IDs must use 1..100 ASCII identifier characters".into(),
                    );
                }
                if let Some(language) = language {
                    self.text(language)?;
                }
            }
            Kind::FileImage { alt, .. } => {
                if !self.has_file {
                    return Err("File image requires Document.file".into());
                }
                self.images += 1;
                if self.images > 64 {
                    return Err("UI image quota exceeded".into());
                }
                self.text(alt)?;
            }
            Kind::Image { source, alt } => {
                if !self.has_source {
                    return Err("Images require Document.source".into());
                }
                self.images += 1;
                if self.images > 64 {
                    return Err("Image node quota exceeded".into());
                }
                if source.is_empty() || source.len() > 4096 {
                    return Err("Image URI must use 1..4096 UTF-8 bytes".into());
                }
                self.text(source)?;
                self.text(alt)?;
            }
            Kind::Button { label } | Kind::Checkbox { label, .. } => self.text(label)?,
            Kind::Input(input) | Kind::Textarea(input) => {
                self.text(&input.value)?;
                self.text(&input.placeholder)?;
            }
            Kind::Choice { options, selected } => {
                self.budget(options.len())?;
                let mut ids = BTreeSet::new();
                for option in options {
                    if option.id.is_empty() || option.id.len() > 128 || !ids.insert(&option.id) {
                        return Err("Invalid/duplicate choice ID".into());
                    }
                    self.text(&option.label)?;
                }
                if selected.as_ref().is_some_and(|id| !ids.contains(id)) {
                    return Err("Unknown choice selection".into());
                }
            }
            Kind::Tabs { tabs, selected } => {
                self.budget(tabs.len())?;
                let mut ids = BTreeSet::new();
                for tab in tabs {
                    if tab.id.is_empty() || tab.id.len() > 128 || !ids.insert(&tab.id) {
                        return Err("Invalid/duplicate tab ID".into());
                    }
                    self.text(&tab.label)?;
                    self.node(&tab.content, depth + 1)?;
                }
                if !ids.contains(selected) {
                    return Err("Unknown tab selection".into());
                }
            }
            Kind::List { items } => {
                self.budget(items.len())?;
                for text in items {
                    self.text(text)?;
                }
            }
            Kind::Table { headers, rows } => {
                if headers.is_empty() || headers.len() > 32 {
                    return Err("Table needs 1..32 columns".into());
                }
                self.budget(headers.len() + rows.len() * headers.len())?;
                for text in headers {
                    self.text(text)?;
                }
                for row in rows {
                    if row.len() != headers.len() {
                        return Err("Table row width differs from headers".into());
                    }
                    for text in row {
                        self.text(text)?;
                    }
                }
            }
            Kind::Progress { label, value } => {
                self.text(label)?;
                dimension(*value, 0., 100.)?;
            }
            Kind::Separator | Kind::Spacer => {}
        }
        Ok(())
    }
}
