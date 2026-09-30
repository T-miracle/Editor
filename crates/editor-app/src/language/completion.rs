//! Adapts plugin LSP completion results to the editor's built-in suggestion menu.

use super::navigation::{LanguageServer, file_uri, position_at_byte};
use anyhow::Result;
use gpui_base::input::{CompletionProvider, Rope};
use gpui_kit::gpui::{App, Task, Window};
use lsp_types::{
    CompletionContext, CompletionResponse, CompletionTextEdit, Position, Range, TextEdit, Uri,
};
use std::{ops, path::Path, sync::Arc};

/// Supplies suggestions from the plugin server for one editor document.
pub struct LanguageCompletionProvider {
    server: Arc<LanguageServer>,
    document_uri: Uri,
    triggers: Vec<String>,
    whitespace_suffixes: Vec<String>,
}

impl LanguageCompletionProvider {
    /// Keeps language-specific punctuation in the plugin manifest.
    pub fn new(path: &Path, server: Arc<LanguageServer>) -> Option<Self> {
        Some(Self {
            document_uri: file_uri(path)?,
            triggers: server.completion_triggers().to_vec(),
            whitespace_suffixes: server.completion_after_whitespace().to_vec(),
            server,
        })
    }
}

impl CompletionProvider for LanguageCompletionProvider {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _trigger: CompletionContext,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<CompletionResponse>> {
        // Capture the exact text and UTF-16 range used by this asynchronous request.
        let source = text.to_string();
        // GPUI supplies a byte offset, including when identifiers contain non-ASCII text.
        let byte_offset = offset.min(text.len());
        // Space triggers are meaningful only after a plugin-declared type context.
        if source[..byte_offset].ends_with(' ')
            && !whitespace_context_matches(&source[..byte_offset], &self.whitespace_suffixes)
        {
            return Task::ready(Ok(CompletionResponse::Array(Vec::new())));
        }
        let position = position_at_byte(&source, byte_offset);
        let prefix = identifier_prefix(&source, byte_offset);
        let start = position_at_byte(&source, byte_offset - prefix.len());
        let server = self.server.clone();
        let document_uri = self.document_uri.clone();
        cx.background_executor()
            .scheduler_executor()
            .spawn_dedicated(move |_| async move {
                let result = server
                    .completions(document_uri, source, position)
                    .map(|response| rank_completions(response, &prefix, start, position));
                if let Err(error) = &result {
                    tracing::warn!(%error, "language completion request failed");
                }
                result
            })
    }

    fn is_completion_trigger(&self, _offset: usize, new_text: &str, _cx: &mut App) -> bool {
        // Ordinary identifiers, plugin punctuation, and declared spaces open suggestions.
        !new_text.is_empty()
            && (new_text
                .chars()
                .all(|character| character.is_alphanumeric() || character == '_')
                || self.triggers.iter().any(|trigger| trigger == new_text)
                || (new_text == " " && !self.whitespace_suffixes.is_empty()))
    }
}

/// Check the current line so spaces elsewhere do not send needless LSP requests.
fn whitespace_context_matches(before_cursor: &str, suffixes: &[String]) -> bool {
    let line = before_cursor.rsplit('\n').next().unwrap_or(before_cursor);
    let before_space = line.trim_end_matches(' ');
    suffixes.iter().any(|suffix| before_space.ends_with(suffix))
}

