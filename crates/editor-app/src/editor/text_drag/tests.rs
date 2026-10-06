//! Dispatch real pointer events to verify selection preservation, atomic moves and cancellation.

use super::*;
use gpui_kit::{TestAppContext, VisualTestContext, component::Root, gpui};
use std::cell::RefCell;

/// Open a plain-text document with deterministic wrapping and no language-server dependency.
fn drag_editor<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (
    tempfile::TempDir,
    Entity<EditorApp>,
    &'a mut VisualTestContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        theme::apply_theme(theme::builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("drag.txt");
    std::fs::write(&path, text).unwrap();
    let workspace = editor_core::Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let captured = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| EditorApp::new(workspace, Some(path), window, cx));
        *captured.borrow_mut() = Some(view.clone());
        Root::new(view, window, cx)
    });
    let view = slot.borrow_mut().take().unwrap();
    cx.simulate_resize(size(px(1000.), px(700.)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        view.read(cx).editor.clone().update(cx, |editor, cx| {
            editor.set_soft_wrap(false, window, cx);
        });
        window.draw(cx).clear(cx);
    });
    (directory, view, cx)
}

/// Paint a selected range, then press inside its first glyph rather than on an ambiguous edge.
fn select_and_press(view: &Entity<EditorApp>, source: Range<usize>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        view.read(cx).editor.clone().update(cx, |editor, cx| {
            editor.set_selected_range(source.clone(), cx);
        });
        window.draw(cx).clear(cx);
    });
    let position = cx.update(|_, cx| {
        let editor = view.read(cx).editor.read(cx);
        let end = source.start
            + editor
                .text()
                .slice(source.clone())
                .chars()
                .next()
                .unwrap()
                .len_utf8();
        editor
            .range_to_bounds(&(source.start..end))
            .unwrap()
            .center()
    });
    cx.simulate_mouse_down(position, MouseButton::Left, Modifiers::default());
    cx.update(|_, cx| {
        assert_eq!(view.read(cx).editor.read(cx).selected_range(), source);
        assert!(view.read(cx).editor_text_drag.gesture.is_some());
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Place the pointer at a byte offset using the base editor's painted caret geometry.
fn caret_position(
    view: &Entity<EditorApp>,
    offset: usize,
    cx: &mut VisualTestContext,
) -> Point<Pixels> {
    cx.update(|_, cx| {
        view.read(cx)
            .editor
            .read(cx)
            .range_to_bounds(&(offset..offset))
            .unwrap()
            .center()
    })
}

/// A move remains a selection gesture until release, with one dirty revision and one undo entry.
#[gpui::test]
fn text_drag_moves_unicode_forward_and_undoes_atomically(cx: &mut TestAppContext) {
    let content = "one 中文🎉 tail";
    let source = 4..4 + "中文🎉".len();
    let (_directory, view, cx) = drag_editor(cx, content);
    select_and_press(&view, source.clone(), cx);
    let destination = caret_position(&view, content.len(), cx);
    cx.simulate_mouse_move(destination, Some(MouseButton::Left), Modifiers::default());
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.read(cx).text().to_string(), content);
        assert_eq!(view.read(cx).editor.read(cx).selected_range(), source);
        window.draw(cx).clear(cx);
        // The source caret and destination caret share a color, but only the drop caret is 2px wide.
        assert!(window.painted_quads().iter().any(|quad| {
            quad.bounds.size.width == px(2.).scale(window.scale_factor())
                && quad.background.as_solid() == Some(cx.theme().caret)
        }));
    });
    cx.simulate_mouse_up(destination, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    let expected = "one  tail中文🎉";
    cx.update(|window, cx| {
        let app = view.read(cx);
        assert_eq!(app.editor.read(cx).text().to_string(), expected);
        assert_eq!(
            app.editor.read(cx).selected_range(),
            expected.len() - source.len()..expected.len()
        );
        let session = &app.tabs[app.active_tab_index().unwrap()]
            .text
            .as_ref()
            .unwrap()
            .session;
        assert!(session.is_dirty());
        assert_eq!(session.revision(), 1);
        window.draw(cx).clear(cx);
    });
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(view.read(cx).editor.read(cx).text().to_string(), content);
        assert_eq!(view.read(cx).editor.read(cx).selected_range(), source);
    });
    cx.simulate_keystrokes("ctrl-y");
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.read(cx).text().to_string(), expected));
}

