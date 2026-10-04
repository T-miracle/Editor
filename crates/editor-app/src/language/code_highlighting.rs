//! Bounded read-only syntax captures use the selected plugin WASM without registry or LSP state.
//!
//! Native views own source/scene cancellation and map capture names to the current theme while
//! painting. This module owns only immutable grammar preparation and provider-generation guards.

use super::{
    plugins,
    providers::{self, GrammarProvider},
};
use std::{
    cmp::Reverse,
    collections::{BTreeSet, VecDeque},
    ops::{ControlFlow, Range},
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use tree_sitter::{ParseOptions, Query, QueryCursor, QueryCursorOptions, StreamingIterator as _};

mod injections;

pub(crate) const MAX_TEXT_BYTES: usize = 64 * 1024;
pub(crate) const MAX_TOKENS: usize = 4096;
const MAX_PREPARED: usize = 16;
const MAX_CAPTURE_NAME_BYTES: usize = 128;
const MAX_CAPTURE_BYTES: usize = 64 * 1024;
const MAX_INJECTION_DEPTH: usize = 4;
const MAX_LAYERS: usize = 32;

/// Unavailable injected grammars leave plain spans; stale or over-budget work rejects the whole job.
enum Failure {
    Unavailable,
    Stopped,
    Limit,
}

/// All nested providers share these budgets and the original process-wide selection generation.
struct Work<'a> {
    text: &'a str,
    cancelled: &'a AtomicBool,
    deadline: Instant,
    epoch: u64,
    captures: usize,
    capture_bytes: usize,
    injection_events: usize,
    injection_ranges: usize,
    injected_bytes: usize,
    layers: usize,
    next_pattern: usize,
}

impl Work<'_> {
    fn check(&self) -> Result<(), Failure> {
        if stopped(self.cancelled, self.deadline) || epoch() != self.epoch {
            Err(Failure::Stopped)
        } else {
            Ok(())
        }
    }

    /// Charge names before copying them, including repeated names from distinct matching nodes.
    fn capture(&mut self, name: &str) -> Result<(), Failure> {
        if self.captures == MAX_TOKENS
            || name.len() > MAX_CAPTURE_NAME_BYTES
            || self.capture_bytes.saturating_add(name.len()) > MAX_CAPTURE_BYTES
        {
            return Err(Failure::Limit);
        }
        self.captures += 1;
        self.capture_bytes += name.len();
        Ok(())
    }
}

/// One selected package and its non-reusable generation bind a background job to provider choice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Selection {
    pub provider: GrammarProvider,
    pub epoch: u64,
}

/// Captures are UTF-8 byte ranges into the unchanged input, not colors or mutable editor state.
///
/// Output is sorted and non-overlapping. Shorter captures win; equal lengths prefer later query
/// patterns/captures, and adjacent spans of the same capture merge. Native consumers only split
/// lines and apply the current theme. No range extends beyond the input or splits a character.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    pub range: Range<usize>,
    pub capture: String,
}

/// Retain query ordering until overlapping captures have been resolved independently of theme.
struct Capture {
    token: Token,
    pattern: usize,
}

struct Prepared {
    selection: Selection,
    grammar: Arc<plugins::LoadedGrammar>,
    query: String,
}

#[derive(Default)]
struct PreparedCache {
    epoch: u64,
    entries: VecDeque<Arc<Prepared>>,
}
static PREPARED: LazyLock<Mutex<PreparedCache>> =
    LazyLock::new(|| Mutex::new(PreparedCache::default()));

/// Resolve an enabled language ID, display name or selected extension from current declarations.
/// Unknown or ambiguous aliases return `None`; no built-in parser may serve as a fallback.
pub(crate) fn selected(language: &str) -> Option<Selection> {
    let (provider, epoch) = providers::code_provider(language)?;
    Some(Selection { provider, epoch })
}

