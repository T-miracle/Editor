//! Schema-free attribute history and permissive SVG suggestions remain XML plugin policy.
use plugin_protocol::{
    api::TextRange,
    language::{CompletionItem, CompletionProposal, CompletionRequest},
};
use quick_xml::{
    NsReader, Reader,
    events::{BytesStart, Event},
    name::ResolveResult,
};
use std::collections::{BTreeMap, BTreeSet};

/// XML parsing is bounded by the public source quota and guest fuel; no filesystem or server is read.
pub(super) fn complete(request: CompletionRequest) -> CompletionProposal {
    let mut proposal = CompletionProposal {
        request: request.request,
        document: request.source.document.clone(),
        items: vec![],
    };
    let source = &request.source.text;
    if !request.valid() {
        return proposal;
    }
    // Catalogs resolve declared rule identifiers; their presence cannot constrain unrelated XML.
    // Matching associations and declarations retain native constraints unless this exact source failed to load.
    let mut constrained = associated(&request);
    // Appending a terminator lets the domain parser expose an unfinished final start tag, including EOF typing.
    let padded = format!("{source} />");
    let mut reader = NsReader::from_str(&padded);
    reader.config_mut().check_end_names = false;
    reader.config_mut().allow_unmatched_ends = true;
    let mut history: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut current = None;
    for _ in 0..16384 {
        let start = reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(Event::Start(tag) | Event::Empty(tag)) => {
                let end = (reader.buffer_position() as usize).min(source.len());
                let name = String::from_utf8_lossy(tag.name().as_ref()).into_owned();
                let attributes = tag
                    .attributes()
                    .with_checks(false)
                    .take(256)
                    .filter_map(Result::ok)
                    .map(|attribute| {
                        // Only an actual XML Schema instance namespace declaration supplies a rule source.
                        // An unrelated prefix with a schemaLocation-looking local name remains ordinary data.
                        let (namespace, local) = reader.resolver().resolve_attribute(attribute.key);
                        constrained |= matches!(namespace, ResolveResult::Bound(namespace)
                            if namespace.as_ref() == b"http://www.w3.org/2001/XMLSchema-instance")
                            && matches!(
                                local.as_ref(),
                                b"schemaLocation" | b"noNamespaceSchemaLocation"
                            )
                            && attribute
                                .value
                                .iter()
                                .any(|byte| !byte.is_ascii_whitespace());
                        String::from_utf8_lossy(attribute.key.as_ref()).into_owned()
                    })
                    .collect::<BTreeSet<_>>();
                history
                    .entry(name.clone())
                    .or_default()
                    .extend(attributes.iter().cloned());
                let name_end = start + 1 + tag.name().as_ref().len();
                if start < request.cursor && request.cursor >= name_end && request.cursor <= end {
                    current = attribute_range(source, name_end, end, request.cursor)
                        .map(|range| (name, attributes, range));
                }
            }
            Ok(Event::DocType(_)) => constrained = true,
            Ok(Event::PI(pi)) if pi.target() == b"xml-model" => constrained = true,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        if history.len() > 4096 {
            break;
        }
    }
    if constrained && !rules_unavailable(&request) {
        return proposal;
    }
    if let Some((name, present, range)) = current {
        let mut candidates = history.remove(&name).unwrap_or_default();
        if svg_enabled(&request) {
            candidates.extend(svg_attributes(&name));
        }
        for label in candidates
            .into_iter()
            .filter(|label| !present.contains(label) && label.len() <= 256)
            .take(256)
        {
            proposal.items.push(CompletionItem {
                new_text: format!("{label}=\"\""),
                label,
                replace: range,
            });
        }
    } else if svg_enabled(&request) {
        // The bundled schema uses a lax wildcard to avoid false errors for legal SVG extensions.
        // Its common global declarations are still useful suggestions at a real opening-tag caret.
        if let Some(range) = element_range(source, request.cursor) {
            for label in svg_elements().into_iter().take(256) {
                proposal.items.push(CompletionItem {
                    new_text: label.clone(),
                    label,
                    replace: range,
                });
            }
        }
    }
    proposal
}

