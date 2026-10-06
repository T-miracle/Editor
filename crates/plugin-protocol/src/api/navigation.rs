//! Versioned navigation targets and shared URI checks do not choose languages or launch external programs.
use super::{ErrorCode, Failure};
use serde::{Deserialize, Serialize};

/// Native effects remain separate from Markdown slug/path selection performed by a guest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NavigationTarget {
    /// Reveal an owned source-mapped block in the currently published native scene.
    PreviewNode {
        panel: String,
        node: String,
        ui_revision: u64,
    },
    /// Open an existing document relative to the source directory, with one percent decoding pass.
    RelativeDocument { path: String },
    /// Only HTTP(S) URLs may reach the system browser; this grants no Shell command execution.
    ExternalUrl { url: String },
}

impl NavigationTarget {
    /// Check bounded portable shape before issuing a request. The native effect rechecks physical paths,
    /// parsed URL authority, source version, scene revision, permissions and resource ownership.
    pub fn validate(&self) -> Result<(), Failure> {
        match self {
            Self::PreviewNode { panel, node, .. } => {
                if panel.is_empty()
                    || panel.len() > 100
                    || node.is_empty()
                    || node.len() > 128
                    || panel.chars().chain(node.chars()).any(char::is_control)
                {
                    return Err(invalid("Invalid navigation panel or node identity"));
                }
            }
            Self::RelativeDocument { path } => {
                document_relative_path(path)?;
            }
            Self::ExternalUrl { url } => {
                let valid_scheme = url.split_once("://").is_some_and(|(scheme, tail)| {
                    (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
                        && !tail.is_empty()
                });
                if url.is_empty()
                    || url.len() > 4096
                    || url.chars().any(char::is_control)
                    || !valid_scheme
                {
                    return Err(invalid("Only bounded HTTP(S) browser URLs are supported"));
                }
            }
        }
        Ok(())
    }
}

/// Decode percent escapes exactly once, preserving literal plus signs. Invalid UTF-8/escapes are failures.
/// The 4096-byte limit applies before decoding to bound URI work in native and independent WASM callers.
pub fn decode_uri_component(value: &str) -> Result<String, Failure> {
    if value.len() > 4096 {
        return Err(invalid("URI exceeds 4096 bytes"));
    }
    let mut decoded = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let pair = bytes
                .get(index + 1..index + 3)
                .ok_or_else(|| invalid("Invalid URI escape"))?;
            let digit = |byte: u8| (byte as char).to_digit(16).map(|value| value as u8);
            let high = digit(pair[0]).ok_or_else(|| invalid("Invalid URI escape"))?;
            let low = digit(pair[1]).ok_or_else(|| invalid("Invalid URI escape"))?;
            decoded.push(high * 16 + low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).map_err(|_| invalid("URI is not UTF-8"))
}

/// Validate a raw relative URI path, allowing `..` only for the later canonical workspace-bound check.
/// Encoded literal `#` is a filename character; unencoded query/fragment belongs to guest link selection.
pub fn document_relative_path(value: &str) -> Result<String, Failure> {
    let decoded = decode_uri_component(value)?;
    if decoded.is_empty()
        || decoded.starts_with('/')
        || value.contains(['?', '#'])
        || decoded.contains(['\\', ':', '<', '>', '"', '|', '?', '*'])
        || decoded.chars().any(char::is_control)
        || decoded.split('/').any(is_windows_device_segment)
    {
        return Err(invalid(
            "Document path must be a relative workspace URI path",
        ));
    }
    Ok(decoded)
}

/// Reject DOS device names consistently on every platform, including aliases with suffixes or spaces.
pub fn is_windows_device_segment(segment: &str) -> bool {
    let name = segment
        .trim_end_matches([' ', '.'])
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    matches!(
        name.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || name
        .strip_prefix("COM")
        .or_else(|| name.strip_prefix("LPT"))
        .is_some_and(|number| {
            matches!(
                number,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
}

fn invalid(message: &str) -> Failure {
    Failure::new(ErrorCode::InvalidPath, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A portable URL path cannot become a second-decoded traversal, device, ADS or invalid UTF-8 path.
    #[test]
    fn navigation_paths_decode_once_and_reject_unsafe_shapes() {
        assert_eq!(
            document_relative_path("../%E4%B8%AD%E6%96%87.md").unwrap(),
            "../中文.md"
        );
        assert_eq!(
            document_relative_path("%252e%252e/a.md").unwrap(),
            "%2e%2e/a.md"
        );
        assert_eq!(document_relative_path("a%23b.md").unwrap(), "a#b.md");
        for invalid in [
            "/outside.md",
            "%2foutside.md",
            "..%5coutside.md",
            "C%3a/a.md",
            "a.md%3Adata",
            "NUL.md",
            "COM%C2%B9.md",
            "%ff",
            "%2",
            "a.md#x",
            "a.md?x",
        ] {
            assert!(document_relative_path(invalid).is_err(), "{invalid}");
        }
    }
    /// External effects admit HTTP(S) only; no unknown protocol is silently converted into a command.
    #[test]
    fn navigation_target_shapes_are_bounded() {
        for url in ["HTTP://example.com", "https://example.com/#title"] {
            assert!(
                NavigationTarget::ExternalUrl { url: url.into() }
                    .validate()
                    .is_ok()
            );
        }
        for url in [
            "file:///x",
            "javascript:alert(1)",
            "mailto:a@b",
            "https://",
            "https://x\n",
        ] {
            assert!(
                NavigationTarget::ExternalUrl { url: url.into() }
                    .validate()
                    .is_err()
            );
        }
        assert!(
            NavigationTarget::PreviewNode {
                panel: "preview".into(),
                node: "".into(),
                ui_revision: 1
            }
            .validate()
            .is_err()
        );
    }
}
