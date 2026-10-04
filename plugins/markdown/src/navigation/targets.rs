//! Link destinations come from CommonMark events and bind to the currently rendered native source ranges.

use super::Reason;
use plugin_protocol::{api, ui};
use pulldown_cmark::{Event, LinkType, Tag, TagEnd, html};
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};

#[derive(Default)]
pub(crate) struct Index {
    links: Vec<Link>,
    headings: Vec<Heading>,
}

struct Link {
    /// Parsed email autolinks retain their implicit mailto scheme; explicit filenames keep their original URI.
    uri: String,
    /// Base emits the HTML-decoded attribute produced by the same locked public Markdown writer.
    href: String,
    /// Visible parser events retain their own ranges so split image paragraphs receive only their local caption.
    caption: Vec<Caption>,
    range: Range<usize>,
}

struct Caption {
    text: String,
    range: Range<usize>,
}

struct Heading {
    slug: String,
    range: Range<usize>,
}

/// Classification keeps encoded paths intact; only fragment names are decoded for heading lookup.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Destination {
    Anchor(String),
    Relative {
        path: String,
        fragment: Option<String>,
    },
    External(String),
}

impl Index {
    /// Raw HTML and fenced code are not Link events and never become ambient navigation instructions.
    pub(crate) fn parse(source: &str) -> Self {
        let mut index = Self::default();
        let mut heading: Option<(Range<usize>, String)> = None;
        let mut used = HashSet::new();
        let mut suffixes = HashMap::new();
        let mut image_depth = 0usize;
        let mut open_link: Option<usize> = None;
        for (event, range) in crate::preview::parser(source).into_offset_iter() {
            match event {
                Event::Start(tag @ Tag::Link { .. }) if image_depth == 0 => {
                    let Tag::Link {
                        dest_url,
                        link_type,
                        ..
                    } = &tag
                    else {
                        unreachable!()
                    };
                    // Email is a parsed semantic kind, not a filename heuristic: user@example.md remains
                    // a relative document only when written as an explicit ordinary Markdown destination.
                    let uri = if *link_type == LinkType::Email {
                        format!("mailto:{dest_url}")
                    } else {
                        dest_url.to_string()
                    };
                    open_link = Some(index.links.len());
                    index.links.push(Link {
                        uri,
                        href: rendered_href(tag),
                        caption: Vec::new(),
                        range,
                    });
                }
                Event::End(TagEnd::Link) if image_depth == 0 => {
                    if let Some(link) = open_link.take() {
                        index.links[link].range.end = index.links[link].range.end.max(range.end);
                    }
                }
                Event::Start(Tag::Image { .. }) => image_depth += 1,
                Event::End(TagEnd::Image) => image_depth = image_depth.saturating_sub(1),
                Event::Start(Tag::Heading { .. }) => heading = Some((range, String::new())),
                Event::End(TagEnd::Heading(_)) => {
                    if let Some((mut heading_range, text)) = heading.take() {
                        heading_range.end = heading_range.end.max(range.end);
                        index.headings.push(Heading {
                            slug: unique_slug(&text, &mut used, &mut suffixes),
                            range: heading_range,
                        });
                    }
                }
                Event::Text(text)
                | Event::Code(text)
                | Event::Html(text)
                | Event::InlineHtml(text) => {
                    if let Some((_, label)) = &mut heading {
                        label.push_str(&text);
                    }
                    if let Some(link) = open_link {
                        index.links[link].caption.push(Caption {
                            text: text.into_string(),
                            range,
                        });
                    }
                }
                Event::SoftBreak | Event::HardBreak => {
                    if let Some((_, label)) = &mut heading {
                        label.push(' ');
                    }
                    if let Some(link) = open_link {
                        index.links[link].caption.push(Caption {
                            text: " ".into(),
                            range,
                        });
                    }
                }
                _ => {}
            }
        }
        index
    }