/// Domain configuration decides which documents have explicit rule sources; unrelated associations stay inert.
fn associated(request: &CompletionRequest) -> bool {
    let Some(text) = request
        .settings
        .get("schema_associations")
        .and_then(|value| value.value.as_str())
    else {
        return false;
    };
    let Ok(values) = serde_json::from_str::<Vec<serde_json::Value>>(text) else {
        return true;
    };
    values.iter().any(|value| {
        value
            .get("pattern")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|pattern| {
                let mut pattern = pattern.replace('\\', "/");
                let native = native_document_path(&request.uri);
                // LemMinX treats literal relative patterns as suffixes by prepending **/.
                // Native absolute drive patterns are adapted separately and keep their full boundary.
                if !pattern.starts_with(['*', '?', '/']) && pattern.as_bytes().get(1) != Some(&b':')
                {
                    pattern.insert_str(0, "**/");
                }
                globset::GlobBuilder::new(&pattern)
                    .literal_separator(true)
                    .case_insensitive(
                        native
                            .as_ref()
                            .is_some_and(|path| path.as_bytes().get(1) == Some(&b':')),
                    )
                    .build()
                    .map(|glob| {
                        let matcher = glob.compile_matcher();
                        matcher.is_match(&request.source.document.path)
                            || matcher.is_match(
                                request
                                    .source
                                    .document
                                    .path
                                    .rsplit('/')
                                    .next()
                                    .unwrap_or(""),
                            )
                            || native.as_ref().is_some_and(|path| matcher.is_match(path))
                    })
                    .unwrap_or(true)
            })
    })
}

/// Decode a readonly logical file identity for exact Windows/UNC/Unix glob matching, without opening it.
fn native_document_path(uri: &str) -> Option<String> {
    let uri = url::Url::parse(uri).ok()?;
    if uri.scheme() != "file" {
        return None;
    }
    let path = percent_encoding::percent_decode_str(uri.path())
        .decode_utf8()
        .ok()?
        .into_owned();
    if let Some(host) = uri
        .host_str()
        .filter(|host| !host.is_empty() && *host != "localhost")
    {
        Some(format!("//{host}{path}"))
    } else if path.as_bytes().get(2) == Some(&b':') {
        Some(path[1..].to_owned())
    } else {
        Some(path)
    }
}

/// Only observed rule-loading failures relax constraints; syntax/content diagnostics and unknown state do not.
fn rules_unavailable(request: &CompletionRequest) -> bool {
    request.diagnostics.as_ref().is_some_and(|items| {
        items.iter().any(|item| {
            item.code
                .as_ref()
                .and_then(serde_json::Value::as_str)
                .is_some_and(|code| {
                    matches!(
                        code,
                        "DownloadResourceDisabled"
                            | "DownloadProblem"
                            | "schema_reference.4"
                            | "DTDNotFound"
                    )
                })
        })
    })
}

/// A tiny lexical cursor refinement works on a parser-confirmed start tag, never comments or CDATA.
fn attribute_range(source: &str, begin: usize, end: usize, cursor: usize) -> Option<TextRange> {
    let bytes = source.as_bytes();
    let mut at = begin.min(bytes.len());
    while at <= cursor && at < end {
        let whitespace = at;
        while at < end && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if cursor >= whitespace && cursor <= at && cursor > begin {
            return Some(TextRange {
                start: cursor,
                end: cursor,
            });
        }
        if at >= end || matches!(bytes[at], b'/' | b'>') {
            return None;
        }
        let start = at;
        while at < end && name_byte(bytes[at]) {
            at += 1;
        }
        if at == start {
            return None;
        }
        if cursor >= start && cursor <= at {
            return Some(TextRange { start, end: at });
        }
        while at < end && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if at >= end || bytes[at] != b'=' {
            return None;
        }
        at += 1;
        while at < end && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if at >= end || !matches!(bytes[at], b'\'' | b'"') {
            return None;
        }
        let quote = bytes[at];
        at += 1;
        while at < end && bytes[at] != quote {
            at += 1;
        }
        if cursor <= at {
            return None;
        }
        at += 1;
    }
    // An unfinished EOF tag ending in whitespace still represents an attribute insertion point.
    (cursor == source.len() && source[..cursor].ends_with(char::is_whitespace)).then_some(
        TextRange {
            start: cursor,
            end: cursor,
        },
    )
}