/// Moving a multiline selection backward preserves the CRLFs and exact UTF-8 bytes.
#[gpui::test]
fn text_drag_moves_multiline_selection_backward(cx: &mut TestAppContext) {
    let content = "头\r\n中文🎉\r\n尾";
    let start = content.find("中文").unwrap();
    let source = start..start + "中文🎉\r\n".len();
    let (_directory, view, cx) = drag_editor(cx, content);
    select_and_press(&view, source.clone(), cx);
    let destination = caret_position(&view, 0, cx);
    cx.simulate_mouse_move(destination, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(destination, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(
            view.read(cx).editor.read(cx).text().to_string(),
            "中文🎉\r\n头\r\n尾"
        );
        assert_eq!(
            view.read(cx).editor.read(cx).selected_range(),
            0..source.len()
        );
        window.draw(cx).clear(cx);
    });
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.read(cx).text().to_string(), content));
}

/// Dropping anywhere in the original range, including either boundary, is an unchanged selection.
#[gpui::test]
fn text_drag_rejects_drops_in_original_selection(cx: &mut TestAppContext) {
    let content = "alpha beta gamma";
    let source = 6..10;
    let (_directory, view, cx) = drag_editor(cx, content);
    for offset in [source.start, 8, source.end] {
        select_and_press(&view, source.clone(), cx);
        let destination = caret_position(&view, offset, cx);
        cx.simulate_mouse_move(destination, Some(MouseButton::Left), Modifiers::default());
        cx.simulate_mouse_up(destination, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(view.read(cx).editor.read(cx).text().to_string(), content);
            assert_eq!(view.read(cx).editor.read(cx).selected_range(), source);
            assert!(
                !view.read(cx).tabs[0]
                    .text
                    .as_ref()
                    .unwrap()
                    .session
                    .is_dirty()
            );
        });
    }
}

