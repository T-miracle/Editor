//! Static injection queries produce bounded original-coordinate ranges without registry fallback.
//!
//! Included ranges respect parent exclusions and the standard include-children/combined properties;
//! target resolution and parser lifetime remain with the caller's shared provider epoch and budgets.

use super::*;

/// One nested parse uses one exact declared language identity and physical source ranges.
pub(super) struct Request {
    pub language: String,
    pub ranges: Vec<tree_sitter::Range>,
}

struct Group {
    pattern: usize,
    combined: bool,
    request: Request,
}

/// Only validated static target properties can request a nested parser. Every content event and
/// physical range is charged before allocation, and separate patterns retain separate combined trees.
pub(super) fn requests(
    grammar: &plugins::LoadedGrammar,
    language: &tree_sitter::Language,
    tree: &tree_sitter::Tree,
    parents: &[tree_sitter::Range],
    work: &mut Work<'_>,
) -> Result<Vec<Request>, Failure> {
    let (source, allowed) = grammar.readonly_injections();
    if source.is_empty() {
        return Ok(Vec::new());
    }
    work.check()?;
    let query = Query::new(language, source).map_err(|_| Failure::Unavailable)?;
    work.check()?;
    let Some(content) = query
        .capture_names()
        .iter()
        .position(|name| *name == "injection.content")
    else {
        return Ok(Vec::new());
    };
    let mut groups: Vec<Group> = Vec::new();
    let mut cursor = QueryCursor::new();
    cursor.set_match_limit(MAX_TOKENS as u32);
    let cancelled = work.cancelled;
    let deadline = work.deadline;
    let text = work.text;
    let mut query_progress = |_: &tree_sitter::QueryCursorState| progress(cancelled, deadline);
    {
        let mut matches = cursor.matches_with_options(
            &query,
            tree.root_node(),
            text.as_bytes(),
            QueryCursorOptions::new().progress_callback(&mut query_progress),
        );
        while let Some(found) = matches.next() {
            work.check()?;
            let settings = query.property_settings(found.pattern_index);
            let target = settings
                .iter()
                .find(|property| property.key.as_ref() == "injection.language")
                .and_then(|property| property.value.as_deref())
                .ok_or(Failure::Unavailable)?;
            if !allowed.iter().any(|allowed| allowed == target) {
                return Err(Failure::Unavailable);
            }
            let combined = settings
                .iter()
                .any(|property| property.key.as_ref() == "injection.combined");
            let include_children = settings
                .iter()
                .any(|property| property.key.as_ref() == "injection.include-children");
            let mut ranges = Vec::new();
            for capture in found
                .captures
                .iter()
                .filter(|capture| capture.index as usize == content)
            {
                if work.injection_events == MAX_TOKENS {
                    return Err(Failure::Limit);
                }
                work.injection_events += 1;
                ranges.extend(content_ranges(
                    capture.node,
                    include_children,
                    parents,
                    work,
                )?);
            }
            if ranges.is_empty() {
                continue;
            }
            if combined {
                if let Some(group) = groups
                    .iter_mut()
                    .find(|group| group.combined && group.pattern == found.pattern_index)
                {
                    group.request.ranges.extend(ranges);
                    continue;
                }
            }
            if groups.len() == MAX_LAYERS {
                return Err(Failure::Limit);
            }
            groups.push(Group {
                pattern: found.pattern_index,
                combined,
                request: Request {
                    language: target.to_owned(),
                    ranges,
                },
            });
        }
    }
    if cursor.did_exceed_match_limit() {
        return Err(Failure::Limit);
    }
    work.check()?;
    let mut result = Vec::with_capacity(groups.len());
    for mut group in groups {
        // Overlapping matches cannot be passed to Tree-sitter; union their source intervals first.
        group
            .request
            .ranges
            .sort_unstable_by_key(|range| (range.start_byte, range.end_byte));
        let mut ranges: Vec<tree_sitter::Range> = Vec::new();
        for range in group.request.ranges {
            if let Some(previous) = ranges.last_mut() {
                if range.start_byte <= previous.end_byte {
                    if range.end_byte > previous.end_byte {
                        previous.end_byte = range.end_byte;
                        previous.end_point = range.end_point;
                    }
                    continue;
                }
            }
            ranges.push(range);
        }
        group.request.ranges = ranges;
        result.push(group.request);
    }
    work.check()?;
    Ok(result)
}

/// Default injection semantics exclude direct children, including their anonymous punctuation.
/// `include-children` retains the whole node; parent included ranges still constrain either form.
fn content_ranges(
    node: tree_sitter::Node<'_>,
    include_children: bool,
    parents: &[tree_sitter::Range],
    work: &mut Work<'_>,
) -> Result<Vec<tree_sitter::Range>, Failure> {
    let mut ranges = Vec::new();
    if include_children {
        append(node.range(), parents, &mut ranges, work)?;
    } else {
        let mut byte = node.start_byte();
        let mut point = node.start_position();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            work.check()?;
            append(
                tree_sitter::Range {
                    start_byte: byte,
                    start_point: point,
                    end_byte: child.start_byte(),
                    end_point: child.start_position(),
                },
                parents,
                &mut ranges,
                work,
            )?;
            byte = child.end_byte();
            point = child.end_position();
        }
        append(
            tree_sitter::Range {
                start_byte: byte,
                start_point: point,
                end_byte: node.end_byte(),
                end_point: node.end_position(),
            },
            parents,
            &mut ranges,
            work,
        )?;
    }
    Ok(ranges)
}

/// Clip exclusions against the parent's physical ranges without reconstructing text or positions.
fn append(
    range: tree_sitter::Range,
    parents: &[tree_sitter::Range],
    ranges: &mut Vec<tree_sitter::Range>,
    work: &mut Work<'_>,
) -> Result<(), Failure> {
    if range.start_byte >= range.end_byte {
        return Ok(());
    }
    if parents.is_empty() {
        return push(range, ranges, work);
    }
    for parent in parents {
        work.check()?;
        let (start_byte, start_point) = if parent.start_byte > range.start_byte {
            (parent.start_byte, parent.start_point)
        } else {
            (range.start_byte, range.start_point)
        };
        let (end_byte, end_point) = if parent.end_byte < range.end_byte {
            (parent.end_byte, parent.end_point)
        } else {
            (range.end_byte, range.end_point)
        };
        if start_byte < end_byte {
            push(
                tree_sitter::Range {
                    start_byte,
                    start_point,
                    end_byte,
                    end_point,
                },
                ranges,
                work,
            )?;
        }
    }
    Ok(())
}

/// Range count and UTF-8 guards prevent a tiny query from allocating many excluded child gaps.
fn push(
    range: tree_sitter::Range,
    ranges: &mut Vec<tree_sitter::Range>,
    work: &mut Work<'_>,
) -> Result<(), Failure> {
    if work.injection_ranges == MAX_TOKENS {
        return Err(Failure::Limit);
    }
    if range.end_byte > work.text.len()
        || !work.text.is_char_boundary(range.start_byte)
        || !work.text.is_char_boundary(range.end_byte)
    {
        return Err(Failure::Unavailable);
    }
    work.injection_ranges += 1;
    ranges.push(range);
    Ok(())
}
