//! Resolve editor hover through one app-owned pointer path for every viewport position.

use crate::*;
use gpui_kit::MouseMoveEvent;
use std::ops::Range;

impl EditorApp {
    /// Watch this editor window before shortcut actions consume their keystrokes.
    pub(crate) fn install_hover_keyboard_dismissal(
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        let target = window.window_handle();
        let app = cx.entity().downgrade();
        cx.intercept_keystrokes(move |_, window, cx| {
            // Application keystroke interceptors also receive settings/plugin windows.
            if window.window_handle() == target {
                let _ = app.update(cx, |app, cx| app.editor_keyboard_move(window, cx));
            }
        })
    }

    /// Compare the caret after key dispatch so copy and modifier keys keep details open.
    fn editor_keyboard_move(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.editor.clone();
        if !editor.read(cx).focus_handle(cx).is_focused(window) {
            // Selecting or copying popup text belongs to its own focus path.
            return;
        }
        let cursor = editor.read(cx).cursor();
        let app = cx.entity().downgrade();
        // Navigation actions run after interception. Observe their actual result
        // rather than duplicating the editor's keybindings and movement rules.
        window.defer(cx, move |_, cx| {
            let _ = app.update(cx, |app, cx| {
                if app.editor.entity_id() == editor.entity_id()
                    && editor.read(cx).cursor() != cursor
                {
                    app.dismiss_pointer_hover(cx);
                }
            });
        });
    }

    /// Keep hover on visible text while canceling the base editor's competing waiter.
    pub(super) fn editor_pointer_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A drag from the selectable details card must not dismiss it when
        // mouse movement propagates through the editor panel.
        if event.pressed_button.is_some() {
            return;
        }
        let context = (self.active_path.clone(), self.active_text_revision());
        if self.pointer_hover_context.as_ref() != Some(&context) {
            // A tab switch or edit invalidates both the cached card and the
            // Escape suppression range, even when the byte range is identical.
            self.pointer_hover_generation = self.pointer_hover_generation.wrapping_add(1);
            self.pointer_hover_context = Some(context.clone());
            self.pointer_hover_symbol = None;
            self.pointer_hover_pending = false;
            self.pointer_hover_cached = None;
            self.pointer_hover_suppressed = None;
        }
        // Syntax markers include punctuation, which the identifier hover path skips.
        let diagnostic = diagnostic_at_point(self.editor.read(cx), event.position);
        let symbol = diagnostic
            .as_ref()
            .map(|entry| {
                // Escape suppression covers the entire diagnostic, even when
                // its presentation is anchored to one visible character.
                let text = self.editor.read(cx).text();
                text.position_to_offset(&entry.diagnostic.range.start)
                    ..text.position_to_offset(&entry.diagnostic.range.end)
            })
            .or_else(|| symbol_at_point(self.editor.read(cx), event.position));
        if event.modifiers.secondary() {
            // Ctrl-hover belongs to the editor's definition navigation path.
            // Invalidate our pending detail request without clearing its underline.
            self.pointer_hover_generation = self.pointer_hover_generation.wrapping_add(1);
            self.pointer_hover_symbol = None;
            self.pointer_hover_pending = false;
            self.pointer_hover_cached = None;
            return;
        }
        // The base editor receives this event before the panel and may start a
        // hover task using stale scroll geometry. Cancel it before it can publish.
        self.editor
            .update(cx, |editor, cx| editor.clear_hover_state(cx));
        self.editor
            .update(cx, |editor, cx| editor.clear_diagnostic_popover(cx));
        if let Some(suppressed) = &self.pointer_hover_suppressed {
            if symbol.as_ref().is_some_and(|symbol| {
                suppressed.start <= symbol.start && symbol.end <= suppressed.end
            }) {
                return;
            }
            self.pointer_hover_suppressed = None;
        }
        if event.modifiers.alt {
            self.pointer_hover_generation = self.pointer_hover_generation.wrapping_add(1);
            self.pointer_hover_symbol = None;
            self.pointer_hover_pending = false;
            self.pointer_hover_cached = None;
            return;
        }
        if let Some(diagnostic) = diagnostic {
            // Cancel a pending type hover so it cannot cover the syntax explanation.
            self.pointer_hover_generation = self.pointer_hover_generation.wrapping_add(1);
            self.pointer_hover_symbol = symbol;
            self.pointer_hover_pending = false;
            self.pointer_hover_cached = None;
            self.editor
                .update(cx, |editor, cx| editor.present_diagnostic(diagnostic, cx));
            return;
        }
        if symbol == self.pointer_hover_symbol {
            if let (Some(symbol), Some(hover)) = (&symbol, &self.pointer_hover_cached) {
                // Restore the app-owned result in the same event after native
                // cancellation, without restarting its one-second pointer wait.
                let symbol = symbol.clone();
                let hover = hover.clone();
                self.editor.update(cx, |editor, cx| {
                    editor.present_hover(symbol, hover, cx);
                });
                return;
            }
            if self.pointer_hover_pending {
                return;
            }
        }
        self.pointer_hover_generation = self.pointer_hover_generation.wrapping_add(1);
        self.pointer_hover_symbol = symbol.clone();
        self.pointer_hover_pending = false;
        self.pointer_hover_cached = None;
        let Some(symbol) = symbol else {
            return;
        };
        let (source_path, source_revision) = context;
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
            // Only pointer presentation waits one second. The provider fetches
            // concurrently and remains immediately available to Ctrl+I.
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let result = task.await;
            let _ = this.update_in(cx, |app, _, cx| {
                if app.pointer_hover_generation != generation
                    || app.pointer_hover_symbol.as_ref() != Some(&symbol)
                    || app.active_path != source_path
                    || app.active_text_revision() != source_revision
                {
                    return;
                }
                app.pointer_hover_pending = false;
                match result {
                    Ok(Some(hover)) => {
                        app.pointer_hover_cached = Some(hover.clone());
                        app.editor.update(cx, |editor, cx| {
                            editor.present_hover(symbol, hover, cx);
                        });
                    }
                    Ok(None) => {}
                    Err(error) => tracing::warn!(%error, "pointer hover request failed"),
                }
            });
        })
        .detach();
    }

    /// Close details and invalidate any cached result that could reopen them.
    pub(super) fn dismiss_pointer_hover(&mut self, cx: &mut Context<Self>) {
        self.pointer_hover_suppressed = self
            .pointer_hover_symbol
            .clone()
            .or_else(|| {
                self.editor
                    .read(cx)
                    .diagnostic_popover()
                    .map(|entry| entry.range.clone())
            })
            .or_else(|| {
                self.editor
                    .read(cx)
                    .hover_popover()
                    .map(|hover| hover.symbol_range.clone())
            });
        self.pointer_hover_generation = self.pointer_hover_generation.wrapping_add(1);
        self.pointer_hover_symbol = None;
        self.pointer_hover_pending = false;
        self.pointer_hover_cached = None;
        // A focus restoration queued before editing or navigation must not
        // reintroduce a popup after this dismissal.
        self.definition_popup_focus.clear();
        self.editor
            .update(cx, |editor, cx| editor.clear_hover_state(cx));
        self.editor
            .update(cx, |editor, cx| editor.clear_diagnostic_popover(cx));
    }
}