/// Escape and release outside the viewport must leave the document and source selection intact.
#[gpui::test]
fn text_drag_cancels_on_escape_or_outside_release(cx: &mut TestAppContext) {
    let content = "alpha beta gamma";
    let source = 6..10;
    let (_directory, view, cx) = drag_editor(cx, content);
    select_and_press(&view, source.clone(), cx);
    let destination = caret_position(&view, content.len(), cx);
    cx.simulate_mouse_move(destination, Some(MouseButton::Left), Modifiers::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_keystrokes("escape");
    cx.simulate_mouse_up(destination, MouseButton::Left, Modifiers::default());
    cx.update(|_, cx| {
        assert!(view.read(cx).editor_text_drag.gesture.is_none());
        assert_eq!(view.read(cx).editor.read(cx).text().to_string(), content);
        assert_eq!(view.read(cx).editor.read(cx).selected_range(), source);
    });
    select_and_press(&view, source.clone(), cx);
    let outside = point(px(10.), destination.y);
    cx.simulate_mouse_move(outside, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(outside, MouseButton::Left, Modifiers::default());
    cx.update(|_, cx| {
        assert_eq!(view.read(cx).editor.read(cx).text().to_string(), content);
        assert_eq!(view.read(cx).editor.read(cx).selected_range(), source);
        assert!(!view.read(cx).editor_text_drag.auto_scroll.is_active());
    });
}

/// A press/release on selected text still places the caret when it never crosses the drag threshold.
#[gpui::test]
fn text_drag_plain_click_collapses_selection(cx: &mut TestAppContext) {
    let (_directory, view, cx) = drag_editor(cx, "alpha beta gamma");
    select_and_press(&view, 6..10, cx);
    let (position, offset) = cx.update(|_, cx| {
        let gesture = view.read(cx).editor_text_drag.gesture.as_ref().unwrap();
        (gesture.press, gesture.press_offset)
    });
    cx.simulate_mouse_up(position, MouseButton::Left, Modifiers::default());
    cx.update(|_, cx| {
        assert_eq!(
            view.read(cx).editor.read(cx).selected_range(),
            offset..offset
        )
    });
}

/// A drag beginning outside a selection keeps the native behavior of selecting text.
#[gpui::test]
fn text_drag_preserves_native_selection_gestures(cx: &mut TestAppContext) {
    let content = "alpha beta gamma";
    let (_directory, view, cx) = drag_editor(cx, content);
    let start = caret_position(&view, 0, cx);
    let end = caret_position(&view, 5, cx);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.read(cx).selected_range(), 0..5);
        assert_eq!(view.read(cx).editor.read(cx).text().to_string(), content);
        assert!(view.read(cx).editor_text_drag.gesture.is_none());
        window.draw(cx).clear(cx);
    });
    let modifiers = Modifiers {
        shift: true,
        ..Default::default()
    };
    let position = caret_position(&view, 3, cx);
    cx.simulate_mouse_down(position, MouseButton::Left, modifiers);
    cx.simulate_mouse_up(position, MouseButton::Left, modifiers);
    cx.update(|_, cx| {
        assert!(view.read(cx).editor_text_drag.gesture.is_none());
        assert_eq!(view.read(cx).editor.read(cx).text().to_string(), content);
    });
}

/// Even a same-length disk reload, which emits no Change, invalidates the pending text move.
#[gpui::test]
fn text_drag_discards_stale_snapshot(cx: &mut TestAppContext) {
    let content = "alpha beta gamma";
    let source = 6..10;
    let (_directory, view, cx) = drag_editor(cx, content);
    select_and_press(&view, source.clone(), cx);
    let destination = caret_position(&view, content.len(), cx);
    cx.simulate_mouse_move(destination, Some(MouseButton::Left), Modifiers::default());
    cx.update(|window, cx| {
        view.read(cx).editor.clone().update(cx, |editor, cx| {
            editor.set_value("alpha BETA gamma", window, cx);
            editor.set_selected_range(source.clone(), cx);
        });
        window.draw(cx).clear(cx);
    });
    cx.simulate_mouse_up(destination, MouseButton::Left, Modifiers::default());
    cx.update(|_, cx| {
        assert_eq!(
            view.read(cx).editor.read(cx).text().to_string(),
            "alpha BETA gamma"
        )
    });
}

/// Dragging selected text must respect readonly mode even though programmatic editing APIs bypass it.
#[gpui::test]
fn text_drag_respects_readonly(cx: &mut TestAppContext) {
    let content = "alpha beta gamma";
    let (_directory, view, cx) = drag_editor(cx, content);
    cx.update(|window, cx| {
        view.read(cx).editor.clone().update(cx, |editor, cx| {
            editor.set_selected_range(6..10, cx);
            editor.set_readonly(true, cx);
        });
        window.draw(cx).clear(cx);
    });
    let start = caret_position(&view, 7, cx);
    let end = caret_position(&view, content.len(), cx);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    cx.update(|_, cx| {
        assert!(view.read(cx).editor_text_drag.gesture.is_none());
        assert_eq!(view.read(cx).editor.read(cx).text().to_string(), content);
    });
}

