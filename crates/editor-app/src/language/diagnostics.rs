//! Derives syntax errors from the same plugin WASM grammars used for highlighting.

use gpui_base::input::{Diagnostic, DiagnosticSeverity, Rope, RopeExt as _};
use gpui_kit::component::highlighter::{LanguageRegistry, SyntaxHighlighter};
use rust_i18n::t;
use std::ops::Range;

/// Bound recovery noise while a partially typed document contains many errors.
pub(crate) const MAX_DIAGNOSTICS: usize = 200;

/// Reuses a document's parser without holding a second editable document model.
#[derive(Default)]
pub(crate) struct SyntaxDiagnostics {
    highlighter: Option<(String, SyntaxHighlighter)>,
}

impl SyntaxDiagnostics {
    /// Parse a read-only snapshot on a worker, using only a registered plugin grammar.
    pub(crate) fn check(&mut self, language: &str, text: &Rope) -> Vec<Diagnostic> {
        // Plugin registration deliberately leaves the native grammar empty. This
        // guard prevents diagnostic parsing from falling back to a host grammar.
        if !LanguageRegistry::singleton()
            .language(language)
            .is_some_and(|config| config.language.is_none() && !config.highlights.is_empty())
        {
            self.highlighter = None;
            return Vec::new();
        }
        if self
            .highlighter
            .as_ref()
            .is_none_or(|(id, _)| id != language)
        {
            self.highlighter = Some((language.to_owned(), SyntaxHighlighter::new(language)));
        }
        let highlighter = &mut self.highlighter.as_mut().unwrap().1;
        highlighter.update(None, text, None);
        let Some(tree) = highlighter.tree() else {
            return Vec::new();
        };
        let mut diagnostics = Vec::new();
        let mut pending = vec![tree.root_node()];
        while let Some(node) = pending.pop() {
            if diagnostics.len() == MAX_DIAGNOSTICS {
                break;
            }
            if !node.has_error() && !node.is_missing() {
                continue;
            }
            let mut cursor = node.walk();
            let children: Vec<_> = node
                .children(&mut cursor)
                .filter(|child| child.has_error() || child.is_missing())
                .collect();
            // Prefer the smallest recovery node instead of underlining an entire
            // function when a nested error already identifies the broken token.
            if node.is_missing() || (node.is_error() && children.is_empty()) {
                let range = visible_error_range(text, node.byte_range());
                if range.is_empty() {
                    continue;
                }
                let message = if node.is_missing() {
                    t!("diagnostics.missing", token = node.kind()).to_string()
                } else {
                    t!("diagnostics.unexpected").to_string()
                };
                diagnostics.push(
                    Diagnostic::new(
                        text.offset_to_position(range.start)..text.offset_to_position(range.end),
                        message,
                    )
                    .with_severity(DiagnosticSeverity::Error)
                    .with_source(language.to_owned()),
                );
            } else {
                // Reverse stack insertion to preserve source order for Base's range index.
                pending.extend(children.into_iter().rev());
            }
        }
        diagnostics.sort_by_key(|diagnostic| {
            (
                diagnostic.range.start.line,
                diagnostic.range.start.character,
                diagnostic.range.end.line,
                diagnostic.range.end.character,
            )
        });
        diagnostics.dedup();
        diagnostics
    }
}

/// Missing tokens have an empty range; give them a visible Unicode character anchor.
fn visible_error_range(text: &Rope, range: Range<usize>) -> Range<usize> {
    if !range.is_empty() {
        return range;
    }
    let offset = range.start.min(text.len());
    if let Some(character) = text.slice(offset..).chars().next()
        && !character.is_whitespace()
    {
        return offset..offset + character.len_utf8();
    }
    // At whitespace or EOF, mark the preceding visible glyph so missing-token
    // explanations remain hoverable even with trailing spaces and blank lines.
    let mut end = offset;
    // Ropey's iterator exposes backwards traversal directly, without allocating a string.
    let characters = text.chars_at(offset).reversed();
    for character in characters {
        let start = end - character.len_utf8();
        if !character.is_whitespace() {
            return start..end;
        }
        end = start;
    }
    offset..offset
}

/// Translate LSP UTF-16 columns into Base's Unicode scalar positions without splitting emoji.
pub(crate) fn from_lsp(text: &Rope, diagnostics: Vec<lsp_types::Diagnostic>) -> Vec<Diagnostic> {
    diagnostics
        .into_iter()
        .filter_map(|mut diagnostic| {
            if diagnostic.message.trim().is_empty() {
                return None;
            }
            let start = lsp_offset(text, diagnostic.range.start)?;
            let end = lsp_offset(text, diagnostic.range.end)?;
            if end < start {
                return None;
            }
            let range = visible_error_range(text, start..end);
            if range.is_empty() {
                return None;
            }
            // An omitted severity is conventionally an error, not an invisible hint.
            diagnostic
                .severity
                .get_or_insert(lsp_types::DiagnosticSeverity::ERROR);
            let mut result = Diagnostic::from(diagnostic);
            result.range = text.offset_to_position(range.start)..text.offset_to_position(range.end);
            Some(result)
        })
        .collect()
}

