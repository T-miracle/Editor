//! Compose image-bearing Markdown into balanced rich fragments and source-bound native image declarations.

use super::{Element, Part, identity, plain_text};
use plugin_protocol::ui;
use pulldown_cmark::{Event, Tag, html};
use std::ops::Range;

/// Split inline images vertically in source order while retaining every surrounding rich-text style.
/// Wrappers supply a paragraph for tight-list content and bold header styling for native table cells.
pub(super) fn flow<'source>(
    parts: &[Part<'source>],
    range: Range<usize>,
    kind: &str,
    wrappers: &[Tag<'source>],
    locale: &str,
) -> ui::Node {
    let base = identity(&range, kind);
    let mut flow = Flow {
        base: &base,
        locale,
        events: Vec::new(),
        active: Vec::new(),
        range: None,
        nodes: Vec::new(),
    };
    for tag in wrappers {
        flow.enter(tag.clone());
    }
    flow.walk(parts);
    for _ in wrappers {
        flow.leave();
    }
    flow.flush();
    if flow.nodes.len() == 1 {
        flow.nodes.pop().unwrap()
    } else {
        // Flow still borrows its short identity while the event buffers are being dropped.
        ui::Node::column(base.clone(), flow.nodes)
            .gap(4.)
            .source_range(range)
    }
}

/// Only transient parser events are buffered; rich fragments and image nodes are derived readonly output.
struct Flow<'source, 'view> {
    base: &'view str,
    locale: &'view str,
    events: Vec<Event<'source>>,
    active: Vec<Tag<'source>>,
    range: Option<Range<usize>>,
    nodes: Vec<ui::Node>,
}

impl<'source> Flow<'source, '_> {
    /// Flush around each image, preserving active emphasis/link/heading tags on both adjacent fragments.
    fn walk(&mut self, parts: &[Part<'source>]) {
        for part in parts {
            match part {
                Part::Element(element) if matches!(element.tag, Some(Tag::Image { .. })) => {
                    self.flush();
                    self.nodes.push(image(element, self.locale));
                }
                Part::Element(element) => {
                    let tag = element.tag.as_ref().unwrap().clone();
                    self.enter(tag);
                    self.walk(&element.children);
                    self.leave();
                }
                Part::Event(Event::Html(text) | Event::InlineHtml(text), range) => {
                    self.push(Event::Text(text.clone()), range);
                }
                Part::Event(Event::TaskListMarker(_), _) => {}
                Part::Event(event, range) => self.push(event.clone(), range),
            }
        }
    }

    /// Remember balanced parser containers so splitting a paragraph never drops its enclosing style.
    fn enter(&mut self, tag: Tag<'source>) {
        self.events.push(Event::Start(tag.clone()));
        self.active.push(tag);
    }

    /// Parser and explicit wrapper containers are always balanced before this derived flow is returned.
    fn leave(&mut self) {
        let tag = self.active.pop().unwrap();
        self.events.push(Event::End(tag.to_end()));
    }

    /// Visible events, including literal HTML and line breaks, supply precise source ranges for rich fragments.
    fn push(&mut self, event: Event<'source>, range: &Range<usize>) {
        if let Some(current) = self.range.as_mut() {
            current.end = current.end.max(range.end);
        } else {
            self.range = Some(range.clone());
        }
        self.events.push(event);
    }

    /// Close a fragment's active tags for safe HTML, then reopen them after the native image.
    /// Empty wrapper-only fragments are omitted so standalone images create no blank text blocks.
    fn flush(&mut self) {
        let mut events = std::mem::take(&mut self.events);
        if let Some(range) = self.range.take() {
            events.extend(self.active.iter().rev().map(|tag| Event::End(tag.to_end())));
            let mut output = String::new();
            html::push_html(&mut output, events.into_iter());
            self.nodes.push(
                ui::Node::rich_text(format!("{}-part-{}", self.base, self.nodes.len()), output)
                    .source_range(range),
            );
        }
        self.events
            .extend(self.active.iter().cloned().map(Event::Start));
    }
}

/// Declare only a URI and author alt text; controlled host tasks own all image reads, decoding and failures.
/// Empty and oversized URIs cannot enter the public image contract, so their explanation stays local.
fn image(element: &Element<'_>, locale: &str) -> ui::Node {
    let Some(Tag::Image { dest_url, .. }) = element.tag.as_ref() else {
        unreachable!("Only parsed image elements declare image resources");
    };
    let range = element.range.clone();
    let alt = plain_text(&element.children);
    let english = locale.starts_with("en");
    let reason = if dest_url.is_empty() {
        Some(if english {
            "The image reference has no source."
        } else {
            "图片引用没有地址。"
        })
    } else if dest_url.len() > 4096 {
        Some(if english {
            "The image reference exceeds the 4096-byte URI limit."
        } else {
            "图片引用地址超过 4096 字节限制。"
        })
    } else {
        None
    };
    if let Some(reason) = reason {
        let label = if alt.is_empty() {
            if english { "Image" } else { "图片" }
        } else {
            &alt
        };
        ui::Node::text(
            identity(&range, "image-error"),
            format!("{label}: {reason}"),
        )
        .source_range(range)
    } else {
        ui::Node::image(identity(&range, "image"), dest_url.to_string(), alt).source_range(range)
    }
}