/// Extract only the identifier being typed, excluding member and path punctuation.
pub(crate) fn identifier_prefix(source: &str, byte_offset: usize) -> String {
    source[..byte_offset]
        .chars()
        .rev()
        .take_while(|character| character.is_alphanumeric() || *character == '_')
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

/// Filter irrelevant server candidates and put closer fuzzy matches first.
pub(crate) fn rank_completions(
    response: CompletionResponse,
    prefix: &str,
    start: Position,
    end: Position,
) -> CompletionResponse {
    let items = match response {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    if prefix.is_empty() {
        return CompletionResponse::Array(items);
    }

    let mut ranked: Vec<_> = items
        .into_iter()
        .enumerate()
        .filter_map(|(index, mut item)| {
            let score = [&item.label, item.filter_text.as_deref().unwrap_or("")]
                .into_iter()
                .filter_map(|text| match_score(prefix, text))
                .min()?;
            let original_order = item.sort_text.clone().unwrap_or_else(|| item.label.clone());
            let needs_import = item
                .additional_text_edits
                .as_ref()
                .is_some_and(|edits| !edits.is_empty());
            // GPUI Kit appends insertText without a textEdit; give it the typed range.
            if item.text_edit.is_none() {
                item.text_edit = Some(CompletionTextEdit::Edit(TextEdit {
                    range: Range { start, end },
                    new_text: item
                        .insert_text
                        .clone()
                        .unwrap_or_else(|| item.label.clone()),
                }));
            }
            Some(((score, needs_import, original_order, index), item))
        })
        .collect();
    ranked.sort_by(|left, right| left.0.cmp(&right.0));
    CompletionResponse::Array(ranked.into_iter().map(|(_, item)| item).collect())
}

/// Score exact and prefix matches before ordered-character fuzzy matches.
fn match_score(query: &str, candidate: &str) -> Option<(u8, usize)> {
    let folded_query = query.to_lowercase();
    let folded_candidate = candidate.to_lowercase();
    if folded_candidate == folded_query {
        return Some((0, 0));
    }
    if folded_candidate.starts_with(&folded_query) {
        return Some((1, 0));
    }
    let ranges = matching_ranges(query, candidate)?;
    let mut last = 0;
    let mut gaps = 0;
    for range in ranges {
        // Smaller gaps keep nearby matching letters ahead of distant matches.
        gaps += range.start.saturating_sub(last);
        last = range.end;
    }
    Some((2, gaps))
}

/// Match ordered characters case-insensitively and retain UTF-8 ranges for highlighting.
pub(crate) fn matching_ranges(query: &str, candidate: &str) -> Option<Vec<ops::Range<usize>>> {
    // Case folding can expand a character; every folded codepoint retains its original span.
    let mut characters = candidate.char_indices().flat_map(|(index, character)| {
        character
            .to_lowercase()
            .map(move |folded| (folded, index..index + character.len_utf8()))
    });
    let mut ranges: Vec<ops::Range<usize>> = Vec::new();
    for character in query.chars().flat_map(char::to_lowercase) {
        let (_, range) = characters.find(|(current, _)| *current == character)?;
        if let Some(previous) = ranges
            .last_mut()
            .filter(|previous| previous.end >= range.start)
        {
            // Adjacent matches form one highlight, including expanded Unicode characters.
            previous.end = previous.end.max(range.end);
        } else {
            ranges.push(range);
        }
    }
    Some(ranges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::CompletionItem;

    /// Exact and prefix matches rank before subsequences, which still remain selectable.
    #[test]
    fn completion_fuzzy_ranking_and_replacement_range() {
        let items = ["println", "push_str", "PN", "pnext", "unrelated"]
            .into_iter()
            .map(|label| CompletionItem {
                label: label.into(),
                ..Default::default()
            })
            .collect();
        let start = Position::new(0, 3);
        let end = Position::new(0, 5);
        let CompletionResponse::Array(items) =
            rank_completions(CompletionResponse::Array(items), "pn", start, end)
        else {
            unreachable!();
        };
        assert_eq!(
            items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            ["PN", "pnext", "println"]
        );
        // Fuzzy insertion replaces the typed abbreviation instead of appending to it.
        assert!(items.iter().all(|item| matches!(
            &item.text_edit,
            Some(CompletionTextEdit::Edit(edit)) if edit.range == Range { start, end }
        )));
    }

    /// Server filter text can match an abbreviation even when its displayed label differs.
    #[test]
    fn completion_fuzzy_filter_text_is_respected() {
        let CompletionResponse::Array(items) = rank_completions(
            CompletionResponse::Array(vec![CompletionItem {
                label: "display label".into(),
                filter_text: Some("HashMap".into()),
                ..Default::default()
            }]),
            "hmp",
            Position::default(),
            Position::new(0, 3),
        ) else {
            unreachable!();
        };
        assert_eq!(items.len(), 1);
    }

    /// Highlight actual matched letters and never split a non-ASCII glyph at a byte boundary.
    #[test]
    fn completion_fuzzy_highlights_follow_matching_characters() {
        assert_eq!(matching_ranges("pn", "println"), Some(vec![0..1, 3..4]));
        assert_eq!(matching_ranges("hm", "HashMap"), Some(vec![0..1, 4..5]));
        assert_eq!(matching_ranges("变量", "变更变量"), Some(vec![0..3, 9..12]));
        assert_eq!(matching_ranges("i\u{307}v", "İValue"), Some(vec![0..3]));
        assert_eq!(matching_ranges("np", "println"), None);
        assert_eq!(matching_ranges("", "println"), Some(vec![]));
    }
}
