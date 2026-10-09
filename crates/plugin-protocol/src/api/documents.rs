//! Stable editor.documents 1.1 identities and immutable, bounded document descriptions.
use super::{DocumentVersion, ErrorCode, Failure, ResourceHandle, TextRange};
use serde::{Deserialize, Serialize};

/// A resource identity names content; it never grants access to its path or another instance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResourceIdentity {
    /// A normalized workspace-relative path. Canonical containment is checked on every open.
    Local { path: String },
    /// Readonly content owned by one live plugin instance, with no backing disk file.
    Virtual { handle: ResourceHandle },
}

impl ResourceIdentity {
    /// Local identities are plain normalized paths, not URI-encoded links. Shape validation
    /// never grants access; the host must still canonicalize and check physical containment.
    pub fn validate(&self) -> Result<(), Failure> {
        if let Self::Local { path } = self {
            if path.is_empty()
                || path.len() > 4096
                || path.contains(['\\', ':', '<', '>', '"', '|', '?', '*'])
                || path.chars().any(char::is_control)
                || path.split('/').any(|part| {
                    part.is_empty()
                        || matches!(part, "." | "..")
                        || part.ends_with(['.', ' '])
                        || super::is_windows_device_segment(part)
                })
            {
                return Err(Failure::new(
                    ErrorCode::InvalidPath,
                    "Resource path must be normalized and workspace-relative",
                ));
            }
        }
        Ok(())
    }
}

/// Ordered metadata notifications carry one source sequence and never contain document text.
/// SubscribeDocumentEvents opts in; overflow terminates the handle and requires a fresh enumeration.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentEvent {
    /// Monotonically increasing sequence within this host workspace session; gaps are permitted.
    pub sequence: u64,
    /// Virtual identities are delivered only to the instance owning their live handle.
    pub resource: Option<ResourceIdentity>,
    /// An event is an observation, including WillSave; it cannot block or intercept a save.
    pub kind: DocumentEventKind,
}

/// Explicit metadata transitions preserve save/close order even between UI redraws.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DocumentEventKind {
    Opened(DocumentInfo),
    ContentChanged(DocumentInfo),
    Closed(DocumentVersion),
    /// This observation does not promise delivery before the disk write completes.
    WillSave(DocumentVersion),
    DidSave {
        document: DocumentVersion,
        result: Result<(), Failure>,
    },
    ActiveChanged(Option<DocumentVersion>),
    SelectionChanged {
        document: DocumentVersion,
        selection: TextRange,
    },
    ViewportChanged {
        document: DocumentVersion,
        visible_rows: Option<VisibleRows>,
    },
}

/// Explicit document abilities prevent callers from assuming that every text resource is writable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentAccess {
    /// The current principal can observe this text session.
    pub read: bool,
    /// Native typing and edit proposals are permitted for this presentation.
    pub edit: bool,
    /// The session can commit to its local file store; virtual sessions always report false.
    pub save: bool,
}

/// UTF-8 text is the current host baseline. A BOM remains part of byte coordinates when present.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentEncoding {
    Utf8,
    Utf8Bom,
}

/// Line-ending metadata is derived from the current text, including unsaved mixed line endings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentEol {
    None,
    Lf,
    CrLf,
    Cr,
    Mixed,
}

/// Half-open laid-out row indices. Wrapping/folding means rows are not source line numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisibleRows {
    pub start: usize,
    pub end: usize,
}

/// Metadata is read at the same revision as the native text; it is not a mutable document model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentInfo {
    /// Exact identity/path/revision required by follow-up requests.
    pub document: DocumentVersion,
    /// Content identity only, with no implicit access grant.
    pub resource: ResourceIdentity,
    /// Native tab display title, independent from a virtual identity URI.
    pub title: String,
    /// Observed read, edit and save abilities at this native presentation.
    pub access: DocumentAccess,
    /// DocumentSession's unsaved local-edit flag; readonly refreshes remain clean.
    pub dirty: bool,
    /// Local language identifier or the provider's readonly display hint.
    pub language: String,
    /// UTF-8 representation, retaining a BOM when present in the native value.
    pub encoding: DocumentEncoding,
    /// Current line endings, including unsaved mixed sequences.
    pub eol: DocumentEol,
    /// Byte length of the full current UTF-8 value, before applying a read range.
    pub byte_len: usize,
    /// Half-open native UTF-8 selection, independent from UTF-16 request coordinates.
    pub selection: TextRange,
    /// None before first layout or when no viewport is mounted for this document.
    pub visible_rows: Option<VisibleRows>,
}

impl DocumentEol {
    /// Describe actual line endings without normalizing bytes or guessing from the file extension.
    pub fn of(text: &str) -> Self {
        Self::of_chars(text.chars())
    }

    /// Describe a borrowed text iterator without materializing a second full document string.
    pub fn of_chars(characters: impl Iterator<Item = char>) -> Self {
        let mut characters = characters.peekable();
        let (mut lf, mut crlf, mut cr) = (false, false, false);
        while let Some(character) = characters.next() {
            match character {
                '\r' if characters.peek() == Some(&'\n') => {
                    crlf = true;
                    characters.next();
                }
                '\r' => cr = true,
                '\n' => lf = true,
                _ => {}
            }
        }
        match (lf, crlf, cr) {
            (false, false, false) => Self::None,
            (true, false, false) => Self::Lf,
            (false, true, false) => Self::CrLf,
            (false, false, true) => Self::Cr,
            _ => Self::Mixed,
        }
    }
}

