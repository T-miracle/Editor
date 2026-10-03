//! Move a pressed text selection through the base editor's public layout and editing APIs.

use crate::*;
use gpui_base::{AutoScroll, input::Rope};
use gpui_kit::{CursorStyle, EntityId, MouseMoveEvent, canvas, fill};
use std::ops::Range;

#[cfg(test)]
mod tests;

/// Gesture state holds no editable text or history; EditorState remains their sole owner.
#[derive(Default)]
pub(crate) struct TextDragState {
    gesture: Option<TextDrag>,
    auto_scroll: AutoScroll,
}

/// A read-only, structurally shared snapshot also detects disk reloads that suppress Change.
struct TextDrag {
    editor: EntityId,
    revision: Option<u64>,
    snapshot: Rope,
    source: Range<usize>,
    press: Point<Pixels>,
    press_offset: usize,
    position: Point<Pixels>,
    dragging: bool,
}

/// A hit includes the glyph under the pointer so empty space cannot start a selection drag.
#[derive(Clone)]
struct TextHit {
    offset: usize,
    bounds: Bounds<Pixels>,
    glyph: Option<Range<usize>>,
}

impl TextDrag {
    /// Document identity, revision and selection must still match the original press.
    fn matches(&self, app: &EditorApp, cx: &App) -> bool {
        app.editor.entity_id() == self.editor
            && app
                .active_tab_index()
                .map(|index| app.tabs[index].session.revision())
                == self.revision
            && app.editor.read(cx).is_editable()
            && app.editor.read(cx).selected_range() == self.source
    }
}

impl EditorApp {
    /// Delay a plain single click on selected glyphs while leaving selection modifiers native.
    pub(super) fn text_drag_press(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_text_drag(cx);
        if event.button != MouseButton::Left
            || event.click_count != 1
            || event.modifiers != Modifiers::default()
            || window.default_prevented()
        {
            return;
        }
        let editor = self.editor.read(cx);
        let source = editor.selected_range();
        if !editor.is_editable()
            || source.is_empty()
            || editor.search_session().open
            || editor.code_action_menu_state().open
        {
            return;
        }
        let Some(hit) = hit_test(editor, event.position, window, cx) else {
            return;
        };
        let selected_glyph = hit
            .glyph
            .as_ref()
            .is_some_and(|glyph| source.start <= glyph.start && glyph.end <= source.end);
        // A selected newline has a highlight after the line's last glyph, including on empty lines.
        let newline_length = match editor.text().char_at(hit.offset) {
            Some('\n') => 1,
            Some('\r') if editor.text().char_at(hit.offset + 1) == Some('\n') => 2,
            _ => 0,
        };
        let selected_newline = newline_length > 0
            && source.start <= hit.offset
            && hit.offset + newline_length <= source.end
            && event.position.x >= hit.bounds.left()
            && hit.bounds.top() <= event.position.y
            && event.position.y < hit.bounds.bottom();
        if !selected_glyph && !selected_newline {
            return;
        }
        self.editor_text_drag.gesture = Some(TextDrag {
            editor: self.editor.entity_id(),
            revision: self
                .active_tab_index()
                .map(|index| self.tabs[index].session.revision()),
            snapshot: editor.text().clone(),
            source,
            press: event.position,
            press_offset: hit.offset,
            position: event.position,
            dragging: false,
        });
        // Suppress Root's selectable-text handling as well as the base editor's press.
        gpui_base::GlobalState::suppress_text_selection(cx);
        self.dismiss_pointer_hover(cx);
        self.editor.update(cx, |editor, cx| {
            editor.dismiss_completion_overlay(cx);
        });
        self.editor.focus_handle(cx).focus(window, cx);
        window.prevent_default();
        cx.stop_propagation();
        self.notify_text_drag(cx);
    }

