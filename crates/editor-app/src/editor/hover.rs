//! Recovers mouse hover on scrolled rows through the public editor layout API.

use crate::*;
use gpui_base::input::RopeExt as _;
use std::ops::Range;

impl EditorApp {
    /// Retry mouse hover when upstream hit testing loses scrolled rows.
    pub(super) fn on_editor_mouse_move(
        &mut self,
        event: &gpui_kit::MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hover_request_id = self.hover_request_id.wrapping_add(1);
        if event.pressed_button.is_some()
            || event.modifiers.alt
            || event.modifiers.secondary()
            || !self
                .editor
                .read(cx)
                .input_bounds()
                .contains(&event.position)
            || self.editor.read(cx).lsp().hover_provider.is_none()
        {
            return;
        }

        let request_id = self.hover_request_id;
        let position = event.position;
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let request = this
                .update_in(cx, |app, window, cx| {
                    if app.hover_request_id != request_id || window.mouse_position() != position {
                        return None;
                    }
                    // Ordinary visible-row hover may already have opened upstream.
                    let existing = app.editor.read(cx);
                    if existing.hover_popover().is_some_and(|hover| {
                        existing
                            .range_to_bounds(&hover.symbol_range)
                            .is_some_and(|bounds| bounds.contains(&position))
                    }) {
                        return None;
                    }
                    let path = app.active_path.clone();
                    let revision = app
                        .active_tab_index()
                        .map(|index| app.tabs[index].session.revision());
                    app.editor.update(cx, |editor, cx| {
                        let range = hovered_symbol_range(editor, position)?;
                        let provider = editor.lsp().hover_provider.clone()?;
                        let task = provider.hover(editor.text(), range.start, window, cx);
                        Some((path, revision, range, task))
                    })
                })
                .ok()
                .flatten();
            let Some((path, revision, range, task)) = request else {
                return;
            };
            let result = task.await;
            let _ = this.update_in(cx, |app, window, cx| {
                // A move, edit, tab switch or pointer exit invalidates this result.
                if app.hover_request_id != request_id
                    || window.mouse_position() != position
                    || app.active_path != path
                    || app
                        .active_tab_index()
                        .map(|index| app.tabs[index].session.revision())
                        != revision
                {
                    return;
                }
                match result {
                    Ok(Some(hover)) => app.editor.update(cx, |editor, cx| {
                        editor.present_hover(range, hover, cx);
                    }),
                    Ok(None) => {}
                    Err(error) => tracing::warn!(%error, "scrolled definition hover failed"),
                }
            });
        })
        .detach();
    }
}

/// Locate the visible identifier under the pointer using laid-out word bounds.
fn hovered_symbol_range(editor: &EditorState, position: Point<Pixels>) -> Option<Range<usize>> {
    if !editor.input_bounds().contains(&position) {
        return None;
    }
    let text = editor.text();
    let visible = editor.visible_row_range()?;
    let mut visible = visible.start.min(text.lines_len())..visible.end.min(text.lines_len());
    visible.find_map(|line| {
        let start = text.line_start_offset(line);
        let content = text.slice_line(line).to_string();
        let mut word_start = None;
        for (byte, ch) in content
            .char_indices()
            .chain(std::iter::once((content.len(), ' ')))
        {
            if ch.is_alphanumeric() || ch == '_' {
                word_start.get_or_insert(byte);
            } else if let Some(first) = word_start.take() {
                let range = start + first..start + byte;
                if editor
                    .range_to_bounds(&range)
                    .is_some_and(|bounds| bounds.contains(&position))
                {
                    return Some(range);
                }
            }
        }
        None
    })
}

#[cfg(test)]
mod tests;