    /// Only final visible readonly leaves declare activation metadata; containers never duplicate focus targets.
    /// CommonMark links outside image alt text are non-nested and source-ordered, allowing a bounded range scan.
    pub(crate) fn annotate(&self, nodes: &mut [ui::Node]) {
        for node in nodes {
            node.links.clear();
            match &mut node.kind {
                ui::Kind::Column { children } | ui::Kind::Row { children } => {
                    self.annotate(children)
                }
                ui::Kind::Scroll { content } => {
                    self.annotate(std::slice::from_mut(content.as_mut()))
                }
                ui::Kind::RichText { .. } | ui::Kind::Image { .. } | ui::Kind::Text { .. } => {
                    let Some(range) = node.source_range else {
                        continue;
                    };
                    if node.disabled || range.start == range.end {
                        continue;
                    }
                    let first = self
                        .links
                        .partition_point(|link| link.range.end <= range.start);
                    node.links = self.links[first..]
                        .iter()
                        .take_while(|link| link.range.start < range.end)
                        .filter(|link| !link.href.is_empty() && link.href.len() <= 4096)
                        .map(|link| {
                            let label = if let ui::Kind::Image { alt, .. } = &node.kind {
                                utf8_prefix(alt, 256).to_owned()
                            } else {
                                link.caption(range.start..range.end)
                            };
                            ui::LinkTarget {
                                uri: link.href.clone(),
                                label,
                            }
                        })
                        .collect();
                }
                _ => {}
            }
        }
    }

    /// Image-split rich fragments may cover just the visible label, so their parser ranges must intersect.
    /// Only the exact rendered href grants a match; return the parsed semantic URI for domain classification.
    /// A matching href elsewhere cannot grant an event from a raw-HTML or unrelated node.
    pub(crate) fn resolve(&self, nodes: &[ui::Node], id: &str, href: &str) -> Option<&str> {
        let Some(node) = find(nodes, id) else {
            return None;
        };
        let allowed = match node.kind {
            ui::Kind::RichText { .. } => true,
            ui::Kind::Image { .. } | ui::Kind::Text { .. } => {
                node.links.iter().any(|target| target.uri == href)
            }
            _ => false,
        };
        if !allowed || node.disabled {
            return None;
        }
        let Some(range) = &node.source_range else {
            return None;
        };
        self.links
            .iter()
            .find(|link| {
                link.href == href && link.range.start < range.end && range.start < link.range.end
            })
            .map(|link| link.uri.as_str())
    }

    /// Only a real parsed heading and an existing rendered source-mapped node can supply a reveal target.
    /// Image-bearing headings may have one fragment instead of an outer heading node.
    pub(super) fn heading(&self, nodes: &[ui::Node], fragment: &str) -> Option<String> {
        let heading = self
            .headings
            .iter()
            .find(|heading| heading.slug == fragment)?;
        mapped_heading(nodes, &heading.range).map(|node| node.id.clone())
    }
}

impl Link {
    /// Captions remain actual visible parser text, truncated as a UTF-8 prefix rather than byte-split or relabeled.
    fn caption(&self, range: Range<usize>) -> String {
        let mut label = String::new();
        for part in &self.caption {
            if part.range.start < range.end && range.start < part.range.end {
                let prefix = utf8_prefix(&part.text, 256 - label.len());
                label.push_str(prefix);
                if prefix.len() < part.text.len() || label.len() == 256 {
                    break;
                }
            }
        }
        label
    }
}

