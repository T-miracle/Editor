//! XML structure policy parses only the host's immutable source; it never opens a file or contacts a server.
use plugin_protocol::{
    api::TextRange,
    structure::{Icon, Node, Proposal, Request},
};
use quick_xml::{
    Reader, XmlVersion,
    events::{BytesStart, Event},
};

/// Return elements in document order, with exact opening-name destinations and separate complete-structure folds.
/// Incomplete elements remain browseable while typing; only confirmed complete ranges become folds.
pub(super) fn describe(request: Request) -> Proposal {
    let mut proposal = Proposal {
        request: request.request,
        document: request.source.document.clone(),
        nodes: vec![],
        folds: vec![],
    };
    if !request.valid() {
        return proposal;
    }
    let source = &request.source.text;
    let mut reader = Reader::from_str(source);
    // Attribute normalization follows the document's declared XML version, without resolving external entities.
    let mut version = XmlVersion::Implicit1_0;
    let mut stack: Vec<Node> = Vec::new();
    let mut count = 0;
    for _ in 0..16384 {
        let start = reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(event @ (Event::Start(_) | Event::Empty(_))) => {
                if count >= 4096 || stack.len() >= 127 {
                    break;
                }
                let empty = matches!(&event, Event::Empty(_));
                let tag = match event {
                    Event::Start(tag) | Event::Empty(tag) => tag,
                    _ => unreachable!(),
                };
                let end = reader.buffer_position() as usize;
                // A leading UTF-8 BOM is consumed before the first event; derive the actual '<' from its raw span.
                let raw: &[u8] = tag.as_ref();
                let Some(start) = end.checked_sub(raw.len() + if empty { 3 } else { 2 }) else {
                    break;
                };
                let Some(node) = element(&reader, &tag, version, start, source.len()) else {
                    break;
                };
                count += 1;
                // Empty events end in />. Their wrapped attributes can be folded too.
                if empty {
                    let mut node = node;
                    node.range.end = end;
                    add_fold(&mut proposal, source, node.range);
                    append(node, &mut stack, &mut proposal.nodes);
                } else {
                    stack.push(node);
                }
            }
            Ok(Event::End(_)) => {
                // Reader's name checking rejects mismatched pairs before reaching this branch.
                let Some(mut node) = stack.pop() else {
                    break;
                };
                node.range.end = reader.buffer_position() as usize;
                add_fold(&mut proposal, source, node.range);
                append(node, &mut stack, &mut proposal.nodes);
            }
            Ok(Event::Comment(_) | Event::CData(_) | Event::DocType(_)) => {
                add_fold(
                    &mut proposal,
                    source,
                    TextRange {
                        start,
                        end: reader.buffer_position() as usize,
                    },
                );
            }
            Ok(Event::Decl(declaration)) => {
                version = declaration.xml_version().unwrap_or(XmlVersion::Implicit1_0)
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    // An unfinished parent contains its parsed descendants, but receives no speculative fold.
    while let Some(node) = stack.pop() {
        append(node, &mut stack, &mut proposal.nodes);
    }
    proposal
        .folds
        .sort_by_key(|range| (range.start, std::cmp::Reverse(range.end)));
    proposal
}

/// Build a display name from id/name without confusing it with the source's precise tag-name range.
fn element(
    reader: &Reader<&[u8]>,
    tag: &BytesStart<'_>,
    version: XmlVersion,
    start: usize,
    length: usize,
) -> Option<Node> {
    let tag_name = std::str::from_utf8(tag.name().as_ref()).ok()?.to_owned();
    let definition = TextRange {
        start: start + 1,
        end: start + 1 + tag_name.len(),
    };
    if tag_name.is_empty() || definition.end > length || tag_name.len() > 256 {
        return None;
    }
    let mut id = None;
    let mut name = None;
    for attribute in tag
        .attributes()
        .with_checks(false)
        .take(256)
        .filter_map(Result::ok)
    {
        if !matches!(attribute.key.as_ref(), b"id" | b"name") {
            continue;
        }
        // Custom DTD entities do not require file access: retain their literal spelling for a display-only label.
        let value = attribute
            .decoded_and_normalized_value(version, reader.decoder())
            .map(|value| value.into_owned())
            .unwrap_or_else(|_| String::from_utf8_lossy(attribute.value.as_ref()).into_owned());
        match attribute.key.as_ref() {
            b"id" => id = Some(value),
            b"name" => name = Some(value),
            _ => {}
        }
    }
    let mut label = if let Some(id) = id.filter(|value| !value.trim().is_empty()) {
        format!("{tag_name} #{id}")
    } else if let Some(name) = name.filter(|value| !value.trim().is_empty()) {
        format!("{tag_name} [name={name}]")
    } else {
        tag_name
    };
    // Labels are presentation only: bound them without splitting UTF-8 or retaining control characters.
    label.retain(|character| !character.is_control());
    let mut end = label.len().min(512);
    while !label.is_char_boundary(end) {
        end -= 1;
    }
    label.truncate(end);
    Some(Node {
        name: label,
        kind: "element".into(),
        icon: Some(Icon {
            light: "icons/element.svg".into(),
            dark: None,
        }),
        range: TextRange { start, end: length },
        definition,
        children: vec![],
    })
}

/// Completed descendants belong to their current parent; plain attributes/text/comments never become nodes.
fn append(node: Node, stack: &mut [Node], roots: &mut Vec<Node>) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else {
        roots.push(node);
    }
}

/// Native folding is line-based; only multi-line, parser-confirmed XML structures are contributed.
fn add_fold(proposal: &mut Proposal, source: &str, range: TextRange) {
    if proposal.folds.len() < 4096
        && source
            .get(range.start..range.end)
            .is_some_and(|text| text.contains('\n'))
    {
        proposal.folds.push(range);
    }
}
