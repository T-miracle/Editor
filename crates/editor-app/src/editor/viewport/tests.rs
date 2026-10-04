//! Exercise source location through actual Base layouts, input and Undo.

use super::*;
use crate::ui::{theme, typography};
use gpui_base::input::{Editor, InputEvent};
use gpui_kit::{
    AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement as _, Render,
    Styled as _, Subscription, TestAppContext, VisualTestContext, div, gpui, point, px, size,
};
use std::{cell::Cell, rc::Rc};

/// Only the native editor is rendered, so async plugin discovery cannot replace the test document.
struct View {
    editor: Entity<EditorState>,
    _changes: Subscription,
}

impl Render for View {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w_full()
            .h_full()
            .text_size(px(14.))
            .child(Editor::new(&self.editor))
    }
}

/// Build deterministic text geometry and count text-change events independently of scrolling.
fn source_window<'a>(
    cx: &'a mut TestAppContext,
    source: &str,
    wrap: bool,
) -> (Entity<View>, Rc<Cell<usize>>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        theme::apply_theme(theme::builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let source = source.to_owned();
    let count = Rc::new(Cell::new(0));
    let events = count.clone();
    let (view, cx) = cx.add_window_view(move |window, cx| {
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("text")
                .line_number(true)
                .soft_wrap(wrap)
        });
        editor.update(cx, |editor, cx| editor.set_value(source, window, cx));
        let changes = cx.subscribe(&editor, move |_, _, event: &InputEvent, _| {
            if matches!(event, InputEvent::Change) {
                events.set(events.get() + 1);
            }
        });
        View {
            editor,
            _changes: changes,
        }
    });
    cx.simulate_resize(size(px(360.), px(180.)));
    cx.update(|window, cx| {
        view.read(cx).editor.focus_handle(cx).focus(window, cx);
        window.draw(cx).clear(cx);
    });
    (view, count, cx)
}

/// Apply only offsets returned by the public locator, repainting between measurements.
fn locate(view: &Entity<View>, offset: usize, fraction: f32, cx: &mut VisualTestContext) {
    let mut locator = Locator::new(offset, fraction);
    for _ in 0..64 {
        let step = cx.update(|_, cx| locator.step(view.read(cx).editor.read(cx)));
        match step {
            Step::Settled => return,
            Step::Failed => panic!("valid source location must settle"),
            Step::Scroll(offset) => paint_scroll(view, offset, cx),
        }
    }
    panic!("source location exceeded its bounded frame budget");
}

/// Observe the final clamp frame before asserting native row geometry.
fn paint_scroll(view: &Entity<View>, offset: Point<Pixels>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        view.read(cx).editor.clone().update(cx, |editor, cx| {
            editor.set_scroll_offset(offset, cx);
        });
        // Base applies and clamps deferred scroll after layout; the following
        // paint must expose the final geometry rather than the unclamped frame.
        window.draw(cx).clear(cx);
        window.draw(cx).clear(cx);
    });
}

/// A distant CRLF row reaches the real viewport without a selection or Undo side effect.
#[gpui::test]
fn source_viewport_distant_location_preserves_text_selection_focus_and_undo(
    cx: &mut TestAppContext,
) {
    let original: String = (0..1000)
        .map(|row| format!("row {row} 中文 source\r\n"))
        .collect();
    let (view, changes, cx) = source_window(cx, &original, false);
    cx.simulate_input("!");
    cx.update(|window, cx| {
        view.read(cx).editor.clone().update(cx, |editor, cx| {
            editor.set_selected_range(1..3, cx);
        });
        window.draw(cx).clear(cx);
    });
    let expected = format!("!{original}");
    let offset = expected.find("row 900 中文").unwrap();
    cx.update(|_, cx| {
        let editor = view.read(cx).editor.read(cx);
        assert!(!editor.visible_row_range().unwrap().contains(&900));
        assert_eq!(editor.text().to_string(), expected);
    });
    locate(&view, offset, 0.35, cx);
    cx.update(|window, cx| {
        let editor = view.read(cx).editor.read(cx);
        let target = editor.range_to_bounds(&(offset..offset)).unwrap();
        let height = editor.line_height().unwrap();
        assert!(editor.visible_row_range().unwrap().contains(&900));
        assert!((target.top() + height * 0.35 - editor.input_bounds().top()).abs() <= px(1.));
        assert_eq!(editor.text().to_string(), expected);
        assert_eq!(editor.selected_range(), 1..3);
        assert!(view.read(cx).editor.focus_handle(cx).is_focused(window));
        assert_eq!(changes.get(), 1, "scrolling must not emit a document edit");
    });
    cx.simulate_keystrokes("ctrl-z");
    cx.update(|_, cx| {
        assert_eq!(view.read(cx).editor.read(cx).text().to_string(), original);
        assert_eq!(
            changes.get(),
            2,
            "Undo must still target only the earlier input"
        );
    });
}

