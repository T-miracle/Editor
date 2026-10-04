//! Converts CommonMark/GFM events into native blocks and controlled image declarations, never raw HTML loads.

use plugin_protocol::ui;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, html};
use std::ops::Range;

mod inline;

/// Parser events retain nesting and UTF-8 ranges until the native tree has been derived.
struct Element<'a> {
    tag: Option<Tag<'a>>,
    range: Range<usize>,
    children: Vec<Part<'a>>,
}

enum Part<'a> {
    Element(Element<'a>),
    Event(Event<'a>, Range<usize>),
}

impl Part<'_> {
    /// Every rendered identity uses the parser's byte offset, independently of translated labels.
    fn range(&self) -> Range<usize> {
        match self {
            Self::Element(element) => element.range.clone(),
            Self::Event(_, range) => range.clone(),
        }
    }
}

/// Parse only the approved extensions, then discard the intermediate event tree.
pub(super) fn blocks(source: &str, locale: &str) -> Vec<ui::Node> {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH;
    let mut stack = vec![Element {
        tag: None,
        range: 0..source.len(),
        children: Vec::new(),
    }];
    for (event, range) in Parser::new_ext(source, options).into_offset_iter() {
        match event {
            Event::Start(tag) => stack.push(Element {
                tag: Some(tag),
                range,
                children: Vec::new(),
            }),
            Event::End(_) => {
                // pulldown-cmark always balances tags, including incomplete Markdown being edited.
                if stack.len() > 1 {
                    let mut element = stack.pop().unwrap();
                    element.range.end = element.range.end.max(range.end);
                    stack
                        .last_mut()
                        .unwrap()
                        .children
                        .push(Part::Element(element));
                }
            }
            event => stack
                .last_mut()
                .unwrap()
                .children
                .push(Part::Event(event, range)),
        }
    }
    Renderer { locale }.blocks(&stack.pop().unwrap().children)
}

struct Renderer<'a> {
    locale: &'a str,
}