    /// Keep the source selection intact and scroll at viewport edges during the move.
    fn text_drag_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(gesture) = self.editor_text_drag.gesture.as_ref() else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) || !gesture.matches(self, cx) {
            self.cancel_text_drag(cx);
            return;
        }
        let gesture = self.editor_text_drag.gesture.as_mut().unwrap();
        gesture.position = event.position;
        // Small hand movements still count as a click, without changing text on press.
        let distance = event.position - gesture.press;
        gesture.dragging |= distance.x.abs().max(distance.y.abs()) >= px(4.);
        if gesture.dragging {
            let viewport = self.editor.read(cx).input_bounds();
            let delta = AutoScroll::compute_delta(event.position.y, viewport).map(|delta| -delta);
            self.editor_text_drag
                .auto_scroll
                .set(delta, cx, |delta, app, cx| {
                    if app
                        .editor_text_drag
                        .gesture
                        .as_ref()
                        .is_none_or(|gesture| !gesture.matches(app, cx))
                    {
                        app.cancel_text_drag(cx);
                        return;
                    }
                    app.editor.update(cx, |editor, cx| {
                        let offset = editor.scroll_offset();
                        editor.set_scroll_offset(point(offset.x, offset.y + delta), cx);
                    });
                    app.notify_text_drag(cx);
                });
        }
        window.prevent_default();
        cx.stop_propagation();
        self.notify_text_drag(cx);
    }

    /// Commit one replacement, so a move never exposes a separate deletion to undo or LSP.
    fn text_drag_release(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button != MouseButton::Left {
            return;
        }
        let Some(gesture) = self.editor_text_drag.gesture.take() else {
            return;
        };
        self.editor_text_drag.auto_scroll.stop();
        window.prevent_default();
        cx.stop_propagation();
        self.notify_text_drag(cx);
        if !gesture.matches(self, cx) || self.editor.read(cx).text() != &gesture.snapshot {
            return;
        }
        if !gesture.dragging {
            // A click that never became a drag retains normal caret-placement behavior.
            self.editor.update(cx, |editor, cx| {
                editor.set_selected_range(gesture.press_offset..gesture.press_offset, cx);
            });
            return;
        }
        let Some(hit) = hit_test(self.editor.read(cx), event.position, window, cx) else {
            // Releasing outside the editor cancels without deleting the selected text.
            return;
        };
        let Some((edit, selection)) = move_edit(&gesture.snapshot, &gesture.source, hit.offset)
        else {
            return;
        };
        self.editor.update(cx, |editor, cx| {
            // A single contiguous edit uses the upstream atomic history entry and Change event.
            editor.apply_lsp_edits(&vec![edit], window, cx);
            editor.set_selected_range(selection, cx);
        });
    }

    /// Escape abandons the move while retaining both the original text and selection.
    pub(super) fn text_drag_escape(
        &mut self,
        _: &gpui_base::input::Escape,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor_text_drag.gesture.is_some() {
            self.cancel_text_drag(cx);
            cx.stop_propagation();
        }
    }

    /// Stop the shared base autoscroller whenever a gesture is canceled or invalidated.
    pub(super) fn cancel_text_drag(&mut self, cx: &mut Context<Self>) {
        if self.editor_text_drag.gesture.take().is_some() {
            self.editor_text_drag.auto_scroll.stop();
            self.notify_text_drag(cx);
        }
    }

    /// The dock has its own render cache, so gesture changes must invalidate its editor panel.
    fn notify_text_drag(&self, cx: &mut Context<Self>) {
        self.editor_panel.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    /// Global capture listeners keep control of the gesture even outside the editor hitbox.
    pub(super) fn render_text_drag_events(&self, cx: &Context<Self>) -> impl IntoElement {
        let app = cx.entity().downgrade();
        canvas(
            |_, _, _| (),
            move |_, (), window, _| {
                let moving = app.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase.capture() {
                        let _ = moving.update(cx, |app, cx| app.text_drag_move(event, window, cx));
                    }
                });
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if phase.capture() {
                        let _ = app.update(cx, |app, cx| app.text_drag_release(event, window, cx));
                    }
                });
            },
        )
        .absolute()
        .size_full()
    }

    /// Draw a theme-colored insertion caret without changing the editor's source selection.
    pub(super) fn render_text_drag_caret(&self, cx: &Context<Self>) -> impl IntoElement {
        let app = cx.entity().downgrade();
        canvas(
            |_, _, _| (),
            move |_, (), window, cx| {
                let Ok(presentation) = app.read_with(cx, |app, cx| {
                    let gesture = app.editor_text_drag.gesture.as_ref()?;
                    if !gesture.dragging || !gesture.matches(app, cx) {
                        return None;
                    }
                    let editor = app.editor.read(cx);
                    let hit = hit_test(editor, gesture.position, window, cx)?;
                    Some((hit, gesture.source.clone(), editor.input_bounds()))
                }) else {
                    return;
                };
                let Some((hit, source, viewport)) = presentation else {
                    return;
                };
                window.set_window_cursor_style(CursorStyle::ClosedHand);
                if hit.offset >= source.start && hit.offset <= source.end {
                    return;
                }
                let caret = Bounds::new(hit.bounds.origin, size(px(2.), hit.bounds.size.height));
                window.with_content_mask(
                    Some(gpui_kit::ContentMask { bounds: viewport }),
                    |window| {
                        window.paint_quad(fill(caret, cx.theme().caret));
                    },
                );
            },
        )
        .absolute()
        .size_full()
    }
}

