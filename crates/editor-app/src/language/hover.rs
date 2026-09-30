//! Connects the editor's hover popover to a plugin language server.

use super::navigation::{LanguageServer, file_uri, position_at_byte};
use anyhow::Result;
use gpui_base::input::{HoverProvider, Rope};
use gpui_kit::gpui::{App, Task, Window};
use lsp_types::{Hover, HoverContents, MarkedString, MarkupContent, MarkupKind, Uri};
use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

/// Shares an in-flight hover result even when the editor cancels a mouse-move waiter.
type SharedHoverResult = Arc<Mutex<Option<std::result::Result<Option<Hover>, String>>>>;

/// Keeps the latest document and symbol as the only reusable hover request.
#[derive(Default)]
struct HoverCache {
    generation: u64,
    current: Option<CachedHover>,
}

/// A completed positive result can also serve later moves over the same symbol.
struct CachedHover {
    source: String,
    symbol_start: usize,
    symbol_end: usize,
    result: SharedHoverResult,
}

/// Fetches symbol information for one document through its shared LSP session.
pub struct LanguageHoverProvider {
    server: Arc<LanguageServer>,
    document_uri: Uri,
    cache: Arc<Mutex<HoverCache>>,
}

impl LanguageHoverProvider {
    /// Uses the same file URI as navigation and completion requests.
    pub fn new(path: &Path, server: Arc<LanguageServer>) -> Option<Self> {
        Some(Self {
            server,
            document_uri: file_uri(path)?,
            cache: Arc::new(Mutex::new(HoverCache::default())),
        })
    }
}

impl HoverProvider for LanguageHoverProvider {
    fn hover(
        &self,
        text: &Rope,
        offset: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<Option<Hover>>> {
        // Mouse hit testing returns caret boundaries; query inside the symbol instead.
        let Some((symbol_start, symbol_end)) = symbol_at(text, offset) else {
            return Task::ready(Ok(None));
        };
        let source = text.to_string();
        // Every caret boundary inside one identifier must address the same LSP symbol.
        let byte_offset = text.char_to_byte_idx(symbol_start);
        let position = position_at_byte(&source, byte_offset);
        let symbol_range = lsp_types::Range::new(
            position_at_byte(&source, text.char_to_byte_idx(symbol_start)),
            position_at_byte(&source, text.char_to_byte_idx(symbol_end)),
        );
        let (shared_result, generation, start_request) = {
            let mut cache = self.cache.lock().expect("hover cache lock poisoned");
            if let Some(current) = &cache.current
                && current.source == source
                && current.symbol_start == symbol_start
                && current.symbol_end == symbol_end
            {
                (current.result.clone(), cache.generation, false)
            } else {
                // Replacing the symbol invalidates work still waiting in the debounce period.
                cache.generation = cache.generation.wrapping_add(1);
                let result = Arc::new(Mutex::new(None));
                cache.current = Some(CachedHover {
                    source: source.clone(),
                    symbol_start,
                    symbol_end,
                    result: result.clone(),
                });
                (result, cache.generation, true)
            }
        };

        if start_request {
            let server = self.server.clone();
            let document_uri = self.document_uri.clone();
            let cache = self.cache.clone();
            let result_for_worker = shared_result.clone();
            // This task outlives individual mouse-move waiters, which GPUI replaces on movement.
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
                let is_current =
                    cache.lock().expect("hover cache lock poisoned").generation == generation;
                let result = if is_current {
                    cx.background_executor()
                        .scheduler_executor()
                        .spawn_dedicated(move |_| async move {
                            fetch_hover(server, document_uri, source, position, symbol_range)
                        })
                        .await
                } else {
                    Ok(None)
                };
                let result = result.map_err(|error| error.to_string());
                let retry_on_next_hover = !matches!(result, Ok(Some(_)));
                *result_for_worker
                    .lock()
                    .expect("hover result lock poisoned") = Some(result);
                if retry_on_next_hover {
                    // Empty or failed responses must not be cached while the server is starting.
                    let mut cache = cache.lock().expect("hover cache lock poisoned");
                    if cache.generation == generation {
                        cache.current = None;
                    }
                }
            })
            .detach();
        }

        cx.spawn(async move |cx| {
            // The editor's own 150 ms delay runs concurrently with this timer.
            // Keep the card hidden for one second even when the result is cached.
            cx.background_executor().timer(Duration::from_secs(1)).await;
            loop {
                if let Some(result) = shared_result
                    .lock()
                    .expect("hover result lock poisoned")
                    .clone()
                {
                    return result.map_err(anyhow::Error::msg);
                }
                // Poll only the shared result; cancellation of this waiter leaves the LSP work alive.
                cx.background_executor()
                    .timer(Duration::from_millis(20))
                    .await;
            }
        })
    }
}