/// Soft-wrapped rows retain a UTF-8 anchor and fractional phase when the pane becomes narrower.
#[gpui::test]
fn source_viewport_samples_wrapped_row_and_relocates_after_width_change(cx: &mut TestAppContext) {
    let paragraph = "中文🎉 wrapped content ".repeat(250);
    let original = format!("{paragraph}\r\n末尾");
    let (view, changes, cx) = source_window(cx, &original, true);
    let height = cx.update(|_, cx| view.read(cx).editor.read(cx).line_height().unwrap());
    paint_scroll(&view, point(px(0.), -height * 18.375), cx);
    let anchor = cx.update(|window, cx| {
        let editor = view.read(cx).editor.read(cx);
        let anchor = sample(editor, window, cx).expect("painted wrapped rows must have an anchor");
        assert!(original.is_char_boundary(anchor.offset));
        assert!(anchor.offset > 0 && anchor.offset < paragraph.len());
        assert!((anchor.line_fraction - 0.375).abs() < 0.01);
        let row = editor
            .range_to_bounds(&(anchor.offset..anchor.offset))
            .unwrap();
        assert!(
            (row.top() + height * anchor.line_fraction - editor.input_bounds().top()).abs()
                <= px(1.)
        );
        anchor
    });
    cx.simulate_resize(size(px(220.), px(180.)));
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        window.draw(cx).clear(cx);
    });
    locate(&view, anchor.offset, anchor.line_fraction, cx);
    cx.update(|window, cx| {
        let editor = view.read(cx).editor.read(cx);
        let row = editor
            .range_to_bounds(&(anchor.offset..anchor.offset))
            .unwrap();
        assert!(
            (row.top() + height * anchor.line_fraction - editor.input_bounds().top()).abs()
                <= px(1.)
        );
        let updated = sample(editor, window, cx).unwrap();
        assert!(updated.offset <= anchor.offset);
        assert!(original.is_char_boundary(updated.offset));
        assert!((updated.line_fraction - anchor.line_fraction).abs() < 0.01);
        assert_eq!(editor.selected_range(), 0..0);
        assert_eq!(editor.text().to_string(), original);
        assert_eq!(changes.get(), 0);
    });
}

/// A backwards location is judged by the painted row, even if Base reports bounds for old offsets.
#[gpui::test]
fn source_viewport_locates_backwards_without_trusting_unlaid_out_bounds(cx: &mut TestAppContext) {
    let original: String = (0..600).map(|row| format!("row {row}\r\n")).collect();
    let (view, changes, cx) = source_window(cx, &original, false);
    let height = cx.update(|_, cx| view.read(cx).editor.read(cx).line_height().unwrap());
    paint_scroll(&view, point(px(0.), -height * 400.5), cx);
    let offset = original.find("row 12\r\n").unwrap();
    cx.update(|_, cx| {
        assert!(
            !view
                .read(cx)
                .editor
                .read(cx)
                .visible_row_range()
                .unwrap()
                .contains(&12)
        );
    });
    locate(&view, offset, 0.2, cx);
    cx.update(|_, cx| {
        let editor = view.read(cx).editor.read(cx);
        assert!(editor.visible_row_range().unwrap().contains(&12));
        let row = editor.range_to_bounds(&(offset..offset)).unwrap();
        assert!((row.top() + height * 0.2 - editor.input_bounds().top()).abs() <= px(1.));
        assert_eq!(editor.selected_range(), 0..0);
        assert_eq!(editor.text().to_string(), original);
        assert_eq!(changes.get(), 0);
    });
}

/// Valid non-scrollable document ends settle at their clamp; invalid offsets never propose scrolling.
#[gpui::test]
fn source_viewport_non_scrollable_end_settles_and_invalid_boundaries_fail(cx: &mut TestAppContext) {
    let original = "中文🎉\r\n短文";
    let (view, changes, cx) = source_window(cx, original, true);
    locate(&view, original.len(), 0.75, cx);
    cx.update(|window, cx| {
        let editor = view.read(cx).editor.read(cx);
        let row = editor
            .range_to_bounds(&(original.len()..original.len()))
            .unwrap();
        assert!(row.top() >= editor.input_bounds().top());
        assert!(row.bottom() <= editor.input_bounds().bottom());
        assert_eq!(editor.scroll_offset().y, px(0.));
        for (offset, fraction) in [
            (1, 0.),
            (original.len() + 1, 0.),
            (usize::MAX, 0.),
            (0, -0.1),
            (0, 1.1),
            (0, f32::NAN),
            (0, f32::INFINITY),
        ] {
            assert_eq!(Locator::new(offset, fraction).step(editor), Step::Failed);
        }
        let anchor = sample(editor, window, cx).unwrap();
        assert_eq!(anchor.offset, 0);
        assert_eq!(anchor.line_fraction, 0.);
        assert_eq!(editor.selected_range(), 0..0);
        assert_eq!(editor.text().to_string(), original);
        assert_eq!(changes.get(), 0);
    });
}