/// Reorder only the span between source and destination, preserving literal bytes and newlines.
fn move_edit(
    text: &Rope,
    source: &Range<usize>,
    target: usize,
) -> Option<(lsp_types::TextEdit, Range<usize>)> {
    if source.is_empty() || (source.start..=source.end).contains(&target) {
        return None;
    }
    let selected = text.slice(source.clone()).to_string();
    let (range, replacement, selection) = if target < source.start {
        let between = text.slice(target..source.start).to_string();
        (
            target..source.end,
            selected + &between,
            target..target + source.len(),
        )
    } else {
        let between = text.slice(source.end..target).to_string();
        (
            source.start..target,
            between + &selected,
            target - source.len()..target,
        )
    };
    Some((
        lsp_types::TextEdit {
            range: lsp_types::Range::new(
                text.offset_to_position(range.start),
                text.offset_to_position(range.end),
            ),
            new_text: replacement,
        },
        selection,
    ))
}

/// Share the existing native layout hit test with external document input without duplicating caret geometry.
pub(crate) fn caret_offset_at(
    editor: &EditorState,
    position: Point<Pixels>,
    window: &Window,
    cx: &App,
) -> Option<usize> {
    hit_test(editor, position, window, cx).map(|hit| hit.offset)
}

/// Find the nearest laid-out caret in logarithmic time, including wrapped or folded source rows.
fn hit_test(
    editor: &EditorState,
    position: Point<Pixels>,
    window: &Window,
    cx: &App,
) -> Option<TextHit> {
    if !editor.input_bounds().contains(&position) {
        return None;
    }
    let text = editor.text();
    let visible = editor.visible_row_range()?;
    let first_byte = text.line_start_offset(visible.start);
    // Horizontal scrolling can put glyphs behind the fixed line-number gutter.
    let content_left =
        editor.range_to_bounds(&(first_byte..first_byte))?.left() - editor.scroll_offset().x;
    if position.x < content_left {
        return None;
    }
    let start = text.offset_to_char_index(first_byte);
    let end = text.offset_to_char_index(
        text.line_end_offset(
            visible
                .end
                .saturating_sub(1)
                .min(text.lines_len().saturating_sub(1)),
        ),
    );
    let caret = |index| {
        let mut offset = text.char_index_to_offset(index);
        // A CRLF is one cursor boundary even though it occupies two bytes.
        if offset > 0
            && text.char_at(offset - 1) == Some('\r')
            && text.char_at(offset) == Some('\n')
        {
            offset -= 1;
        }
        Some(TextHit {
            offset,
            bounds: editor.range_to_bounds(&(offset..offset))?,
            glyph: None,
        })
    };
    let first = lower_bound(start..end + 1, |index| {
        caret(index).is_some_and(|hit| hit.bounds.bottom() <= position.y)
    })
    .min(end);
    let row = caret(first)?;
    let row_end = lower_bound(first..end + 1, |index| {
        caret(index).is_some_and(|hit| hit.bounds.top() <= row.bounds.top())
    });
    let right_index = lower_bound(first..row_end, |index| {
        caret(index).is_some_and(|hit| hit.bounds.left() <= position.x)
    });
    // Prefer the last coincident boundary, which skips hidden folded text and zero-width glyphs.
    let left_index = if right_index == first {
        lower_bound(first..row_end, |index| {
            caret(index).is_some_and(|hit| hit.bounds.left() <= row.bounds.left())
        })
        .saturating_sub(1)
    } else {
        right_index - 1
    };
    let mut left = caret(left_index)?;
    let right = if right_index < row_end {
        caret(right_index)
    } else {
        // The public range geometry places a wrap boundary on the next row.
        // Reconstruct its equally valid position after this row's last glyph.
        caret(row_end).and_then(|next| wrapped_row_end(editor, &left, next, window, cx))
    };
    let Some(right) = right else {
        return Some(left);
    };
    let glyph = (left.bounds.left() <= position.x && position.x < right.bounds.left())
        .then(|| left.offset..right.offset);
    if position.x - left.bounds.left() <= right.bounds.left() - position.x {
        left.glyph = glyph;
        Some(left)
    } else {
        Some(TextHit { glyph, ..right })
    }
}