/// Fetch documentation and a bounded definition excerpt for one stable symbol position.
fn fetch_hover(
    server: Arc<LanguageServer>,
    document_uri: Uri,
    source: String,
    position: lsp_types::Position,
    symbol_range: lsp_types::Range,
) -> Result<Option<Hover>> {
    let result = server.hover(document_uri.clone(), source.clone(), position);
    if let Err(error) = &result {
        tracing::warn!(%error, "language hover request failed");
    }
    let hover = result?;
    // Definition previews complement the LSP signature and documentation.
    let preview = server
        .definitions(document_uri.clone(), source.clone(), position)
        .ok()
        .and_then(|locations| locations.into_iter().next())
        .and_then(|location| definition_preview(&document_uri, &source, location));
    let mut details = combine_details(hover, preview);
    if let Some(details) = &mut details
        && details.range.is_none()
    {
        // Keep the server's hover range when present and anchor an absent one to the word.
        details.range = Some(symbol_range);
    }
    Ok(details)
}

/// Resolve a UTF-8 byte boundary to a character inside its identifier.
fn symbol_at(text: &Rope, offset: usize) -> Option<(usize, usize)> {
    let len = text.len_chars();
    // GPUI and Rope::char use bytes, while the scan and cached range use character indices.
    let mut index = text.byte_to_char_idx(offset.min(text.len()));
    let char_at = |index| text.char(text.char_to_byte_idx(index));
    if index == len || !is_identifier(char_at(index)) {
        index = index.checked_sub(1)?;
        if !is_identifier(char_at(index)) {
            return None;
        }
    }
    let mut start = index;
    while start > 0 && is_identifier(char_at(start - 1)) {
        start -= 1;
    }
    let mut end = index + 1;
    while end < len && is_identifier(char_at(end)) {
        end += 1;
    }
    Some((start, end))
}

/// Keep the hover hit region aligned with Rust identifier characters.
fn is_identifier(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

#[cfg(test)]
mod symbol_tests {
    use super::*;

    /// Hover offsets are UTF-8 bytes even when earlier text contains multibyte characters.
    #[test]
    fn finds_symbol_after_multibyte_text() {
        let source = "中文 alpha";
        let text = Rope::from_str(source);
        let start = source.find("alpha").unwrap();
        assert_eq!(symbol_at(&text, 0), Some((0, 2)));
        assert_eq!(symbol_at(&text, start), Some((3, 8)));
        assert_eq!(symbol_at(&text, start + 2), Some((3, 8)));
        assert_eq!(symbol_at(&text, source.len()), Some((3, 8)));
    }
}

/// Read a bounded local excerpt from the definition target, including dependencies.
fn definition_preview(
    current_uri: &Uri,
    current_source: &str,
    location: lsp_types::LocationLink,
) -> Option<String> {
    let url = url::Url::parse(location.target_uri.as_str()).ok()?;
    let path = url.to_file_path().ok()?;
    let source = if &location.target_uri == current_uri {
        current_source.to_owned()
    } else {
        // Large files should not be read in full merely because the pointer moved.
        if fs::metadata(&path).ok()?.len() > 2 * 1024 * 1024 {
            return None;
        }
        fs::read_to_string(&path).ok()?
    };
    let line = location.target_selection_range.start.line as usize;
    let excerpt = source
        .lines()
        .skip(line)
        .take(24)
        .collect::<Vec<_>>()
        .join("\n");
    if excerpt.is_empty() {
        return None;
    }
    Some(format!(
        "**定义实现** · `{}:{}`\n\n````rust\n{}\n````",
        path.display(),
        line + 1,
        excerpt
    ))
}

/// Preserve the server's hover documentation and append a source excerpt when available.
fn combine_details(hover: Option<Hover>, preview: Option<String>) -> Option<Hover> {
    let preview = match preview {
        Some(preview) => preview,
        None => return hover,
    };
    let (mut content, range) = match hover {
        Some(Hover { contents, range }) => (hover_markdown(contents), range),
        None => (String::new(), None),
    };
    if !content.is_empty() {
        content.push_str("\n\n---\n\n");
    }
    content.push_str(&preview);
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: content,
        }),
        range,
    })
}

/// Convert legacy LSP hover payloads before composing the definition preview.
fn hover_markdown(contents: HoverContents) -> String {
    match contents {
        HoverContents::Markup(markup) => markup.value,
        HoverContents::Scalar(MarkedString::String(text)) => text,
        HoverContents::Scalar(MarkedString::LanguageString(item)) => {
            format!("```{}\n{}\n```", item.language, item.value)
        }
        HoverContents::Array(items) => items
            .into_iter()
            .map(|item| match item {
                MarkedString::String(text) => text,
                MarkedString::LanguageString(item) => {
                    format!("```{}\n{}\n```", item.language, item.value)
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
    }
}
