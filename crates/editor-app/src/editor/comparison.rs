//! Bounded document comparison over the existing native editor sessions.

use crate::*;
use gpui_base::TestSupportExt as _;
use gpui_base::input::{RangeDecoration, RangeDecorationCollection, RangeDecorationStyle};
use gpui_kit::Hsla;
use plugin_runtime::plugin_protocol::api;

/// Own only comparison decorations and the temporary left-pane input restriction.
pub(crate) struct DocumentComparison {
    /// Two local sources do not carry a virtual handle, so retain the caller's runtime authority.
    authority: plugin_runtime::EditorAuthority,
    left: api::DocumentVersion,
    right: api::DocumentVersion,
    left_editor: Entity<EditorState>,
    right_editor: Entity<EditorState>,
    left_readonly: bool,
    left_title: String,
    right_title: String,
    left_marks: RangeDecorationCollection,
    right_marks: RangeDecorationCollection,
    // Immutable byte ranges are sufficient to recolor live comparisons; text stays in EditorState.
    hunks: Vec<diff::Hunk>,
    colors: [Hsla; 3],
}

/// Resolve deletion, insertion and modification colors from the current local theme.
fn comparison_colors(cx: &App) -> [Hsla; 3] {
    [
        cx.theme().danger.opacity(0.20),
        cx.theme().success.opacity(0.20),
        cx.theme().warning.opacity(0.20),
    ]
}

/// Build only this comparison's owner ranges; unrelated diagnostics and marks remain untouched.
fn comparison_marks(hunks: &[diff::Hunk], colors: [Hsla; 3], left: bool) -> Vec<RangeDecoration> {
    hunks
        .iter()
        .filter_map(|hunk| {
            let (range, opposite, one_sided) = if left {
                (&hunk.left, &hunk.right, colors[0])
            } else {
                (&hunk.right, &hunk.left, colors[1])
            };
            (!range.is_empty()).then(|| {
                RangeDecoration::new(range.clone())
                    .with_style(RangeDecorationStyle::Fill)
                    .with_color(if opposite.is_empty() {
                        one_sided
                    } else {
                        colors[2]
                    })
            })
        })
        .collect()
}