/// Compare package and generation, rejecting even a remove/reinstall or A/B/A selection sequence.
pub(crate) fn is_current(selection: &Selection) -> bool {
    providers::code_is_current(&selection.provider, selection.epoch)
}

/// Native views can invalidate scenes when the last provider is withdrawn as well as installed.
pub(crate) fn epoch() -> u64 {
    providers::code_epoch()
}

/// Provider changes release prepared resources; older notifications cannot roll the cache back.
pub(super) fn invalidate_prepared(epoch: u64) {
    let mut cache = PREPARED.lock().unwrap();
    if epoch > cache.epoch {
        cache.entries.clear();
        cache.epoch = epoch;
    }
}

/// Capture a small immutable code block on a background worker, or return plain text on failure.
///
/// The caller supplies its scene cancellation flag and absolute deadline. Cold WASM loading and
/// query compilation have no progress callback, so their results are checked before and after;
/// parsing and query evaluation additionally cooperate with cancellation during their traversal.
/// Static injections use independently selected plugin modules with the same epoch. Missing or
/// failed injected providers leave parent styles; four levels, 32 parse layers and 64 KiB of total
/// injected ranges bound recursive work. Capture names are checked before any per-token copies:
/// each is at most 128 bytes, and raw/output copies each have a 64 KiB cumulative budget.
/// Over-budget, stale, invalid, failed or interrupted work returns no partial captures.
pub(crate) fn highlight(
    selection: &Selection,
    text: &str,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Vec<Token> {
    if text.len() > MAX_TEXT_BYTES || stopped(cancelled, deadline) || !is_current(selection) {
        return Vec::new();
    }
    let mut work = Work {
        text,
        cancelled,
        deadline,
        epoch: selection.epoch,
        captures: 0,
        capture_bytes: 0,
        injection_events: 0,
        injection_ranges: 0,
        injected_bytes: 0,
        layers: 0,
        next_pattern: 0,
    };
    let Ok(captures) = layer(selection, &[], 0, &mut work) else {
        return Vec::new();
    };
    let tokens = normalize(captures, cancelled, deadline);
    if work.check().is_err() || !is_current(selection) {
        Vec::new()
    } else {
        tokens
    }
}

/// Parse one layer at original source coordinates; recursive queries never create a mutable document.
fn layer(
    selection: &Selection,
    ranges: &[tree_sitter::Range],
    depth: usize,
    work: &mut Work<'_>,
) -> Result<Vec<Capture>, Failure> {
    work.check()?;
    if !is_current(selection) {
        return Err(Failure::Stopped);
    }
    if work.layers == MAX_LAYERS {
        return Err(Failure::Limit);
    }
    work.layers += 1;
    if depth > 0 {
        let bytes = ranges
            .iter()
            .map(|range| range.end_byte - range.start_byte)
            .sum::<usize>();
        if work.injected_bytes.saturating_add(bytes) > MAX_TEXT_BYTES {
            return Err(Failure::Limit);
        }
        work.injected_bytes += bytes;
    }
    let Some(prepared) = prepare(selection, work.cancelled, work.deadline) else {
        work.check()?;
        return Err(Failure::Unavailable);
    };
    work.check()?;
    let (mut parser, language) =
        plugins::readonly_parser(prepared.grammar.clone()).map_err(|_| Failure::Unavailable)?;
    work.check()?;
    if parser.set_language(&language).is_err()
        || (!ranges.is_empty() && parser.set_included_ranges(ranges).is_err())
    {
        return Err(Failure::Unavailable);
    }
    let cancelled = work.cancelled;
    let deadline = work.deadline;
    let mut parse_progress = |_: &tree_sitter::ParseState| progress(cancelled, deadline);
    let bytes = work.text.as_bytes();
    let mut read = |offset: usize, _: tree_sitter::Point| bytes.get(offset..).unwrap_or_default();
    let tree = parser
        .parse_with_options(
            &mut read,
            None,
            Some(ParseOptions::new().progress_callback(&mut parse_progress)),
        )
        .ok_or(Failure::Stopped)?;
    work.check()?;
    let query = Query::new(&language, &prepared.query).map_err(|_| Failure::Unavailable)?;
    work.check()?;
    // Later injected queries win equal-range ties over their parent query without theme coupling.
    let pattern = work.next_pattern;
    work.next_pattern = pattern
        .checked_add(query.pattern_count())
        .ok_or(Failure::Limit)?;
    let mut captures = collect(&query, &tree, ranges, pattern, work)?;
    {
        let requests = injections::requests(&prepared.grammar, &language, &tree, ranges, work)?;
        // Reaching the bound is harmless only when there is no deeper parse to perform.
        if depth >= MAX_INJECTION_DEPTH && !requests.is_empty() {
            return Err(Failure::Limit);
        }
        for request in requests {
            work.check()?;
            let Some(injected) = selected(&request.language).filter(|provider| {
                provider.epoch == work.epoch
                    && provider.provider.declaration.language == request.language
            }) else {
                continue;
            };
            match layer(&injected, &request.ranges, depth + 1, work) {
                Ok(injected) => captures.extend(injected),
                Err(Failure::Unavailable) => {}
                Err(failure) => return Err(failure),
            }
        }
    }
    work.check()?;
    Ok(captures)
}

/// A capture from included ranges must be split at each physical source gap before styling.
fn pieces(range: Range<usize>, included: &[tree_sitter::Range]) -> Vec<Range<usize>> {
    if included.is_empty() {
        return vec![range];
    }
    included
        .iter()
        .filter_map(|included| {
            let start = range.start.max(included.start_byte);
            let end = range.end.min(included.end_byte);
            (start < end).then_some(start..end)
        })
        .collect()
}

/// Raw captures and query match state are bounded before normalization can allocate any events.
fn collect(
    query: &Query,
    tree: &tree_sitter::Tree,
    included: &[tree_sitter::Range],
    pattern: usize,
    work: &mut Work<'_>,
) -> Result<Vec<Capture>, Failure> {
    let mut cursor = QueryCursor::new();
    cursor.set_match_limit(MAX_TOKENS as u32);
    let cancelled = work.cancelled;
    let deadline = work.deadline;
    let mut query_progress = |_: &tree_sitter::QueryCursorState| progress(cancelled, deadline);
    let text = work.text;
    let mut captures = Vec::new();
    {
        let mut matches = cursor.matches_with_options(
            query,
            tree.root_node(),
            text.as_bytes(),
            QueryCursorOptions::new().progress_callback(&mut query_progress),
        );
        while let Some(found) = matches.next() {
            work.check()?;
            for capture in found.captures {
                let name = query.capture_names()[capture.index as usize];
                let range = capture.node.byte_range();
                if range.end > text.len()
                    || !text.is_char_boundary(range.start)
                    || !text.is_char_boundary(range.end)
                {
                    return Err(Failure::Unavailable);
                }
                if range.is_empty() {
                    work.capture("")?;
                    continue;
                }
                for range in pieces(range, included) {
                    work.capture(name)?;
                    captures.push(Capture {
                        token: Token {
                            range,
                            capture: name.to_owned(),
                        },
                        pattern: pattern + found.pattern_index,
                    });
                }
            }
        }
    }
    if cursor.did_exceed_match_limit() {
        return Err(Failure::Limit);
    }
    work.check()?;
    Ok(captures)
}

/// Sweep at most two events per raw capture; never expand overlapping styles into an unbounded
/// cross product. Priority uses source span length, then query order rather than theme availability.
fn normalize(captures: Vec<Capture>, cancelled: &AtomicBool, deadline: Instant) -> Vec<Token> {
    let raw_bytes = captures.iter().try_fold(0usize, |total, capture| {
        let bytes = capture.token.capture.len();
        (bytes <= MAX_CAPTURE_NAME_BYTES).then(|| total.saturating_add(bytes))
    });
    if captures.len() > MAX_TOKENS || raw_bytes.is_none_or(|bytes| bytes > MAX_CAPTURE_BYTES) {
        return Vec::new();
    }
    let mut events = Vec::with_capacity(captures.len() * 2);
    for (index, capture) in captures.iter().enumerate() {
        events.push((capture.token.range.start, index, true));
        events.push((capture.token.range.end, index, false));
    }
    events.sort_unstable();
    let priority = |index: usize| {
        let capture = &captures[index];
        (
            capture.token.range.len(),
            Reverse(capture.pattern),
            Reverse(index),
        )
    };
    let mut active = BTreeSet::new();
    let mut result: Vec<Token> = Vec::new();
    let mut output_bytes = 0;
    let mut event = 0;
    while event < events.len() {
        if stopped(cancelled, deadline) {
            return Vec::new();
        }
        let offset = events[event].0;
        while event < events.len() && events[event].0 == offset {
            let (_, index, start) = events[event];
            if start {
                active.insert(priority(index));
            } else {
                active.remove(&priority(index));
            }
            event += 1;
        }
        let Some(&(_, _, Reverse(index))) = active.first() else {
            continue;
        };
        let Some(&(end, _, _)) = events.get(event) else {
            break;
        };
        let capture = &captures[index].token.capture;
        if let Some(previous) = result.last_mut() {
            if previous.range.end == offset && &previous.capture == capture {
                previous.range.end = end;
                continue;
            }
        }
        if result.len() == MAX_TOKENS || output_bytes + capture.len() > MAX_CAPTURE_BYTES {
            return Vec::new();
        }
        output_bytes += capture.len();
        result.push(Token {
            range: offset..end,
            capture: capture.clone(),
        });
    }
    result
}

/// Prepare outside the cache lock so provider changes and other cancellation checks never wait
/// for WASM initialization. The cache retains at most sixteen validated modules in one generation.
fn prepare(
    selection: &Selection,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Option<Arc<Prepared>> {
    {
        let cache = PREPARED.lock().unwrap();
        if let Some(prepared) = cache
            .entries
            .iter()
            .find(|item| item.selection == *selection)
        {
            return Some(prepared.clone());
        }
    }
    if stopped(cancelled, deadline) || !is_current(selection) {
        return None;
    }
    let (grammar, query) = plugins::prepare_dynamic(&selection.provider).ok()?;
    if stopped(cancelled, deadline) || !is_current(selection) {
        return None;
    }
    let prepared = Arc::new(Prepared {
        selection: selection.clone(),
        grammar,
        query,
    });
    let mut cache = PREPARED.lock().unwrap();
    // A delayed initialization cannot revive resources retired by a newer provider generation.
    if selection.epoch < cache.epoch {
        return None;
    }
    if selection.epoch > cache.epoch {
        cache.entries.clear();
        cache.epoch = selection.epoch;
    }
    if let Some(existing) = cache
        .entries
        .iter()
        .find(|item| item.selection == *selection)
    {
        return Some(existing.clone());
    }
    if cache.entries.len() == MAX_PREPARED {
        cache.entries.pop_front();
    }
    cache.entries.push_back(prepared.clone());
    Some(prepared)
}

/// Tree-sitter's parse/query callbacks share the same cancellation and wall-clock policy.
fn progress(cancelled: &AtomicBool, deadline: Instant) -> ControlFlow<()> {
    if stopped(cancelled, deadline) {
        ControlFlow::Break(())
    } else {
        ControlFlow::Continue(())
    }
}

/// A scene flag and absolute deadline apply consistently to every phase of read-only work.
fn stopped(cancelled: &AtomicBool, deadline: Instant) -> bool {
    cancelled.load(Ordering::Acquire) || Instant::now() >= deadline
}

#[cfg(test)]
mod tests;
