//! Regression coverage for hovering visible text after scrolling and resizing.

use crate::*;
use editor_core::Workspace;
use gpui_kit::{TestAppContext, component::Root, gpui, size};
use lsp_types::{Hover, HoverContents, MarkedString};
use std::{cell::Cell, cell::RefCell, rc::Rc};

#[path = "hover_keyboard_tests.rs"]
mod keyboard;

struct ReadyHover(Rc<Cell<usize>>);

impl gpui_base::input::HoverProvider for ReadyHover {
    fn hover(
        &self,
        _: &gpui_base::input::Rope,
        _: usize,
        _: &mut Window,
        _: &mut App,
    ) -> gpui_kit::Task<anyhow::Result<Option<Hover>>> {
        self.0.set(self.0.get() + 1);
        gpui_kit::Task::ready(Ok(Some(Hover {
            contents: HoverContents::Scalar(MarkedString::String("details".into())),
            range: None,
        })))
    }
}

#[gpui::test]
fn hover_visible_row_after_scroll(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        theme::apply_theme(theme::builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("scrolled.rs");
    let content = (0..120)
        .map(|line| format!("symbol_{line:03}\n"))
        .collect::<String>();
    std::fs::write(&path, content).unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| EditorApp::new(workspace, Some(path), window, cx));
        *capture.borrow_mut() = Some(view.clone());
        Root::new(view, window, cx)
    });
    let view = slot.borrow_mut().take().unwrap();
    // Activation is required for real focus/blur notifications; an inactive
    // test window would miss the focus transfer caused by selecting details.
    cx.update(|window, _| window.activate_window());
    cx.simulate_resize(size(px(1000.), px(400.)));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|_, cx| {
        let editor = view.read(cx).editor.clone();
        editor.update(cx, |state, cx| {
            state.set_scroll_offset(point(px(0.), px(-900.)), cx);
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let (expected, position) = cx.update(|_, cx| {
        let app = view.read(cx);
        let state = app.editor.read(cx);
        let mut visible = state.visible_row_range().unwrap();
        assert!(visible.start > 0);
        visible
            .find_map(|line| {
                let start = state.text().line_start_offset(line);
                let bounds = state.range_to_bounds(&(start..start + 6))?;
                state
                    .input_bounds()
                    .contains(&bounds.center())
                    .then_some((start, bounds.center()))
            })
            .expect("a scrolled identifier should be visible")
    });
    let requests = Rc::new(Cell::new(0));
    cx.update(|_, cx| {
        let editor = view.read(cx).editor.clone();
        editor.update(cx, |state, _| {
            state.lsp_mut().hover_provider = Some(Rc::new(ReadyHover(requests.clone())));
        });
    });
    cx.simulate_mouse_move(position, None::<MouseButton>, Default::default());
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        requests.get() > 0,
        "visible scrolled text must request hover"
    );
    assert!(cx.debug_bounds("editor-definition-card").is_some());
    let card_bounds = cx.debug_bounds("editor-definition-card").unwrap();
    let symbol_bounds = cx.update(|_, cx| {
        let app = view.read(cx);
        let state = app.editor.read(cx);
        state.range_to_bounds(&(expected..expected + 6)).unwrap()
    });
    // The card must sit wholly above or below the scrolled symbol.
    assert!(
        card_bounds.bottom() <= symbol_bounds.top() || card_bounds.top() >= symbol_bounds.bottom(),
        "card {card_bounds:?} overlaps symbol {symbol_bounds:?}"
    );
    let initial_requests = requests.get();
    cx.simulate_mouse_move(
        point(position.x + px(2.), position.y),
        None::<MouseButton>,
        Default::default(),
    );
    cx.update(|_, cx| {
        let app = view.read(cx);
        assert!(
            app.editor.read(cx).hover_popover().is_some(),
            "the same symbol must retain hover state before the next LSP result"
        );
    });
    assert_eq!(
        requests.get(),
        initial_requests,
        "a small move within the same symbol must not refetch details"
    );
    // Draw before queued LSP work completes to catch a one-frame dismissal.
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-definition-card").is_some(),
        "a small pointer move inside the symbol must keep details open"
    );
    cx.run_until_parked();
    cx.simulate_mouse_move(
        card_bounds.center(),
        None::<MouseButton>,
        Default::default(),
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-definition-card").is_some(),
        "moving into the card must keep details open"
    );
    cx.update(|window, cx| {
        let focus = view.read(cx).editor.read(cx).focus_handle(cx);
        focus.focus(window, cx);
        assert!(
            focus.is_focused(window),
            "the editor must own focus before the card click"
        );
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_mouse_down(card_bounds.center(), MouseButton::Left, Default::default());
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-definition-card").is_some(),
        "focus transfer on mouse down must retain details"
    );
    cx.simulate_mouse_up(card_bounds.center(), MouseButton::Left, Default::default());
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-definition-card").is_some(),
        "clicking the card must keep details open"
    );
    // Padding is still inside the card even when no selectable text owns
    // the click. Redraw on press to catch an immediate native dismissal.
    for position in [
        point(card_bounds.left() + px(10.), card_bounds.top() + px(10.)),
        point(card_bounds.left() + px(10.), card_bounds.bottom() - px(10.)),
        point(card_bounds.right() - px(10.), card_bounds.top() + px(10.)),
    ] {
        cx.simulate_mouse_move(position, None::<MouseButton>, Default::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.simulate_mouse_down(position, MouseButton::Left, Default::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            cx.debug_bounds("editor-definition-card").is_some(),
            "left press in popup padding at {position:?} must retain details"
        );
        cx.simulate_mouse_up(position, MouseButton::Left, Default::default());
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            cx.debug_bounds("editor-definition-card").is_some(),
            "left release in popup padding at {position:?} must retain details"
        );
    }
    // Drag across the selectable markdown, then use the normal Copy binding.
    let text_bounds = cx.debug_bounds("editor-definition-text").unwrap();
    let start = point(text_bounds.left() + px(3.), text_bounds.center().y);
    let end = point(text_bounds.right() - px(3.), start.y);
    assert!(
        card_bounds.contains(&start) && card_bounds.contains(&end),
        "selectable text {text_bounds:?} must fit inside card {card_bounds:?}"
    );
    cx.simulate_mouse_down(start, MouseButton::Left, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_mouse_move(end, MouseButton::Left, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_mouse_up(end, MouseButton::Left, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_keystrokes("ctrl-c");
    assert_eq!(
        cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text())),
        Some("details".to_string()),
        "text {text_bounds:?} must remain selectable in card {card_bounds:?}; bottom handle {:?}",
        cx.debug_bounds("editor-definition-resize-bottom")
    );
    // Base paints the selection after the glyphs, so its actual painted
    // color must leave the selected text visible underneath it.
    cx.update(|window, cx| {
        let selection = cx.theme().selection;
        let selected_text = text_bounds.scale(window.scale_factor());
        let colors = window
            .painted_quads()
            .into_iter()
            .filter(|quad| selected_text.contains(&quad.bounds.center()))
            .filter_map(|quad| quad.background.as_solid())
            .filter(|color| {
                color.h == selection.h && color.s == selection.s && color.l == selection.l
            })
            .collect::<Vec<_>>();
        assert!(
            !colors.is_empty(),
            "selected text must have a visible highlight"
        );
        assert!(
            colors.iter().all(|color| color.a > 0. && color.a <= 0.35),
            "selection must not paint an opaque rectangle over the text: {colors:?}"
        );
    });
    // Selection movement must not leave a native hover waiter that can
    // replace the app-owned card after the pointer stops.
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(requests.get(), initial_requests);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("editor-definition-card").is_some());
    cx.update(|_, cx| {
        let editor = view.read(cx).editor.clone();
        let hover = editor.read(cx).hover_popover().cloned().unwrap();
        assert_eq!(hover.symbol_range.start, expected);
    });
    // Border drags must stay captured after leaving the card, with each
    // edge changing only its own dimension and retaining the details.
    let before_resize = cx.debug_bounds("editor-definition-card").unwrap();
    let right = cx
        .debug_bounds("editor-definition-resize-right")
        .unwrap()
        .center();
    let right_end = right + point(px(120.), px(0.));
    cx.simulate_mouse_down(right, MouseButton::Left, Default::default());
    cx.simulate_mouse_move(right_end, MouseButton::Left, Default::default());
    // Redraw during the drag to exercise retained capture across frames.
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_mouse_up(right_end, MouseButton::Left, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let wider = cx.debug_bounds("editor-definition-card").unwrap();
    assert!((wider.size.width - before_resize.size.width - px(120.)).abs() <= px(1.));
    assert_eq!(wider.size.height, before_resize.size.height);
    let bottom = cx
        .debug_bounds("editor-definition-resize-bottom")
        .unwrap()
        .center();
    let bottom_end = bottom + point(px(0.), px(80.));
    cx.simulate_mouse_down(bottom, MouseButton::Left, Default::default());
    cx.simulate_mouse_move(bottom_end, MouseButton::Left, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_mouse_up(bottom_end, MouseButton::Left, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let taller = cx.debug_bounds("editor-definition-card").unwrap();
    assert!((taller.size.height - wider.size.height - px(80.)).abs() <= px(1.));
    assert_eq!(taller.size.width, wider.size.width);
    // Oversized drags must preserve readable content inside the viewport,
    // while placement remains wholly on one side of the hovered symbol.
    for edge in [
        "editor-definition-resize-right",
        "editor-definition-resize-bottom",
    ] {
        let start = cx.debug_bounds(edge).unwrap().center();
        let end = start + point(px(3000.), px(3000.));
        cx.simulate_mouse_down(start, MouseButton::Left, Default::default());
        cx.simulate_mouse_move(end, MouseButton::Left, Default::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.simulate_mouse_up(end, MouseButton::Left, Default::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let resized = cx.debug_bounds("editor-definition-card").unwrap();
        assert!(resized.left() >= px(8.) && resized.right() <= px(992.));
        assert!(resized.top() >= px(8.) && resized.bottom() <= px(392.));
        assert!(resized.bottom() <= symbol_bounds.top() || resized.top() >= symbol_bounds.bottom());
    }
    cx.simulate_keystrokes("escape");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-definition-card").is_none(),
        "Escape must dismiss definition details"
    );
    cx.simulate_mouse_move(position, None::<MouseButton>, Default::default());
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-definition-card").is_none(),
        "Escape must keep the current symbol dismissed until the pointer leaves it"
    );
    let blank_after_escape = cx.update(|_, cx| {
        let app = view.read(cx);
        let input = app.editor.read(cx).input_bounds();
        point(input.right() - px(8.), position.y)
    });
    cx.simulate_mouse_move(blank_after_escape, None::<MouseButton>, Default::default());
    cx.simulate_mouse_move(position, None::<MouseButton>, Default::default());
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-definition-card").is_some(),
        "leaving and returning to the symbol must allow details again"
    );

    let (lower_range, lower_bounds) = cx.update(|window, cx| {
        let app = view.read(cx);
        let state = app.editor.read(cx);
        state
            .visible_row_range()
            .unwrap()
            .rev()
            .find_map(|row| {
                let start = state.text().line_start_offset(row);
                let range = start..start + 6;
                let bounds = state.range_to_bounds(&range)?;
                (state.input_bounds().contains(&bounds.center())
                    && window.bounds().bottom() - bounds.bottom() < px(51.))
                .then_some((range, bounds))
            })
            .expect("a visible symbol should be close to the window bottom")
    });
    cx.update(|_, cx| {
        let editor = view.read(cx).editor.clone();
        editor.update(cx, |state, cx| {
            state.present_hover(
                lower_range.clone(),
                Hover {
                    contents: HoverContents::Scalar(MarkedString::String(
                        "long details\n\n".repeat(100),
                    )),
                    range: None,
                },
                cx,
            );
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let upper_card = cx.debug_bounds("editor-definition-card").unwrap();
    // Near the lower edge the same card must flip wholly above the symbol.
    assert!(
        upper_card.bottom() <= lower_bounds.top(),
        "card {upper_card:?} should be above symbol {lower_bounds:?}"
    );
    assert!(upper_card.top() >= px(8.));
    // Directly presented, long details must survive focus transfer too;
    // restoring an older pointer cache would replace this documentation.
    cx.update(|window, cx| {
        view.read(cx)
            .editor
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let text_position = upper_card.origin + point(px(20.), px(20.));
    cx.simulate_mouse_move(text_position, None::<MouseButton>, Default::default());
    cx.simulate_click(text_position, Default::default());
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("editor-definition-card").is_some());
    cx.update(|_, cx| {
        let hover = view.read(cx).editor.read(cx).hover_popover().unwrap();
        assert_eq!(hover.symbol_range, lower_range);
        assert_eq!(
            hover.hover.contents,
            HoverContents::Scalar(MarkedString::String("long details\n\n".repeat(100)))
        );
    });
    // An upper card exposes the outer top edge: dragging down shrinks it,
    // and dragging up grows it while its bottom stays against the symbol.
    assert!(cx.debug_bounds("editor-definition-resize-bottom").is_none());
    let top = cx
        .debug_bounds("editor-definition-resize-top")
        .expect("an upper card must have a top resize handle");
    assert_eq!(top.top(), upper_card.top());
    let start = top.center();
    let end = start + point(px(0.), px(120.));
    cx.simulate_mouse_down(start, MouseButton::Left, Default::default());
    cx.simulate_mouse_move(end, MouseButton::Left, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_mouse_up(end, MouseButton::Left, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let shorter = cx.debug_bounds("editor-definition-card").unwrap();
    assert!((upper_card.size.height - shorter.size.height - px(120.)).abs() <= px(1.));
    assert_eq!(shorter.bottom(), upper_card.bottom());
    assert_eq!(shorter.size.width, upper_card.size.width);
    let start = cx
        .debug_bounds("editor-definition-resize-top")
        .unwrap()
        .center();
    let end = start - point(px(0.), px(60.));
    cx.simulate_mouse_down(start, MouseButton::Left, Default::default());
    cx.simulate_mouse_move(end, MouseButton::Left, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_mouse_up(end, MouseButton::Left, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let taller = cx.debug_bounds("editor-definition-card").unwrap();
    assert!((taller.size.height - shorter.size.height - px(60.)).abs() <= px(1.));
    assert_eq!(taller.bottom(), upper_card.bottom());
    // The right edge remains available in the upper placement as well.
    let start = cx
        .debug_bounds("editor-definition-resize-right")
        .unwrap()
        .center();
    let end = start + point(px(90.), px(0.));
    cx.simulate_mouse_down(start, MouseButton::Left, Default::default());
    cx.simulate_mouse_move(end, MouseButton::Left, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_mouse_up(end, MouseButton::Left, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let wider = cx.debug_bounds("editor-definition-card").unwrap();
    assert!((wider.size.width - taller.size.width - px(90.)).abs() <= px(1.));
    assert_eq!(wider.size.height, taller.size.height);
    let blank = cx.update(|_, cx| {
        let app = view.read(cx);
        let input = app.editor.read(cx).input_bounds();
        point(input.right() - px(8.), lower_bounds.center().y)
    });
    cx.simulate_mouse_move(blank, None::<MouseButton>, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-definition-card").is_none(),
        "leaving the symbol must dismiss its details"
    );
}

/// The first viewport and scrolled viewports must use the same pointer hover owner.
#[gpui::test]
fn hover_visible_row_without_scroll_uses_pointer_path(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        theme::apply_theme(theme::builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("top.rs");
    std::fs::write(&path, "中文 alpha beta\n").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| EditorApp::new(workspace, Some(path), window, cx));
        *capture.borrow_mut() = Some(view.clone());
        Root::new(view, window, cx)
    });
    let view = slot.borrow_mut().take().unwrap();
    cx.simulate_resize(size(px(1000.), px(600.)));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let symbol_range = 7..12;
    let position = cx.update(|_, cx| {
        let app = view.read(cx);
        let state = app.editor.read(cx);
        assert_eq!(state.scroll_offset().y, px(0.));
        state.range_to_bounds(&symbol_range).unwrap().center()
    });
    let requests = Rc::new(Cell::new(0));
    cx.update(|_, cx| {
        let editor = view.read(cx).editor.clone();
        editor.update(cx, |state, _| {
            state.lsp_mut().hover_provider = Some(Rc::new(ReadyHover(requests.clone())));
        });
    });
    cx.simulate_mouse_move(position, None::<MouseButton>, Default::default());
    // Both viewport positions must dispatch through the app path before the
    // upstream editor's separate 150 ms waiter can own this pointer.
    assert_eq!(requests.get(), 1);
    cx.update(|_, cx| {
        assert_eq!(view.read(cx).pointer_hover_symbol, Some(symbol_range));
    });
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(
        requests.get(),
        1,
        "the native hover waiter must be canceled"
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("editor-definition-card").is_some());
    cx.simulate_mouse_move(
        point(position.x + px(2.), position.y),
        None::<MouseButton>,
        Default::default(),
    );
    assert_eq!(requests.get(), 1, "the same word must reuse its result");
    cx.update(|_, cx| {
        assert!(view.read(cx).editor.read(cx).hover_popover().is_some());
    });
}

/// Resizing the window must use the symbol's newly painted horizontal position.
#[gpui::test]
fn hover_card_tracks_symbol_after_window_resize(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        theme::apply_theme(theme::builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("resize.rs");
    std::fs::write(&path, "alpha beta\n").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    // A restored desktop window has a screen origin; text geometry still
    // starts at the content area's local origin when either edge is resized.
    let window = cx.update(|cx| {
        cx.open_window(
            gpui_kit::WindowOptions {
                window_bounds: Some(gpui_kit::WindowBounds::Windowed(gpui_kit::Bounds::new(
                    point(px(550.), px(100.)),
                    size(px(1000.), px(600.)),
                ))),
                ..Default::default()
            },
            move |window, cx| {
                cx.new(|cx| {
                    let view = cx.new(|cx| EditorApp::new(workspace, Some(path), window, cx));
                    *capture.borrow_mut() = Some(view.clone());
                    Root::new(view, window, cx)
                })
            },
        )
        .unwrap()
    });
    let cx = &mut gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let view = slot.borrow_mut().take().unwrap();
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|_, cx| {
        let editor = view.read(cx).editor.clone();
        editor.update(cx, |state, cx| {
            state.present_hover(
                0..5,
                Hover {
                    contents: HoverContents::Scalar(MarkedString::String("details".into())),
                    range: None,
                },
                cx,
            );
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    for width in [1400., 900., 1700., 1000.] {
        cx.simulate_resize(size(px(width), px(600.)));
        // Assert the first resized frame, before a later redraw can hide a
        // previous-frame anchor captured during the render phase.
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let card = cx.debug_bounds("editor-definition-card").unwrap();
        let symbol = cx.update(|_, cx| {
            view.read(cx)
                .editor
                .read(cx)
                .range_to_bounds(&(0..5))
                .unwrap()
        });
        assert!(
            (card.left() - symbol.left()).abs() <= px(1.),
            "width {width}: card {card:?} must align with current symbol {symbol:?}"
        );
    }
    // A wider card can move left only to fit the local right edge, then
    // realign with its symbol when the window becomes wide enough again.
    cx.update(|_, cx| {
        let editor = view.read(cx).editor.clone();
        editor.update(cx, |state, cx| {
            state.present_hover(
                0..5,
                Hover {
                    contents: HoverContents::Scalar(MarkedString::String("details ".repeat(30))),
                    range: None,
                },
                cx,
            );
        });
    });
    for width in [650., 1400.] {
        cx.simulate_resize(size(px(width), px(600.)));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let card = cx.debug_bounds("editor-definition-card").unwrap();
        let symbol = cx.update(|_, cx| {
            view.read(cx)
                .editor
                .read(cx)
                .range_to_bounds(&(0..5))
                .unwrap()
        });
        assert!(card.left() >= px(8.) && card.right() <= px(width - 7.));
        assert!(
            (card.left() - symbol.left()).abs() <= px(1.)
                || (card.right() - px(width - 8.)).abs() <= px(1.),
            "width {width}: card must align with its symbol or the local right margin"
        );
        assert!(card.bottom() <= symbol.top() || card.top() >= symbol.bottom());
    }
}

/// A modal explorer prompt must prevent pointer movement from reaching editor hover.
#[gpui::test]
fn explorer_edit_mask_blocks_definition_hover(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        theme::apply_theme(theme::builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sample.rs");
    std::fs::write(&path, "alpha beta\n").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| EditorApp::new(workspace, Some(path), window, cx));
        *capture.borrow_mut() = Some(view.clone());
        Root::new(view, window, cx)
    });
    let view = slot.borrow_mut().take().unwrap();
    cx.simulate_resize(size(px(1000.), px(600.)));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let requests = Rc::new(Cell::new(0));
    cx.update(|_, cx| {
        let editor = view.read(cx).editor.clone();
        editor.update(cx, |state, cx| {
            state.lsp_mut().hover_provider = Some(Rc::new(ReadyHover(requests.clone())));
            state.present_hover(
                0..5,
                Hover {
                    contents: HoverContents::Scalar(MarkedString::String("details".into())),
                    range: None,
                },
                cx,
            );
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("editor-definition-card").is_some());
    let detail_text = cx.debug_bounds("editor-definition-text").unwrap();
    let beta = cx.update(|_, cx| {
        let app = view.read(cx);
        app.editor
            .read(cx)
            .range_to_bounds(&(6..10))
            .unwrap()
            .center()
    });
    cx.update(|window, cx| {
        view.update(cx, |app, cx| {
            app.start_explorer_edit(
                crate::explorer::ExplorerEditKind::Directory,
                directory.path().to_path_buf(),
                true,
                window,
                cx,
            );
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.update(|_, cx| view.read(cx).explorer_edit.is_some()));
    assert!(
        cx.debug_bounds("editor-definition-card").is_none(),
        "raised details must not appear above the modal mask"
    );
    // Moving over covered text must not issue a new hover request behind the mask.
    cx.simulate_mouse_move(beta, None::<MouseButton>, Default::default());
    assert_eq!(requests.get(), 0);
    let cursor = cx.update(|_, cx| view.read(cx).editor.read(cx).cursor());
    cx.simulate_click(beta, Default::default());
    assert_eq!(
        cx.update(|_, cx| view.read(cx).editor.read(cx).cursor()),
        cursor,
        "the backdrop must block clicks on editor text"
    );
    let selection_start = point(detail_text.left() + px(3.), detail_text.center().y);
    let selection_end = point(detail_text.right() - px(3.), detail_text.center().y);
    cx.simulate_mouse_down(selection_start, MouseButton::Left, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_mouse_move(selection_end, MouseButton::Left, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_mouse_up(selection_end, MouseButton::Left, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.update(|window, cx| gpui_base::TextSelection::selected_text(window, cx))
            .is_empty(),
        "masked definition text must not be selectable"
    );
    // The backdrop blocks the editor without swallowing controls inside the dialog.
    let cancel = cx.debug_bounds("explorer-edit-cancel").unwrap();
    cx.simulate_click(cancel.center(), Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.update(|_, cx| view.read(cx).explorer_edit.is_none()));
}