/// A zero-based source line and UTF-16 column. Splitting a surrogate pair is invalid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextPosition {
    pub line: u32,
    pub character: u32,
}

/// Explicit coordinates remove ambiguity between LSP UTF-16 and native UTF-8 byte offsets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "unit", rename_all = "snake_case", deny_unknown_fields)]
pub enum DocumentRange {
    Bytes {
        start: usize,
        end: usize,
    },
    Utf16 {
        start: TextPosition,
        end: TextPosition,
    },
}

impl DocumentRange {
    /// Validate against the requested immutable revision; endpoints are never silently clamped.
    pub fn resolve(self, text: &str) -> Result<TextRange, Failure> {
        let (start, end) = match self {
            Self::Bytes { start, end } => (start, end),
            Self::Utf16 { start, end } => (start.offset(text)?, end.offset(text)?),
        };
        if start > end
            || end > text.len()
            || !text.is_char_boundary(start)
            || !text.is_char_boundary(end)
        {
            return Err(Failure::new(
                ErrorCode::InvalidRequest,
                "Range is outside text or splits a UTF-8 character",
            ));
        }
        Ok(TextRange { start, end })
    }
}

impl TextPosition {
    /// Convert strict UTF-16 coordinates; CRLF is one line break and its CR is not a column.
    pub fn offset(self, text: &str) -> Result<usize, Failure> {
        self.offset_chars(text.chars())
    }

    /// Resolve the same strict coordinates over a borrowed native Rope iterator without a full copy.
    /// Returns a UTF-8 byte offset, or InvalidRequest for a missing line/column or split surrogate.
    pub fn offset_chars(self, characters: impl Iterator<Item = char>) -> Result<usize, Failure> {
        let mut characters = characters.peekable();
        let (mut line, mut offset, mut column) = (0u32, 0usize, 0u32);
        while line < self.line {
            let Some(character) = characters.next() else {
                break;
            };
            offset += character.len_utf8();
            if character == '\r' || character == '\n' {
                if character == '\r' && characters.peek() == Some(&'\n') {
                    characters.next();
                    offset += 1;
                }
                line += 1;
            }
        }
        if line == self.line {
            if self.character == 0 {
                return Ok(offset);
            }
            for character in characters {
                if character == '\r' || character == '\n' {
                    break;
                }
                let Some(next) = column.checked_add(character.len_utf16() as u32) else {
                    break;
                };
                column = next;
                offset += character.len_utf8();
                if column == self.character {
                    return Ok(offset);
                }
                if column > self.character {
                    break;
                }
            }
        }
        Err(Failure::new(
            ErrorCode::InvalidRequest,
            "UTF-16 position is outside text or splits a surrogate pair",
        ))
    }
}

/// Range reads return the same complete metadata plus the actual half-open UTF-8 range.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentSnapshot {
    pub info: DocumentInfo,
    pub range: TextRange,
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plain filenames retain literal URL punctuation while unsafe platform paths are rejected.
    #[test]
    fn local_identity_is_normalized_plain_text_and_never_a_url_authority() {
        for path in ["nested/a#b%20.txt", "中文.txt"] {
            ResourceIdentity::Local { path: path.into() }
                .validate()
                .unwrap();
        }
        for path in [
            "", "../a", "a//b", "C:/x", "a\\b", "a:", "CON.txt", "a. ", "/a",
        ] {
            assert_eq!(
                ResourceIdentity::Local { path: path.into() }
                    .validate()
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidPath
            );
        }
    }

    /// BOM and non-BMP coordinates remain exact across every supported line-break form.
    #[test]
    fn strict_positions_retain_bom_and_handle_empty_final_cr_and_crlf_lines() {
        let text = "\u{feff}中😀\r\nx\ry\n";
        assert_eq!(
            TextPosition {
                line: 0,
                character: 2
            }
            .offset(text)
            .unwrap(),
            6
        );
        assert_eq!(
            TextPosition {
                line: 0,
                character: 4
            }
            .offset(text)
            .unwrap(),
            10
        );
        assert_eq!(
            TextPosition {
                line: 1,
                character: 0
            }
            .offset(text)
            .unwrap(),
            12
        );
        assert_eq!(
            TextPosition {
                line: 2,
                character: 1
            }
            .offset(text)
            .unwrap(),
            15
        );
        assert_eq!(
            TextPosition {
                line: 3,
                character: 0
            }
            .offset(text)
            .unwrap(),
            text.len()
        );
        for position in [
            TextPosition {
                line: 0,
                character: 3,
            },
            TextPosition {
                line: 3,
                character: 1,
            },
            TextPosition {
                line: 4,
                character: 0,
            },
        ] {
            assert_eq!(
                position.offset(text).unwrap_err().code,
                ErrorCode::InvalidRequest
            );
        }
        assert_eq!(DocumentEol::of(text), DocumentEol::Mixed);
        assert_eq!(
            DocumentRange::Bytes { start: 3, end: 10 }
                .resolve(text)
                .unwrap(),
            TextRange { start: 3, end: 10 }
        );
        assert!(
            DocumentRange::Bytes { start: 10, end: 3 }
                .resolve(text)
                .is_err()
        );
    }
}
