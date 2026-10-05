//! Task edits are derived from parsed native checkboxes and retain the editor's actual UTF-8 selection.

use crate::format::Edit;
use plugin_protocol::ui;
use std::ops::Range;

/// Only the marker's one-byte middle changes; the full parsed marker is checked again before writing.
pub(super) struct Change {
    marker: Range<usize>,
    before: u8,
    after: u8,
}

/// Resolve a real enabled checkbox in the current parsed tree; node strings never supply arbitrary offsets.
/// A repeated request for the already-rendered state is a no-op and creates no undo entry.
pub(super) fn change(nodes: &[ui::Node], source: &str, id: &str, checked: bool) -> Option<Change> {
    let node = find(nodes, id)?;
    let ui::Kind::Checkbox {
        checked: previous, ..
    } = &node.kind
    else {
        return None;
    };
    if node.disabled || *previous == checked {
        return None;
    }
    let range = node.source_range.as_ref()?;
    let marker = range.start..range.end;
    // The parser accepts these ASCII blanks without accepting line breaks or wider Unicode spaces.
    // Capture the exact original byte so a different blank in a later snapshot still rejects this intent.
    let (before, source_checked) = match source.get(marker.clone())?.as_bytes() {
        [b'[', blank, b']'] if matches!(*blank, b' ' | b'\t' | b'\x0b' | b'\x0c') => {
            (*blank, false)
        }
        b"[x]" => (b'x', true),
        b"[X]" => (b'X', true),
        _ => return None,
    };
    if source_checked != *previous {
        return None;
    }
    Some(Change {
        marker,
        before,
        after: if checked { b'x' } else { b' ' },
    })
}

impl Change {
    /// Verify the unchanged marker and selection against the readonly snapshot after the host's selection read.
    /// Equal one-byte replacement length keeps both selection endpoints valid in the resulting full source.
    pub(super) fn plan(self, source: &str, selection: Range<usize>) -> Option<Edit> {
        source.get(selection.clone())?;
        if source.get(self.marker.clone())?.as_bytes() != [b'[', self.before, b']'].as_slice() {
            return None;
        }
        let start = self.marker.start + 1;
        Some(Edit {
            range: start..start + 1,
            text: char::from(self.after).to_string(),
            selection,
        })
    }
}

/// Containers are traversed through the public node model; unrelated text, code and image nodes stay readonly.
fn find<'a>(nodes: &'a [ui::Node], id: &str) -> Option<&'a ui::Node> {
    for node in nodes {
        if node.id == id {
            return Some(node);
        }
        let found = match &node.kind {
            ui::Kind::Column { children } | ui::Kind::Row { children } => find(children, id),
            ui::Kind::Scroll { content } => find(std::slice::from_ref(content.as_ref()), id),
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    //! Parser-to-edit regressions cover control-byte markers and snapshot boundaries without host state.

    use super::{change, find};
    use crate::preview::blocks;
    use plugin_protocol::ui;

    /// Every blank accepted by the actual parser becomes one ASCII-byte edit with the same UTF-8 selection.
    #[test]
    fn parsed_ascii_blank_tasks_keep_original_selection_and_neighboring_text() {
        for blank in [' ', '\t', '\u{000b}', '\u{000c}'] {
            let source = format!("- [{blank}] 中文🙂\n- [ ] 相邻\n");
            let nodes = blocks(&source, "zh-CN").unwrap();
            let selection = 3..source.len();
            let edit = change(&nodes, &source, "b-2-task", true)
                .expect("the enabled parser checkbox must accept its blank marker")
                .plan(&source, selection.clone())
                .unwrap();
            assert_eq!(edit.range, 3..4);
            assert_eq!(edit.selection, selection);
            let mut replaced = source;
            replaced.replace_range(edit.range, &edit.text);
            assert_eq!(replaced, "- [x] 中文🙂\n- [ ] 相邻\n");
            let updated = blocks(&replaced, "zh-CN").unwrap();
            assert!(matches!(
                &find(&updated, "b-2-task").unwrap().kind,
                ui::Kind::Checkbox { checked: true, .. }
            ));
        }
    }

    /// A different unchecked byte is still a changed snapshot; selection endpoints cannot split Chinese text.
    #[test]
    fn blank_task_plan_rejects_changed_marker_and_invalid_utf8_selection() {
        for blank in [' ', '\t', '\u{000b}', '\u{000c}'] {
            let source = format!("- [{blank}] 中文🙂\n");
            let nodes = blocks(&source, "en-US").unwrap();
            let replacement = if blank == ' ' { '\t' } else { ' ' };
            let changed = format!("- [{replacement}] 中文🙂\n");
            assert!(
                change(&nodes, &source, "b-2-task", true)
                    .unwrap()
                    .plan(&changed, 0..0)
                    .is_none(),
                "even a semantically identical blank cannot replace the captured original byte"
            );
            assert!(
                change(&nodes, &source, "b-2-task", true)
                    .unwrap()
                    .plan(&source, 7..7)
                    .is_none(),
                "the preserved selection must remain a valid UTF-8 boundary"
            );
        }
    }

    /// Whitespace inside ordinary text or non-task syntax never grants a checkbox edit by a guessed node ID.
    #[test]
    fn task_syntax_with_line_breaks_or_unicode_spaces_stays_readonly() {
        for marker in ["[\n]", "[\r]", "[\r\n]", "[\u{00a0}]", "[\u{3000}]", "[  ]"] {
            let source = format!("- {marker} 普通文字\n");
            let nodes = blocks(&source, "zh-CN").unwrap();
            assert!(change(&nodes, &source, "b-2-task", true).is_none());
        }
    }
}