impl Renderer<'_> {
    /// Tight lists have bare inline events; buffer them as one paragraph rather than separate text nodes.
    fn blocks(&self, parts: &[Part<'_>]) -> Vec<ui::Node> {
        let mut nodes = Vec::new();
        let mut inline_start = 0;
        for (index, part) in parts.iter().enumerate() {
            if self.is_block(part) {
                self.flush_inline(&parts[inline_start..index], &mut nodes);
                nodes.push(self.block(part));
                inline_start = index + 1;
            }
        }
        self.flush_inline(&parts[inline_start..], &mut nodes);
        nodes
    }

    /// The selected parser flags emit no footnotes, math, metadata or definition-list blocks.
    fn is_block(&self, part: &Part<'_>) -> bool {
        match part {
            Part::Element(element) => matches!(
                element.tag,
                Some(
                    Tag::Paragraph
                        | Tag::Heading { .. }
                        | Tag::BlockQuote(_)
                        | Tag::CodeBlock(_)
                        | Tag::HtmlBlock
                        | Tag::List(_)
                        | Tag::Item
                        | Tag::Table(_)
                )
            ),
            Part::Event(event, _) => matches!(event, Event::Rule | Event::Html(_)),
        }
    }

    /// Native list, quote, code, separator and task nodes keep their semantics outside HTML parsing.
    fn block(&self, part: &Part<'_>) -> ui::Node {
        let range = part.range();
        match part {
            Part::Element(element) => match element.tag.as_ref().unwrap() {
                Tag::CodeBlock(kind) => {
                    let language = match kind {
                        CodeBlockKind::Indented => None,
                        CodeBlockKind::Fenced(info) => info
                            .split_whitespace()
                            .next()
                            .filter(|language| {
                                !language.is_empty()
                                    && language.len() <= 100
                                    && language.bytes().all(|byte| {
                                        byte.is_ascii_alphanumeric() || b"._+-#".contains(&byte)
                                    })
                            })
                            .map(str::to_owned),
                    };
                    ui::Node::code_block(
                        identity(&range, "code"),
                        plain_text(&element.children),
                        language,
                    )
                    .source_range(range)
                }
                Tag::List(start) => self.list(element, *start),
                Tag::BlockQuote(_) => ui::Node::row(
                    identity(&range, "quote"),
                    vec![
                        ui::Node::text(identity(&range, "quote-marker"), "│").width(12.),
                        ui::Node::column(
                            identity(&range, "quote-body"),
                            self.blocks(&element.children),
                        )
                        .gap(6.)
                        .grow(),
                    ],
                )
                .gap(6.)
                .source_range(range),
                Tag::HtmlBlock => {
                    ui::Node::text(identity(&range, "raw-html"), plain_text(&element.children))
                        .source_range(range)
                }
                Tag::Heading { .. } => {
                    self.content(std::slice::from_ref(part), range, "heading", false)
                }
                Tag::Table(_) if has_image(&element.children) => self.table(element),
                Tag::Table(_) => self.rich(std::slice::from_ref(part), range, "table", false),
                Tag::Paragraph => {
                    self.content(std::slice::from_ref(part), range, "paragraph", false)
                }
                _ => ui::Node::column(identity(&range, "block"), self.blocks(&element.children))
                    .gap(6.)
                    .source_range(range),
            },
            Part::Event(Event::Rule, _) => {
                ui::Node::new(identity(&range, "rule"), ui::Kind::Separator).source_range(range)
            }
            Part::Event(Event::Html(text), _) => {
                ui::Node::text(identity(&range, "raw-html"), text.to_string()).source_range(range)
            }
            _ => self.content(std::slice::from_ref(part), range, "paragraph", true),
        }
    }

    /// Image tables retain header/body rows and flex cells, so an image cannot flatten their structure.
    /// Per-row cell identities stay unique even when GFM pads several missing cells at the same byte offset.
    fn table(&self, element: &Element<'_>) -> ui::Node {
        let mut rows = Vec::new();
        for (row_index, part) in element.children.iter().enumerate() {
            let Part::Element(row) = part else { continue };
            let header = matches!(row.tag, Some(Tag::TableHead));
            if !header && !matches!(row.tag, Some(Tag::TableRow)) {
                continue;
            }
            let mut cells = Vec::new();
            for (cell_index, part) in row.children.iter().enumerate() {
                let Part::Element(cell) = part else { continue };
                if !matches!(cell.tag, Some(Tag::TableCell)) {
                    continue;
                }
                let wrappers = if header {
                    vec![Tag::Paragraph, Tag::Strong]
                } else {
                    vec![Tag::Paragraph]
                };
                cells.push(
                    inline::flow(
                        &cell.children,
                        cell.range.clone(),
                        &format!("table-cell-{row_index}-{cell_index}"),
                        &wrappers,
                        self.locale,
                    )
                    .padding(4.)
                    .grow(),
                );
            }
            rows.push(
                ui::Node::row(
                    identity(&row.range, &format!("table-row-{row_index}")),
                    cells,
                )
                .gap(4.)
                .source_range(row.range.clone()),
            );
        }
        ui::Node::column(identity(&element.range, "table"), rows)
            .gap(4.)
            .source_range(element.range.clone())
    }

    /// Ordered starts and recursive item bodies cannot depend on an HTML parser's list defaults.
    /// Task state is always parsed from this source version; native toggles request an edit instead of changing it here.
    fn list(&self, element: &Element<'_>, start: Option<u64>) -> ui::Node {
        let mut items = Vec::new();
        for (index, part) in element.children.iter().enumerate() {
            let Part::Element(item) = part else { continue };
            let marker = if let Some((checked, range)) = task(&item.children) {
                ui::Node::checkbox(identity(&range, "task"), "", checked)
                    .tooltip(if self.locale.starts_with("en") {
                        if checked {
                            "Mark task incomplete"
                        } else {
                            "Mark task complete"
                        }
                    } else if checked {
                        "标记任务未完成"
                    } else {
                        "标记任务已完成"
                    })
                    .source_range(range)
            } else {
                let marker = start.map_or_else(
                    || "•".to_owned(),
                    |start| format!("{}.", start.saturating_add(index as u64)),
                );
                let width = (marker.chars().count() as f32 * 8. + 4.).max(20.);
                ui::Node::text(identity(&item.range, "list-marker"), marker).width(width)
            };
            items.push(
                ui::Node::row(
                    identity(&item.range, "item"),
                    vec![
                        marker,
                        ui::Node::column(
                            identity(&item.range, "item-body"),
                            self.blocks(&item.children),
                        )
                        .gap(6.)
                        .grow(),
                    ],
                )
                .gap(6.)
                .source_range(item.range.clone()),
            );
        }
        ui::Node::column(identity(&element.range, "list"), items)
            .gap(6.)
            .source_range(element.range.clone())
    }

    /// Omitted task markers still contribute their original byte range to the checkbox above.
    fn flush_inline(&self, parts: &[Part<'_>], nodes: &mut Vec<ui::Node>) {
        let Some(first) = parts.first() else { return };
        let range = first.range().start..parts.last().unwrap().range().end;
        let node = self.content(parts, range, "paragraph", true);
        if !matches!(&node.kind, ui::Kind::RichText { html } if html == "<p></p>\n") {
            nodes.push(node);
        }
    }

    /// Only image-bearing content needs native fragment composition; ordinary rich blocks keep their layout.
    fn content(
        &self,
        parts: &[Part<'_>],
        range: Range<usize>,
        kind: &str,
        paragraph: bool,
    ) -> ui::Node {
        if has_image(parts) {
            let wrappers = if paragraph {
                vec![Tag::Paragraph]
            } else {
                Vec::new()
            };
            inline::flow(parts, range, kind, &wrappers, self.locale)
        } else {
            self.rich(parts, range, kind, paragraph)
        }
    }

    /// Generate markup from parser events, never by interpolating unescaped Markdown text or URLs.
    fn rich(
        &self,
        parts: &[Part<'_>],
        range: Range<usize>,
        kind: &str,
        paragraph: bool,
    ) -> ui::Node {
        let mut events = Vec::new();
        if paragraph {
            events.push(Event::Start(Tag::Paragraph));
        }
        self.safe_events(parts, &mut events);
        if paragraph {
            events.push(Event::End(Tag::Paragraph.to_end()));
        }
        let mut output = String::new();
        html::push_html(&mut output, events.into_iter());
        ui::Node::rich_text(identity(&range, kind), output).source_range(range)
    }

    /// Images leave rich text through native composition; defensive alt text never emits ambient image tags.
    /// HTML events become escaped text, and no task HTML is emitted.
    fn safe_events<'a>(&self, parts: &[Part<'a>], events: &mut Vec<Event<'a>>) {
        for part in parts {
            match part {
                Part::Element(element) => {
                    let tag = element.tag.as_ref().unwrap();
                    if matches!(tag, Tag::Image { .. }) {
                        events.push(Event::Text(plain_text(&element.children).into()));
                    } else {
                        events.push(Event::Start(tag.clone()));
                        self.safe_events(&element.children, events);
                        events.push(Event::End(tag.to_end()));
                    }
                }
                Part::Event(Event::Html(text) | Event::InlineHtml(text), _) => {
                    events.push(Event::Text(text.clone()));
                }
                Part::Event(Event::TaskListMarker(_), _) => {}
                Part::Event(event, _) => events.push(event.clone()),
            }
        }
    }
}

/// Images can be nested inside emphasis or links; detection does not flatten their surrounding semantics.
fn has_image(parts: &[Part<'_>]) -> bool {
    parts.iter().any(|part| match part {
        Part::Element(element) => {
            matches!(element.tag, Some(Tag::Image { .. })) || has_image(&element.children)
        }
        Part::Event(_, _) => false,
    })
}

/// Keep native IDs independent of source text, locale and theme, while retaining per-block range identity.
fn identity(range: &Range<usize>, kind: &str) -> String {
    format!("b-{}-{kind}", range.start)
}

/// Preserve literal code, raw HTML and image alternative text without interpreting their markup.
fn plain_text(parts: &[Part<'_>]) -> String {
    let mut text = String::new();
    for part in parts {
        match part {
            Part::Element(element) => text.push_str(&plain_text(&element.children)),
            Part::Event(
                Event::Text(value)
                | Event::Code(value)
                | Event::Html(value)
                | Event::InlineHtml(value),
                _,
            ) => {
                text.push_str(value);
            }
            Part::Event(Event::SoftBreak | Event::HardBreak, _) => text.push('\n'),
            _ => {}
        }
    }
    text
}

/// A nested list owns its own task marker; only this item's first paragraph can label its checkbox.
fn task(parts: &[Part<'_>]) -> Option<(bool, Range<usize>)> {
    for part in parts {
        match part {
            Part::Event(Event::TaskListMarker(checked), range) => {
                return Some((*checked, range.clone()));
            }
            Part::Element(element) if matches!(element.tag, Some(Tag::Paragraph)) => {
                if let Some(marker) = task(&element.children) {
                    return Some(marker);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests;