/// Caret hit testing uses current screen geometry after vertical and horizontal scrolling.
#[gpui::test]
fn text_drag_hit_testing_tracks_scrolled_layout(cx: &mut TestAppContext) {
    let content = (0..100)
        .map(|row| format!("row {row:03}: 中文🎉 {}\n", "x".repeat(180)))
        .collect::<String>();
    let (_directory, view, cx) = drag_editor(cx, &content);
    let source_start = content.find("row 080:").unwrap();
    let source = source_start + 9..source_start + 9 + "中文🎉".len();
    cx.update(|window, cx| {
        view.read(cx).editor.clone().update(cx, |editor, cx| {
            editor.set_selected_range(source.clone(), cx);
            editor.set_scroll_offset(point(px(-50.), -editor.line_height().unwrap() * 70.), cx);
        });
        window.draw(cx).clear(cx);
    });
    select_and_press(&view, source.clone(), cx);
    let target = content.find("row 081:").unwrap() + 20;
    let destination = caret_position(&view, target, cx);
    cx.update(|window, cx| {
        assert_eq!(
            hit_test(view.read(cx).editor.read(cx), destination, window, cx)
                .unwrap()
                .offset,
            target
        )
    });
    cx.simulate_mouse_move(destination, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(destination, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(
            view.read(cx).editor.read(cx).selected_text().to_string(),
            "中文🎉"
        );
        assert_eq!(
            view.read(cx).editor.read(cx).selected_range(),
            target - source.len()..target
        );
    });
}

/// A soft-wrap boundary has two screen positions; the pointer chooses the end of its visual row.
#[gpui::test]
fn text_drag_hits_soft_wrap_end(cx: &mut TestAppContext) {
    let content = "abcdefghijklmnopqrstuvwx".repeat(12);
    let (_directory, view, cx) = drag_editor(cx, &content);
    cx.update(|window, cx| {
        view.read(cx).editor.clone().update(cx, |editor, cx| {
            editor.set_soft_wrap(true, window, cx);
        });
        window.draw(cx).clear(cx);
    });
    let (boundary, destination, glyph_position) = cx.update(|_, cx| {
        let editor = view.read(cx).editor.read(cx);
        let bounds = |offset| editor.range_to_bounds(&(offset..offset)).unwrap();
        let boundary = (1..content.len())
            .find(|&offset| bounds(offset).top() > bounds(offset - 1).top())
            .unwrap();
        let width = bounds(1).left() - bounds(0).left();
        let last = bounds(boundary - 1);
        (
            boundary,
            point(last.left() + width, last.center().y),
            point(last.left() + width / 4., last.center().y),
        )
    });
    cx.update(|window, cx| {
        let editor = view.read(cx).editor.read(cx);
        assert_eq!(
            hit_test(editor, destination, window, cx).unwrap().offset,
            boundary
        );
        assert_eq!(
            hit_test(editor, glyph_position, window, cx).unwrap().glyph,
            Some(boundary - 1..boundary)
        );
    });
    select_and_press(&view, 0..3, cx);
    cx.simulate_mouse_move(destination, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(destination, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(
            view.read(cx).editor.read(cx).selected_range(),
            boundary - 3..boundary
        )
    });
}

