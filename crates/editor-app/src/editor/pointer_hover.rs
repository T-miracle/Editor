//! Resolve editor hover in the app viewport while upstream scroll hit testing uses stale bounds.

use crate::*;
use gpui_kit::MouseMoveEvent;
use std::ops::Range;

impl EditorApp {
    /// Keep hover on visible scrolled text without replacing the base editor state.
    pub(super) fn editor_pointer_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (scrolled, symbol, already_shown) = {
            let state = self.editor.read(cx);
            (
                state.scroll_offset().y != px(0.),
                symbol_at_point(state, event.position),
                state.hover_popover().is_some(),
            )
        };
        if !scrolled || event.modifiers.alt {
            self.pointer_hover_generation = self.pointer_hover_generation.wrapping_add(1);
            self.pointer_hover_symbol = None;
            self.pointer_hover_pending = false;
            return;
        }
        if symbol == self.pointer_hover_symbol && (self.pointer_hover_pending || already_shown) {
            return;
        }
        self.pointer_hover_generation = self.pointer_hover_generation.wrapping_add(1);
        self.pointer_hover_symbol = symbol.clone();
        self.pointer_hover_pending = false;
        let Some(symbol) = symbol else {
            return;
        };
        let source_path = self.active_path.clone();
        let source_revision = self
            .active_tab_index()
            .map(|index| self.tabs[index].session.revision());
        let task = self.editor.update(cx, |editor, cx| {
            let provider = editor.lsp().hover_provider.clone()?;
            Some(provider.hover(editor.text(), symbol.start, window, cx))
        });
        let Some(task) = task else {
            return;
        };
        let generation = self.pointer_hover_generation;
        self.pointer_hover_pending = true;
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |app, _, cx| {
                if app.pointer_hover_generation != generation
                    || app.pointer_hover_symbol.as_ref() != Some(&symbol)
                    || app.active_path != source_path
                    || app
                        .active_tab_index()
                        .map(|index| app.tabs[index].session.revision())
                        != source_revision
                    || app.editor.read(cx).scroll_offset().y == px(0.)
                {
                    return;
                }
                app.pointer_hover_pending = false;
                match result {
                    Ok(Some(hover)) => app.editor.update(cx, |editor, cx| {
                        editor.present_hover(symbol, hover, cx);
                    }),
                    Ok(None) => {}
                    Err(error) => tracing::warn!(%error, "pointer hover request failed"),
                }
            });
        })
        .detach();
    }
}

/// Find a visible identifier through public layout geometry, without moving the caret.
fn symbol_at_point(editor: &EditorState, position: Point<Pixels>) -> Option<Range<usize>> {
    if !editor.input_bounds().contains(&position) {
        return None;
    }
    let text = editor.text();
    for row in editor.visible_row_range()? {
        if row >= text.lines_len() {
            break;
        }
        let line_start = text.line_start_offset(row);
        let mut word_start = None;
        for (column, character) in text
            .slice_line(row)
            .chars()
            .chain(std::iter::once(' '))
            .enumerate()
        {
            if character.is_alphanumeric() || character == '_' {
                word_start.get_or_insert(column);
            } else if let Some(start) = word_start.take() {
                let range = line_start + start..line_start + column;
                if editor
                    .range_to_bounds(&range)
                    .is_some_and(|bounds| bounds.contains(&position))
                {
                    return Some(range);
                }
            }
        }
    }
    None
}
