//! Regression coverage for hovering a document after scrolling it past the first viewport.

use super::*;
use editor_core::Workspace;
use gpui_kit::{TestAppContext, component::Root, gpui, size};
use lsp_types::{Hover, HoverContents, MarkedString};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

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

/// The application requests details for a visible row that upstream hover rejects.
#[gpui::test]
fn hover_hit_test_after_scroll(cx: &mut TestAppContext) {
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
        let visible = state.visible_row_range().unwrap();
        assert!(visible.start > 0, "the document must actually be scrolled");
        let (expected, position) = visible
            .clone()
            .find_map(|line| {
                let start = state.text().line_start_offset(line);
                let bounds = state.range_to_bounds(&(start..start + 6))?;
                state
                    .input_bounds()
                    .contains(&bounds.center())
                    .then_some((start, bounds.center()))
            })
            .expect("a scrolled identifier should be visible");
        assert_eq!(
            hovered_symbol_range(&state, position).map(|range| range.start),
            Some(expected)
        );
        (expected, position)
    });
    let requests = Rc::new(Cell::new(0));
    cx.update(|_, cx| {
        let editor = view.read(cx).editor.clone();
        editor.update(cx, |state, _| {
            state.lsp_mut().hover_provider = Some(Rc::new(ReadyHover(requests.clone())));
        });
    });
    // Upstream misses this row; the application retries after a full second.
    cx.simulate_mouse_move(position, None::<MouseButton>, Default::default());
    cx.executor().advance_clock(Duration::from_millis(500));
    cx.run_until_parked();
    assert_eq!(requests.get(), 0);
    cx.executor().advance_clock(Duration::from_millis(500));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        requests.get() > 0,
        "hover should be requested after the delay"
    );
    assert!(cx.debug_bounds("editor-definition-card").is_some());
    cx.update(|_, cx| {
        let editor = view.read(cx).editor.clone();
        let hover = editor.read(cx).hover_popover().cloned().unwrap();
        assert_eq!(hover.symbol_range.start, expected);
    });
}