impl EditorApp {
    /// Compare two exact live versions; bounded snapshots are temporary algorithm inputs only.
    pub(crate) fn compare_plugin_documents(
        &mut self,
        left: &api::DocumentVersion,
        right: &api::DocumentVersion,
        request: &plugin_runtime::EditorRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<api::EditorValue, api::Failure> {
        let locate = |target: &api::DocumentVersion| {
            (0..self.tabs.len()).find(|index| {
                self.plugin_document_version(*index)
                    .is_ok_and(|version| version == *target)
            })
        };
        let li = locate(left).ok_or_else(|| {
            api::Failure::new(
                api::ErrorCode::StaleRevision,
                "Comparison left document changed or closed",
            )
        })?;
        let ri = locate(right).ok_or_else(|| {
            api::Failure::new(
                api::ErrorCode::StaleRevision,
                "Comparison right document changed or closed",
            )
        })?;
        if li == ri {
            return Err(api::Failure::new(
                api::ErrorCode::InvalidRequest,
                "Comparison requires distinct sessions",
            ));
        }
        let live = |index: usize| {
            self.tabs[index]
                .virtual_document
                .as_ref()
                .is_none_or(|tab| tab.resource.is_live())
        };
        if !live(li) || !live(ri) {
            return Err(api::Failure::new(
                api::ErrorCode::StaleRevision,
                "Comparison resource was revoked",
            ));
        }
        let left_editor = self.tabs[li]
            .text
            .as_ref()
            .ok_or_else(|| {
                api::Failure::new(
                    api::ErrorCode::UnsupportedOperation,
                    "Comparison requires text",
                )
            })?
            .editor
            .clone();
        let right_editor = self.tabs[ri]
            .text
            .as_ref()
            .ok_or_else(|| {
                api::Failure::new(
                    api::ErrorCode::UnsupportedOperation,
                    "Comparison requires text",
                )
            })?
            .editor
            .clone();
        // Check Rope byte lengths before allocating immutable snapshots on the UI thread.
        if left_editor.read(cx).text().len() > 1024 * 1024
            || right_editor.read(cx).text().len() > 1024 * 1024
        {
            return Err(api::Failure::new(
                api::ErrorCode::LimitExceeded,
                "Comparison exceeds 1 MiB per document",
            ));
        }
        let hunks = diff::compare(
            &left_editor.read(cx).value().to_string(),
            &right_editor.read(cx).value().to_string(),
        )
        .map_err(|_| {
            api::Failure::new(
                api::ErrorCode::LimitExceeded,
                "Comparison exceeds 2000 lines or 4M cells",
            )
        })?;
        // Expensive pure comparison is cancellable until native focus/decorations actually change.
        if !request.authority().is_live() || !request.enter_side_effect() {
            return Err(api::Failure::new(
                api::ErrorCode::Cancelled,
                "Comparison did not execute",
            ));
        }
        self.close_plugin_document_menu(window, cx);
        self.close_document_comparison(cx);
        self.activate_tab(ri, window, cx);
        // Activating a tab can settle linked input. Never attach a pre-activation diff
        // if that settlement advanced either source's capability revision.
        if self.plugin_document_version(li)? != *left || self.plugin_document_version(ri)? != *right
        {
            return Err(api::Failure::new(
                api::ErrorCode::StaleRevision,
                "Comparison source changed during activation",
            ));
        }
        // Preserve the readonly flag itself: a separately disabled editor must not become readonly.
        let left_readonly = left_editor.read(cx).presentation().is_readonly();
        let colors = comparison_colors(cx);
        let left_marks = left_editor.update(cx, |editor, cx| {
            editor.set_readonly(true, cx);
            editor.create_range_decorations_collection(comparison_marks(&hunks, colors, true), cx)
        });
        let right_marks = right_editor.update(cx, |editor, cx| {
            editor.create_range_decorations_collection(comparison_marks(&hunks, colors, false), cx)
        });
        self.document_comparison = Some(DocumentComparison {
            authority: request.authority().clone(),
            left: left.clone(),
            right: right.clone(),
            left_editor,
            right_editor,
            left_readonly,
            left_title: self.tabs[li].title(),
            right_title: self.tabs[ri].title(),
            left_marks,
            right_marks,
            hunks,
            colors,
        });
        self.editor_panel.update(cx, |_, cx| cx.notify());
        cx.notify();
        Ok(api::EditorValue::DocumentsCompared {
            left: left.clone(),
            right: right.clone(),
        })
    }

    /// Invalidate changed versions/authority, returning keys only from a pane being unmounted.
    pub(crate) fn sync_document_comparison(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.document_comparison_is_current() {
            // Dismiss the popup while its previous left focus is still mounted, then reuse
            // the same selective repair as direct keyboard focus on the retired pane.
            self.close_plugin_document_menu(window, cx);
            // GPUI retains the old FocusHandle after unmounting; focus-lost alone does not
            // give keys to the surviving editor. Leave dialogs and an already-visible
            // editor alone, and never lend a background text entity to a binary tab.
            let restore_focus = self.document_comparison.as_ref().is_some_and(|comparison| {
                [&comparison.left_editor, &comparison.right_editor]
                    .into_iter()
                    .any(|editor| {
                        editor != &self.editor
                            && editor
                                .read(cx)
                                .focus_handle(cx)
                                .contains_focused(window, cx)
                    })
            });
            self.close_document_comparison(cx);
            if restore_focus && self.active_text_tab_index().is_some() {
                self.editor
                    .update(cx, |editor, cx| editor.focus(window, cx));
            }
            return;
        }
        let colors = comparison_colors(cx);
        if let Some(comparison) = &mut self.document_comparison
            && comparison.colors != colors
        {
            // Theme changes do not invalidate text versions; replace only owned colors once.
            comparison
                .left_marks
                .set(comparison_marks(&comparison.hunks, colors, true), cx);
            comparison
                .right_marks
                .set(comparison_marks(&comparison.hunks, colors, false), cx);
            comparison.colors = colors;
        }
    }

    /// Rendering rechecks because queued requests may mutate documents after shell synchronization.
    fn document_comparison_is_current(&self) -> bool {
        let Some(comparison) = &self.document_comparison else {
            return false;
        };
        let live = |target: &api::DocumentVersion| {
            (0..self.tabs.len()).any(|index| {
                self.plugin_document_version(index)
                    .is_ok_and(|version| version == *target)
                    && self.tabs[index]
                        .virtual_document
                        .as_ref()
                        .is_none_or(|tab| tab.resource.is_live())
            })
        };
        comparison.authority.is_live()
            && live(&comparison.left)
            && live(&comparison.right)
            && self.active_tab_index().is_some_and(|index| {
                self.plugin_document_version(index)
                    .is_ok_and(|version| version == comparison.right)
            })
    }

    /// Dispose only our owners; diagnostics and other editor decorations survive closure.
    pub(crate) fn close_document_comparison(&mut self, cx: &mut Context<Self>) {
        if let Some(comparison) = self.document_comparison.take() {
            comparison.left_marks.dispose(cx);
            comparison.right_marks.dispose(cx);
            comparison.left_editor.update(cx, |editor, cx| {
                editor.set_readonly(comparison.left_readonly, cx)
            });
            self.editor_panel.update(cx, |_, cx| cx.notify());
            cx.notify();
        }
    }

    /// Global Save must respect the actual focused pane, not the active right tab alone.
    pub(crate) fn comparison_readonly_has_focus(&self, window: &Window, cx: &App) -> bool {
        self.document_comparison.as_ref().is_some_and(|comparison| {
            [&comparison.left_editor, &comparison.right_editor]
                .into_iter()
                .any(|editor| {
                    !editor.read(cx).presentation().is_editable()
                        && editor
                            .read(cx)
                            .focus_handle(cx)
                            .contains_focused(window, cx)
                })
        })
    }

    /// Render each existing entity once, with independent native scrolling and local appearance.
    pub(crate) fn render_document_comparison(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        // The shell synchronizes lifetime/version validity before composing this view.
        if !self.document_comparison_is_current() {
            return None;
        }
        let comparison = self.document_comparison.as_ref()?;
        let left = comparison.left_editor.clone();
        let left_title = comparison.left_title.clone();
        let right_title = comparison.right_title.clone();
        let left_label = format!(
            "{} · {} · {}",
            t!("editor.diff_left"),
            left_title,
            t!("editor.readonly_label")
        );
        let right_label = format!("{} · {}", t!("editor.diff_right"), right_title);
        let left_input = crate::ui::controls::readonly_editor(
            "document-diff-left-input",
            left_label.clone(),
            &left,
            window,
            cx,
        );
        let right = self.render_native_editor(window, cx);
        let menu_target = self.plugin_menu_target(Path::new(&comparison.left.path));
        let left_focus = left.clone();
        Some(
            div()
                .flex()
                .flex_col()
                .size_full()
                .min_h_0()
                .child(
                    div().flex().justify_end().child(
                        Button::new("close-document-diff")
                            .label(t!("editor.close_diff").to_string())
                            .accessibility_label(t!("editor.close_diff").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_plugin_document_menu(window, cx);
                                this.close_document_comparison(cx);
                                // An explicit Close always returns keys to the current document;
                                // automatic invalidation does so only for an unmounted focused pane.
                                this.editor
                                    .update(cx, |editor, cx| editor.focus(window, cx));
                            })),
                    ),
                )
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_h_0()
                        .child(
                            div()
                                .id("document-diff-left-region")
                                .role(gpui_kit::Role::Group)
                                .aria_label(left_label)
                                .debug_selector(|| "document-diff-left".into())
                                // Admit host document shortcuts even inside the panel's PluginSurface.
                                .key_context("NativeEditorSource")
                                .capture_any_mouse_down(cx.listener(
                                    move |this, event: &MouseDownEvent, window, cx| {
                                        if event.button == MouseButton::Right {
                                            // The actual left gesture owns focus/selection; active right-tab
                                            // state is never substituted for its captured resource version.
                                            left_focus
                                                .update(cx, |editor, cx| editor.focus(window, cx));
                                            this.open_plugin_document_menu(
                                                menu_target.clone(),
                                                event.position,
                                                window,
                                                cx,
                                            );
                                            cx.stop_propagation();
                                        }
                                    },
                                ))
                                .flex()
                                .flex_col()
                                .flex_1()
                                .w_0()
                                .min_h_0()
                                .overflow_hidden()
                                .child(div().px_2().child(format!(
                                    "{} · {}",
                                    t!("editor.diff_left"),
                                    left_title
                                )))
                                .child(div().flex_1().min_h_0().child(left_input)),
                        )
                        .child(
                            div()
                                .id("document-diff-right-region")
                                .role(gpui_kit::Role::Group)
                                .aria_label(right_label)
                                .test_support()
                                .debug_selector(|| "document-diff-right".into())
                                .flex()
                                .flex_col()
                                .flex_1()
                                .w_0()
                                .min_h_0()
                                .overflow_hidden()
                                .child(div().px_2().child(format!(
                                    "{} · {}",
                                    t!("editor.diff_right"),
                                    right_title
                                )))
                                .child(right),
                        ),
                )
                .into_any_element(),
        )
    }
}