/// Byte quotas cannot split a Chinese character or emoji; empty captions remain available to host localization.
fn utf8_prefix(text: &str, limit: usize) -> &str {
    let mut end = limit.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Reuse the public writer rather than duplicating its href byte-escape table or URI decoding policy.
/// Rendering keeps its own escaping while the parsed semantic URI independently retains an email's mailto scheme.
fn rendered_href(tag: Tag<'_>) -> String {
    let mut markup = String::new();
    html::push_html(
        &mut markup,
        [Event::Start(tag), Event::End(TagEnd::Link)].into_iter(),
    );
    let mut attribute = markup
        .strip_prefix("<a href=\"")
        .and_then(|link| link.split_once('"').map(|(href, _)| href))
        .expect("the locked public writer emits a quoted href for every parsed Link");
    let mut href = String::with_capacity(attribute.len());
    // escape_href emits only these two HTML entities. Consume original input once: a literal entity
    // exposed after decoding &amp; must never be decoded again, and percent escapes are never touched.
    while !attribute.is_empty() {
        if let Some(rest) = attribute.strip_prefix("&amp;") {
            href.push('&');
            attribute = rest;
        } else if let Some(rest) = attribute.strip_prefix("&#x27;") {
            href.push('\'');
            attribute = rest;
        } else {
            let ch = attribute.chars().next().unwrap();
            href.push(ch);
            attribute = &attribute[ch.len_utf8()..];
        }
    }
    href
}

/// A browser URL is forwarded unchanged only after a click; queries are not a relative-file feature.
/// Percent decoding is strict and single-pass, so an encoded percent cannot become a second path traversal.
pub(super) fn destination(uri: &str) -> Result<Destination, Reason> {
    if uri.len() > 4096 || uri.chars().any(char::is_control) {
        return Err(Reason::InvalidUri);
    }
    if let Some((scheme, _)) = uri.split_once("://") {
        if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
            return Err(Reason::Unsupported);
        }
        api::NavigationTarget::ExternalUrl { url: uri.into() }
            .validate()
            .map_err(|_| Reason::InvalidUri)?;
        return Ok(Destination::External(uri.into()));
    }
    let (path_uri, fragment) = match uri.split_once('#') {
        Some((path, fragment)) => (path, Some(fragment_name(fragment)?)),
        None => (uri, None),
    };
    if path_uri.is_empty() {
        return fragment.map(Destination::Anchor).ok_or(Reason::Unsupported);
    }
    if path_uri.contains('?') {
        return Err(Reason::Unsupported);
    }
    let path = api::decode_uri_component(path_uri).map_err(|_| Reason::InvalidUri)?;
    if path.is_empty()
        || path.starts_with(['/', '\\'])
        || path.contains(':')
        || path.chars().any(char::is_control)
    {
        return Err(Reason::Unsupported);
    }
    let markdown = path.rsplit_once('.').is_some_and(|(_, extension)| {
        extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
    });
    if !markdown {
        return Err(Reason::Unsupported);
    }
    Ok(Destination::Relative {
        path: path_uri.into(),
        fragment,
    })
}

/// Fragment matching is exact after one UTF-8 decoding pass; it does not silently invent or normalize anchors.
fn fragment_name(fragment: &str) -> Result<String, Reason> {
    let fragment = api::decode_uri_component(fragment).map_err(|_| Reason::InvalidUri)?;
    if fragment.is_empty() || fragment.chars().any(char::is_control) {
        return Err(Reason::InvalidAnchor);
    }
    Ok(fragment)
}

/// Unicode letters/numbers and literal underscores/hyphens survive; whitespace runs become one separator.
/// A global used-name set prevents generated suffixes from colliding with naturally suffixed headings.
fn unique_slug(
    text: &str,
    used: &mut HashSet<String>,
    suffixes: &mut HashMap<String, usize>,
) -> String {
    let mut base = String::new();
    let mut whitespace = false;
    for ch in text.chars().flat_map(char::to_lowercase) {
        if ch.is_whitespace() {
            whitespace = true;
        } else if ch.is_alphanumeric() || matches!(ch, '_' | '-') {
            if whitespace && !base.is_empty() {
                base.push('-');
            }
            base.push(ch);
            whitespace = false;
        }
    }
    if base.is_empty() {
        base.push_str("section");
    }
    let suffix = suffixes.entry(base.clone()).or_default();
    let mut slug = base.clone();
    while used.contains(&slug) {
        *suffix += 1;
        slug = format!("{base}-{suffix}");
    }
    used.insert(slug.clone());
    slug
}

/// Prefer the complete heading block; a native image or rich fragment can represent a split heading's position.
fn mapped_heading<'a>(nodes: &'a [ui::Node], range: &Range<usize>) -> Option<&'a ui::Node> {
    for node in nodes {
        let mapped = node.source_range.as_ref().is_some_and(|mapped| {
            mapped.start >= range.start && mapped.end <= range.end && mapped.start < mapped.end
        });
        if mapped && !node.disabled {
            return Some(node);
        }
        let found = match &node.kind {
            ui::Kind::Column { children } | ui::Kind::Row { children } => {
                mapped_heading(children, range)
            }
            ui::Kind::Scroll { content } => {
                mapped_heading(std::slice::from_ref(content.as_ref()), range)
            }
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

/// Only public containers are navigated; node identities never encode trusted offsets or paths.
fn find<'a>(nodes: &'a [ui::Node], id: &str) -> Option<&'a ui::Node> {
    for node in nodes {
        if node.id == id {
            return Some(node);
        }
        let found = match &node.kind {
            ui::Kind::Column { children } | ui::Kind::Row { children } => find(children, id),
            ui::Kind::Scroll { content } => find(std::slice::from_ref(content.as_ref()), id),
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}