/// Shape only the final glyph to expose the upstream wrap boundary's alternate caret position.
fn wrapped_row_end(
    editor: &EditorState,
    last: &TextHit,
    mut next: TextHit,
    window: &Window,
    cx: &App,
) -> Option<TextHit> {
    let text = editor.text();
    if next.bounds.top() <= last.bounds.top()
        || text.offset_to_point(last.offset).row != text.offset_to_point(next.offset).row
    {
        return None;
    }
    let glyph = text.slice(last.offset..next.offset).to_string();
    let glyph = if glyph == "\t" {
        // Host documents use four-column tabs; earlier tabs count by their displayed width.
        let start = text.line_start_offset(text.offset_to_point(last.offset).row);
        let column = text
            .slice(start..last.offset)
            .chars()
            .fold(0, |column, character| {
                column + if character == '\t' { 4 - column % 4 } else { 1 }
            });
        " ".repeat(4 - column % 4)
    } else {
        glyph
    };
    let font_size = component_styles(cx, ThemeComponent::Editor)
        .base
        .font_size_px
        .map(px)
        .unwrap_or(cx.theme().mono_font_size);
    let run = gpui_kit::TextRun {
        len: glyph.len(),
        font: gpui_kit::font(cx.theme().mono_font_family.clone()),
        color: cx.theme().foreground,
        ..Default::default()
    };
    let line = window
        .text_system()
        .shape_line(glyph.into(), font_size, &[run], None);
    next.bounds = Bounds::new(
        point(last.bounds.left() + line.width, last.bounds.top()),
        last.bounds.size,
    );
    Some(next)
}

/// Return the first index after a monotonic prefix without allocating glyph or line tables.
fn lower_bound(mut range: Range<usize>, before: impl Fn(usize) -> bool) -> usize {
    while range.start < range.end {
        let middle = range.start + (range.end - range.start) / 2;
        if before(middle) {
            range.start = middle + 1;
        } else {
            range.end = middle;
        }
    }
    range.start
}