/// Compute byte ranges for line changes without retaining a second document model.
mod diff {
    use std::ops::Range;

    /// A paired hunk is a modification; a one-sided hunk is an insertion or deletion.
    #[derive(Debug, PartialEq)]
    pub(super) struct Hunk {
        pub left: Range<usize>,
        pub right: Range<usize>,
    }

    /// Reject excessive input before allocating the bounded LCS matrix.
    pub(super) fn compare(left: &str, right: &str) -> Result<Vec<Hunk>, ()> {
        if left.len() > 1024 * 1024 || right.len() > 1024 * 1024 {
            return Err(());
        }
        // Count with an early stop before allocating per-line references for hostile input.
        if left.split_inclusive('\n').take(2001).count() > 2000
            || right.split_inclusive('\n').take(2001).count() > 2000
        {
            return Err(());
        }
        let a = left.split_inclusive('\n').collect::<Vec<_>>();
        let b = right.split_inclusive('\n').collect::<Vec<_>>();
        if a.len() > 2000 || b.len() > 2000 || a.len() * b.len() > 4_000_000 {
            return Err(());
        }
        // The final row/column are implicit zeroes; u16 holds at most 2000 matched lines.
        let mut table = vec![0u16; a.len() * b.len()];
        let cell = |table: &[u16], i: usize, j: usize| {
            if i < a.len() && j < b.len() {
                table[i * b.len() + j]
            } else {
                0
            }
        };
        for i in (0..a.len()).rev() {
            for j in (0..b.len()).rev() {
                table[i * b.len() + j] = if a[i] == b[j] {
                    1 + cell(&table, i + 1, j + 1)
                } else {
                    cell(&table, i + 1, j).max(cell(&table, i, j + 1))
                };
            }
        }
        let (mut i, mut j, mut left_offset, mut right_offset) = (0, 0, 0, 0);
        let mut hunks = Vec::new();
        while i < a.len() || j < b.len() {
            if i < a.len() && j < b.len() && a[i] == b[j] {
                left_offset += a[i].len();
                right_offset += b[j].len();
                i += 1;
                j += 1;
                continue;
            }
            let (start_left, start_right) = (left_offset, right_offset);
            while i < a.len() || j < b.len() {
                if i < a.len() && j < b.len() && a[i] == b[j] {
                    break;
                }
                if i < a.len() && (j == b.len() || cell(&table, i + 1, j) >= cell(&table, i, j + 1))
                {
                    left_offset += a[i].len();
                    i += 1;
                } else {
                    right_offset += b[j].len();
                    j += 1;
                }
            }
            hunks.push(Hunk {
                left: start_left..left_offset,
                right: start_right..right_offset,
            });
        }
        Ok(hunks)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Guard limits and empty-side differences must not require an unbounded matrix.
        #[test]
        fn bounds_work_and_handles_empty_and_identical_documents() {
            assert!(compare(&"x".repeat(1024 * 1024 + 1), "").is_err());
            assert!(compare(&"x\n".repeat(2001), "").is_err());
            assert!(
                compare(&"x\n".repeat(2000), &"x\n".repeat(2000))
                    .unwrap()
                    .is_empty()
            );
            assert!(compare("", "").unwrap().is_empty());
            assert_eq!(
                compare("", "新").unwrap(),
                vec![Hunk {
                    left: 0..0,
                    right: 0..3
                }]
            );
            assert_eq!(
                compare("旧", "").unwrap(),
                vec![Hunk {
                    left: 0..3,
                    right: 0..0
                }]
            );
        }

        #[test]
        fn preserves_unicode_crlf_and_classifies_changed_lines() {
            assert_eq!(
                compare("同\r\n旧\r\n尾", "同\r\n新\r\n尾").unwrap(),
                vec![Hunk {
                    left: 5..10,
                    right: 5..10
                }]
            );
            assert_eq!(
                compare("a\n", "a\nb\n").unwrap(),
                vec![Hunk {
                    left: 2..2,
                    right: 2..4
                }]
            );
        }
    }
}
