//! Merge bounded snapshot supplements without changing native server ordering or language semantics.
use super::*;
use lsp_types::{CompletionItem, CompletionTextEdit, Range, TextEdit};
use plugin_runtime::plugin_protocol::language::{CompletionDiagnostic, SourceSnapshot};

impl LanguageServer {
    /// A pure supplement remains tied to the caller's editor identity and revision.
    pub(crate) fn completions_at_version(
        &self,
        document: DocumentLease,
        source: String,
        position: Position,
        version: plugin_runtime::plugin_protocol::api::DocumentVersion,
    ) -> anyhow::Result<CompletionResponse> {
        let native = self.completions_for(document.clone(), source.clone(), position)?;
        let Some(cursor) = byte_at_position(&source, position) else {
            return Ok(native);
        };
        // The store accepts only a native version or a current immutable URI mapping and this exact text.
        // Unknown summaries remain None; the host never interprets a language's rule-loading error codes.
        let diagnostics = self
            .connection
            .lock()
            .ok()
            .and_then(|connection| {
                connection.as_ref().and_then(|connection| {
                    connection
                        .diagnostics
                        .snapshot(document.uri.as_str(), &source)
                })
            })
            .map(|items| {
                items
                    .into_iter()
                    .take(128)
                    .map(|item| CompletionDiagnostic {
                        code: item
                            .code
                            .and_then(|code| serde_json::to_value(code).ok())
                            .filter(|code| code.as_str().is_none_or(|text| text.len() <= 128)),
                        message: bounded_message(&item.message),
                    })
                    .collect()
            });
        let supplement = self.service.complete_snapshot(
            SourceSnapshot {
                document: version,
                text: source.clone(),
            },
            cursor,
            document.uri.as_str().into(),
            diagnostics,
        );
        ensure!(
            self.is_active() && document.is_active(),
            "Completion source has been retired"
        );
        let proposal = match supplement {
            Ok(Some(proposal)) => proposal,
            Ok(None) => return Ok(native),
            Err(error) => {
                // A failed optional supplement cannot erase valid server results; its actual cause remains visible.
                self.service.append_runtime_log(
                    &self.retired,
                    plugin_runtime::LogLevel::Error,
                    "language/completion",
                    format!("{error:#}"),
                );
                return Ok(native);
            }
        };
        let mut items = match native {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        };
        for candidate in proposal.items {
            let edit = CompletionTextEdit::Edit(TextEdit {
                range: Range {
                    start: position_at_byte(&source, candidate.replace.start),
                    end: position_at_byte(&source, candidate.replace.end),
                },
                new_text: candidate.new_text,
            });
            // Preserve the server's richer same-label entry and avoid repeated supplements with the same edit.
            if items
                .iter()
                .any(|item| item.label == candidate.label || item.text_edit.as_ref() == Some(&edit))
            {
                continue;
            }
            items.push(CompletionItem {
                label: candidate.label,
                text_edit: Some(edit),
                ..Default::default()
            });
        }
        Ok(CompletionResponse::Array(items))
    }
}

/// Diagnostic previews stop at a UTF-8 character boundary, without carrying arbitrary native response data.
fn bounded_message(message: &str) -> String {
    let mut end = message.len().min(1024);
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    message[..end].to_owned()
}

/// UTF-16 conversion rejects split surrogates, overflowing lines and out-of-range columns.
fn byte_at_position(source: &str, position: Position) -> Option<usize> {
    let mut line = 0;
    let mut utf16 = 0;
    for (offset, character) in source.char_indices() {
        if line == position.line && utf16 == position.character {
            return Some(offset);
        }
        if character == '\n' {
            if line == position.line {
                return None;
            }
            line += 1;
            utf16 = 0;
        } else if line == position.line {
            utf16 += character.len_utf16() as u32;
        }
    }
    (line == position.line && utf16 == position.character).then_some(source.len())
}