/// The base autoscroller runs while the pointer is held at the edge and stops with cancellation.
#[gpui::test]
fn text_drag_auto_scrolls_without_extending_source(cx: &mut TestAppContext) {
    let content = (0..100)
        .map(|row| format!("line {row:03}: alpha beta gamma\n"))
        .collect::<String>();
    let (_directory, view, cx) = drag_editor(cx, &content);
    let source = 10..15;
    select_and_press(&view, source.clone(), cx);
    let (edge, original_offset) = cx.update(|_, cx| {
        let editor = view.read(cx).editor.read(cx);
        let bounds = editor.input_bounds();
        (
            point(bounds.center().x, bounds.bottom() - px(2.)),
            editor.scroll_offset(),
        )
    });
    cx.simulate_mouse_move(edge, Some(MouseButton::Left), Modifiers::default());
    for _ in 0..3 {
        cx.executor().advance_clock(Duration::from_millis(16));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    cx.update(|_, cx| {
        assert!(view.read(cx).editor.read(cx).scroll_offset().y < original_offset.y);
        assert_eq!(view.read(cx).editor.read(cx).selected_range(), source);
        assert_eq!(view.read(cx).editor.read(cx).text().to_string(), content);
    });
    cx.simulate_keystrokes("escape");
    cx.simulate_mouse_up(edge, MouseButton::Left, Modifiers::default());
    cx.update(|_, cx| {
        assert!(!view.read(cx).editor_text_drag.auto_scroll.is_active());
        assert!(view.read(cx).editor_text_drag.gesture.is_none());
    });
}

/// Switching away and back cannot revive a gesture captured before the tab change.
#[gpui::test]
fn text_drag_cancels_on_tab_activation(cx: &mut TestAppContext) {
    let content = "alpha beta gamma";
    let (directory, view, cx) = drag_editor(cx, content);
    select_and_press(&view, 6..10, cx);
    let destination = caret_position(&view, content.len(), cx);
    cx.simulate_mouse_move(destination, Some(MouseButton::Left), Modifiers::default());
    let other = directory.path().join("other.txt");
    std::fs::write(&other, "other document").unwrap();
    cx.update(|window, cx| {
        view.update(cx, |app, cx| {
            app.open_file(other, window, cx);
            app.activate_tab(0, window, cx);
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_mouse_up(destination, MouseButton::Left, Modifiers::default());
    cx.update(|_, cx| {
        assert!(view.read(cx).editor_text_drag.gesture.is_none());
        assert_eq!(view.read(cx).editor.read(cx).text().to_string(), content);
    });
}

/// Gutter clicks cannot drag glyphs hidden underneath the line numbers after horizontal scrolling.
#[gpui::test]
fn text_drag_excludes_fixed_gutter(cx: &mut TestAppContext) {
    let content = "abcdefghijklmnopqrstuvwxyz".repeat(12);
    let (_directory, view, cx) = drag_editor(cx, &content);
    cx.update(|window, cx| {
        view.read(cx).editor.clone().update(cx, |editor, cx| {
            editor.set_selected_range(0..30, cx);
            editor.set_scroll_offset(point(px(-40.), px(0.)), cx);
        });
        window.draw(cx).clear(cx);
    });
    let position = cx.update(|_, cx| {
        let editor = view.read(cx).editor.read(cx);
        let start = editor.range_to_bounds(&(0..0)).unwrap();
        point(
            start.left() - editor.scroll_offset().x - px(4.),
            start.center().y,
        )
    });
    cx.update(|window, cx| {
        assert!(hit_test(view.read(cx).editor.read(cx), position, window, cx).is_none());
    });
    cx.simulate_mouse_down(position, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(position, MouseButton::Left, Modifiers::default());
    cx.update(|_, cx| {
        assert!(view.read(cx).editor_text_drag.gesture.is_none());
        assert_eq!(view.read(cx).editor.read(cx).text().to_string(), content);
    });
}

/// A selection containing only an empty line's newline can still be pressed on its highlight.
#[gpui::test]
fn text_drag_moves_selected_empty_line(cx: &mut TestAppContext) {
    let content = "head\n\ntail";
    let source = 5..6;
    let (_directory, view, cx) = drag_editor(cx, content);
    cx.update(|window, cx| {
        view.read(cx).editor.clone().update(cx, |editor, cx| {
            editor.set_selected_range(source.clone(), cx);
        });
        window.draw(cx).clear(cx);
    });
    let start = caret_position(&view, source.start, cx) + point(px(4.), px(0.));
    let destination = caret_position(&view, 0, cx);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.update(|_, cx| assert!(view.read(cx).editor_text_drag.gesture.is_some()));
    cx.simulate_mouse_move(destination, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(destination, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(
            view.read(cx).editor.read(cx).text().to_string(),
            "\nhead\ntail"
        );
        assert_eq!(view.read(cx).editor.read(cx).selected_range(), 0..1);
    });
}