/// Hit-test visible glyphs so errors still work after scrolling, wrapping, or folding.
fn diagnostic_at_point(
    editor: &EditorState,
    position: Point<Pixels>,
) -> Option<gpui_base::input::DiagnosticEntry> {
    if !editor.input_bounds().contains(&position) {
        return None;
    }
    let visible = editor.visible_row_range()?;
    let text = editor.text();
    for entry in editor.diagnostics()?.iter() {
        let rows = (entry.diagnostic.range.start.line as usize).max(visible.start)
            ..(entry.diagnostic.range.end.line as usize + 1).min(visible.end);
        for row in rows {
            let start = text.line_start_offset(row).max(entry.range.start);
            let end = text.line_end_offset(row).min(entry.range.end);
            if start >= end {
                continue;
            }
            let mut offset = start;
            for character in text.slice(start..end).chars() {
                let next = offset + character.len_utf8();
                if character != '\n'
                    && character != '\r'
                    && editor
                        .range_to_bounds(&(offset..next))
                        .is_some_and(|bounds| bounds.contains(&position))
                {
                    // Keep the original source location in `diagnostic`, but anchor
                    // the card at the hovered glyph when its first line is offscreen.
                    let mut presentation = entry.clone();
                    presentation.range = offset..next;
                    return Some(presentation);
                }
                offset = next;
            }
        }
    }
    None
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
        let mut byte_column = 0;
        for character in text.slice_line(row).chars().chain(std::iter::once(' ')) {
            if character.is_alphanumeric() || character == '_' {
                word_start.get_or_insert(byte_column);
            } else if let Some(start) = word_start.take() {
                // Input ranges use UTF-8 bytes, including when earlier text
                // on the same row contains multibyte characters.
                let range = line_start + start..line_start + byte_column;
                if editor
                    .range_to_bounds(&range)
                    .is_some_and(|bounds| bounds.contains(&position))
                {
                    return Some(range);
                }
            }
            byte_column += character.len_utf8();
        }
    }
    None
}