/// Element history is handled by LemMinX; SVG's wildcard model needs only common-name assistance.
fn element_range(source: &str, cursor: usize) -> Option<TextRange> {
    let start = source[..cursor].rfind('<')? + 1;
    let name = &source[start..cursor];
    if !name.bytes().all(name_byte) {
        return None;
    }
    // Parsing the prefix prevents `<` text inside a comment, CDATA or a quoted value from becoming a tag.
    let mut reader = Reader::from_str(&source[..start - 1]);
    reader.config_mut().check_end_names = false;
    reader.config_mut().allow_unmatched_ends = true;
    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    let mut end = cursor;
    while end < source.len() && name_byte(source.as_bytes()[end]) {
        end += 1;
    }
    Some(TextRange { start, end })
}

/// Names may contain qualified prefixes and non-ASCII bytes; final ranges must stay UTF-8 aligned.
fn name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"_:-.".contains(&byte) || byte >= 128
}

/// Explicitly disabled bundled assistance returns no SVG-only supplements.
fn svg_enabled(request: &CompletionRequest) -> bool {
    request
        .source
        .document
        .path
        .to_ascii_lowercase()
        .ends_with(".svg")
        && request
            .settings
            .get("svg_suggestions")
            .and_then(|value| value.value.as_bool())
            .unwrap_or(true)
}

/// Own lax schema declarations are the single source for shipped common elements and attributes.
fn svg_elements() -> BTreeSet<String> {
    schema_names("element", None)
}
fn svg_attributes(element: &str) -> BTreeSet<String> {
    let mut attributes = schema_names("attribute", Some("common"));
    let kind = match element.rsplit(':').next().unwrap_or(element) {
        "svg" | "symbol" => "viewport",
        "path" => "path",
        "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" => "shape",
        "text" | "tspan" | "textPath" => "text",
        "linearGradient" | "radialGradient" => "gradient",
        "stop" => "stop",
        "use" | "image" => "reference",
        _ => "common",
    };
    attributes.extend(schema_names("attribute", Some(kind)));
    attributes
}

/// A small embedded schema parse avoids maintaining separate suggestion lists or obtaining file authority.
fn schema_names(kind: &str, wanted_type: Option<&str>) -> BTreeSet<String> {
    let mut reader = Reader::from_str(include_str!("../schemas/svg.xsd"));
    let mut result = BTreeSet::new();
    let mut current_type = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(tag) | Event::Empty(tag)) => {
                if tag.local_name().as_ref() == b"complexType" {
                    current_type = attribute(&tag, b"name");
                }
                if tag.local_name().as_ref() == kind.as_bytes()
                    && (wanted_type.is_none() || wanted_type == current_type.as_deref())
                    && let Some(name) = attribute(&tag, b"name")
                {
                    result.insert(name);
                }
            }
            Ok(Event::End(tag)) if tag.local_name().as_ref() == b"complexType" => {
                current_type = None
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    result
}

/// Schema identifiers are owned UTF-8 strings; malformed declarations do not become suggestions.
fn attribute(tag: &BytesStart<'_>, name: &[u8]) -> Option<String> {
    tag.attributes()
        .filter_map(Result::ok)
        .find(|attribute| attribute.key.as_ref() == name)
        .and_then(|attribute| {
            std::str::from_utf8(&attribute.value)
                .ok()
                .map(str::to_owned)
        })
}