/// Clamp malformed columns to a character boundary while rejecting nonexistent lines.
fn lsp_offset(text: &Rope, position: lsp_types::Position) -> Option<usize> {
    let row = position.line as usize;
    if row >= text.lines_len() {
        return None;
    }
    let mut units = 0;
    let mut bytes = 0;
    for character in text.slice_line(row).chars() {
        if character == '\r'
            || character == '\n'
            || units + character.len_utf16() > position.character as usize
        {
            break;
        }
        units += character.len_utf16();
        bytes += character.len_utf8();
    }
    Some(text.line_start_offset(row) + bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LSP positions count UTF-16 units; the editor counts Unicode scalar values.
    #[test]
    fn semantic_diagnostics_preserve_unicode_ranges_and_metadata() {
        let text = Rope::from("😀 中文 missing()\r\n");
        let diagnostic: lsp_types::Diagnostic = serde_json::from_value(serde_json::json!({
            "range": { "start": { "line": 0, "character": 6 }, "end": { "line": 0, "character": 13 } },
            "severity": 1, "code": "E0425", "source": "rustc", "message": "cannot find function missing"
        })).unwrap();
        let result = from_lsp(&text, vec![diagnostic]);
        assert_eq!(result.len(), 1);
        let error = &result[0];
        assert_eq!(error.range.start.character, 5);
        assert_eq!(error.code.as_deref(), Some("E0425"));
        assert_eq!(error.source.as_deref(), Some("rustc"));
        assert_eq!(error.severity, DiagnosticSeverity::Error);
        assert_eq!(
            text.slice(
                text.position_to_offset(&error.range.start)
                    ..text.position_to_offset(&error.range.end)
            )
            .to_string(),
            "missing"
        );
    }

    /// Malformed server positions never panic or create markers in unrelated lines.
    #[test]
    fn invalid_semantic_ranges_are_safe() {
        let text = Rope::from("😀\r\n");
        let diagnostic = |line, start, end| {
            lsp_types::Diagnostic::new_simple(
                lsp_types::Range::new(
                    lsp_types::Position::new(line, start),
                    lsp_types::Position::new(line, end),
                ),
                "error".into(),
            )
        };
        assert!(from_lsp(&text, vec![diagnostic(99, 0, 1), diagnostic(0, 2, 0)]).is_empty());
        let result = from_lsp(&text, vec![diagnostic(0, 1, 1)]);
        assert_eq!(result[0].range.start.character, 0);
        assert_eq!(result[0].range.end.character, 1);
    }

    /// Register source WASM assets so tests exercise plugin parsing, not native grammars.
    fn check(language: &str, source: &str) -> (Rope, Vec<Diagnostic>) {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../plugins")
            .join(language);
        crate::language::plugins::register_plugin(&root).unwrap();
        let text = Rope::from(source);
        let diagnostics = SyntaxDiagnostics::default().check(language, &text);
        (text, diagnostics)
    }

    /// Incomplete Rust must mark the missing semicolon, and a correction removes it.
    #[test]
    fn rust_missing_token_and_correction() {
        let (text, errors) = check("rust", "fn main() { let value = 1 }");
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|error| error.message.contains(';')));
        for error in errors {
            let start = text.position_to_offset(&error.range.start);
            let end = text.position_to_offset(&error.range.end);
            assert!(end > start);
        }
        let mut parser = SyntaxDiagnostics::default();
        assert!(!parser.check("rust", &text).is_empty());
        assert!(
            parser
                .check("rust", &Rope::from("fn main() { let value = 1; }"))
                .is_empty()
        );
    }

    /// TOML errors and multibyte text keep valid, nonempty byte ranges.
    #[test]
    fn toml_unicode_error_range() {
        let (text, errors) = check("toml", "name = \"中文😀\"\nvalue = @\n");
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|error| error.range.start.line == 1));
        for error in errors {
            let range = text.position_to_offset(&error.range.start)
                ..text.position_to_offset(&error.range.end);
            assert!(!range.is_empty());
            assert!(!text.slice(range).to_string().is_empty());
        }
        assert!(check("toml", "name = \"中文😀\"\nvalue = 1\n").1.is_empty());
    }

    /// Zero-width ranges at CRLF and EOF never split an emoji or underline a newline.
    #[test]
    fn missing_token_has_visible_unicode_anchor() {
        let text = Rope::from("中文😀\r\n");
        assert_eq!(visible_error_range(&text, 10..10), 6..10);
        assert_eq!(visible_error_range(&text, 12..12), 6..10);
        let padded = Rope::from("中文😀  \r\n");
        assert_eq!(visible_error_range(&padded, 14..14), 6..10);
        assert_eq!(visible_error_range(&Rope::new(), 0..0), 0..0);
    }
}
