//! Exercise native pointer capture and keyed rename through the public control lifecycle.
use super::*;
use gpui_kit::{TestAppContext, component::Root, gpui, point};
use plugin_runtime::plugin_protocol::ui::SideTab;
use std::cell::RefCell;

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::ui::typography::init(cx);
        crate::ui::theme::apply_theme(crate::ui::theme::builtin_theme(false), cx);
    });
}
fn model() -> SideTabs {
    SideTabs {
        id: "tabs".into(),
        position: SideTabsPosition::Right,
        items: ["one", "two"]
            .into_iter()
            .map(|id| SideTab {
                id: id.into(),
                label: id.into(),
                status: None,
                closable: true,
                disabled: false,
            })
            .collect(),
        selected: Some("one".into()),
        rename: None,
        width: 180.,
        min_width: 80.,
        max_width: 480.,
    }
}
fn style(cx: &App) -> SideTabsStyle {
    let menu = MenuStyle::current(cx);
    SideTabsStyle {
        background: gpui_kit::rgb(0xf7f8fa).into(),
        border: menu.border,
        active: gpui_kit::rgb(0xffffff).into(),
        active_foreground: menu.foreground,
        active_inner_border: gpui_kit::rgb(0x3574f0).into(),
        menu,
        close_background: None,
        close_foreground: None,
        rename_background: None,
        rename_foreground: None,
    }
}

#[gpui::test]
fn side_tabs_select_reorder_and_capture_resize(cx: &mut TestAppContext) {
    init(cx);
    let events = Rc::new(RefCell::new(vec![]));
    let sink = events.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            SideTabBar::new(
                model(),
                style(cx),
                cx.focus_handle(),
                move |action, _, _| sink.borrow_mut().push(action),
                Rc::new(Cell::new(None)),
                |_| {},
                cx,
            )
        });
        Root::new(view, window, cx)
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let first = cx.debug_bounds("side-tab-one").unwrap().center();
    let second = cx.debug_bounds("side-tab-two").unwrap().center();
    cx.simulate_click(second, Default::default());
    cx.run_until_parked();
    assert!(events.borrow().contains(&Action::Select("two".into())));
    cx.simulate_mouse_down(first, MouseButton::Left, Default::default());
    cx.simulate_mouse_move(second, MouseButton::Left, Default::default());
    cx.simulate_mouse_up(second, MouseButton::Left, Default::default());
    cx.run_until_parked();
    assert!(events.borrow().contains(&Action::Move {
        from: "one".into(),
        to: "two".into()
    }));
    let edge = cx.debug_bounds("side-tabs-resize").unwrap().center();
    cx.simulate_mouse_down(edge, MouseButton::Left, Default::default());
    let outside = edge - point(px(40.), px(0.));
    cx.simulate_mouse_move(outside, MouseButton::Left, Default::default());
    cx.simulate_mouse_move(
        outside - point(px(10.), px(0.)),
        MouseButton::Left,
        Default::default(),
    );
    // A drag previews native geometry; the plugin receives one committed width on release.
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, Action::Resize(_)))
            .count(),
        0
    );
    cx.simulate_mouse_up(outside, MouseButton::Left, Default::default());
    cx.run_until_parked();
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, Action::Resize(_)))
            .count(),
        1
    );
}

#[gpui::test]
fn side_tabs_preserves_native_rename_during_guest_updates(cx: &mut TestAppContext) {
    init(cx);
    let events = Rc::new(RefCell::new(vec![]));
    let sink = events.clone();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            SideTabBar::new(
                model(),
                style(cx),
                cx.focus_handle(),
                move |action, _, _| sink.borrow_mut().push(action),
                Rc::new(Cell::new(None)),
                |_| {},
                cx,
            )
        });
        *capture.borrow_mut() = Some(view.clone());
        Root::new(view, window, cx)
    });
    let view = slot.borrow_mut().take().unwrap();
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let mut next = model();
            next.rename = Some("one".into());
            view.update(next, style(cx), window, cx);
        });
        window.draw(cx).clear(cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let input = view.read(cx).editing.as_ref().unwrap().state.clone();
        input.update(cx, |input, cx| input.replace_all("中文会话", window, cx));
        let mut next = model();
        next.rename = Some("one".into());
        next.items[1].label = "后台输出更新".into();
        view.update(cx, |view, cx| view.update(next, style(cx), window, cx));
        assert_eq!(input.read(cx).value().as_str(), "中文会话");
        window.draw(cx).clear(cx);
    });
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|e| matches!(e, Action::Rename { .. }))
            .count(),
        1
    );
    assert!(events.borrow().contains(&Action::Rename {
        id: "one".into(),
        value: "中文会话".into()
    }));
}

/// Left placement mirrors the divider hit target and drag direction without changing tab IDs.
#[gpui::test]
fn left_side_tabs_resize_and_reorder_use_the_inner_edge(cx: &mut TestAppContext) {
    init(cx);
    let events = Rc::new(RefCell::new(vec![]));
    let sink = events.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            let mut tabs = model();
            tabs.position = SideTabsPosition::Left;
            SideTabBar::new(
                tabs,
                style(cx),
                cx.focus_handle(),
                move |action, _, _| sink.borrow_mut().push(action),
                Rc::new(Cell::new(None)),
                |_| {},
                cx,
            )
        });
        Root::new(view, window, cx)
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let first = cx.debug_bounds("side-tab-one").unwrap().center();
    let second = cx.debug_bounds("side-tab-two").unwrap().center();
    cx.simulate_mouse_down(second, MouseButton::Left, Default::default());
    cx.simulate_mouse_move(first, MouseButton::Left, Default::default());
    cx.simulate_mouse_up(first, MouseButton::Left, Default::default());
    cx.run_until_parked();
    assert!(events.borrow().contains(&Action::Move {
        from: "two".into(),
        to: "one".into()
    }));
    let bar = cx.debug_bounds("native-side-tabs").unwrap();
    let handle = cx.debug_bounds("side-tabs-resize").unwrap();
    assert_eq!(handle.right(), bar.right());
    let start = handle.center();
    let grown = start + point(px(40.), px(0.));
    cx.simulate_mouse_down(start, MouseButton::Left, Default::default());
    cx.simulate_mouse_move(grown, MouseButton::Left, Default::default());
    assert!(
        !events
            .borrow()
            .iter()
            .any(|event| matches!(event, Action::Resize(_)))
    );
    cx.simulate_mouse_up(grown, MouseButton::Left, Default::default());
    cx.run_until_parked();
    assert!(events.borrow().contains(&Action::Resize(220.)));
}

/// Visible paint, rather than submitted quads alone, guards shared separators and inner accents.
#[gpui::test]
fn side_tabs_paint_complete_borders_without_hover_changes(cx: &mut TestAppContext) {
    init(cx);
    for position in [SideTabsPosition::Right, SideTabsPosition::Left] {
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let view = cx.new(|cx| {
                let mut tabs = model();
                tabs.position = position;
                SideTabBar::new(
                    tabs,
                    style(cx),
                    cx.focus_handle(),
                    |_, _, _| {},
                    Rc::new(Cell::new(None)),
                    |_| {},
                    cx,
                )
            });
            Root::new(view, window, cx)
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let row = cx.debug_bounds("side-tab-one").unwrap();
        cx.update(|window, _| {
            let scale = window.scale_factor();
            let quads = window.painted_quads();
            let border = quads
                .iter()
                .find(|quad| {
                    quad.bounds.origin.x.0 / scale == row.left() / px(1.)
                        && quad.bounds.origin.y.0 / scale == row.top() / px(1.)
                        && quad.border_widths.left.0 > 0.
                })
                .expect("the selected row keeps its outer and bottom borders");
            // The dock title or previous row already supplies the shared top separator.
            assert_eq!(border.border_widths.top.0, 0.);
            for width in [
                border.border_widths.left,
                border.border_widths.right,
                border.border_widths.bottom,
            ] {
                assert_eq!(width.0 / scale, 1.);
            }
            let accent = quads
                .iter()
                .find(|quad| quad.background == gpui_kit::rgb(0x3574f0).into())
                .expect("the selected row paints its inner accent");
            // A submitted accent can still disappear when the scroll container clips its border.
            let visible = accent.bounds.intersect(&accent.content_mask.bounds);
            assert_eq!(visible.size.width.0 / scale, 1.);
            assert_eq!(accent.bounds.size.width.0 / scale, 1.);
            assert_eq!(
                accent.bounds.size.height.0 / scale,
                row.size.height / px(1.)
            );
            let edge = match position {
                SideTabsPosition::Left => row.right() - px(1.),
                SideTabsPosition::Right => row.left(),
            };
            assert_eq!(accent.bounds.left().0 / scale, edge / px(1.));
        });
        let painted = |window: &mut Window| {
            window
                .painted_quads()
                .into_iter()
                .map(|quad| {
                    (
                        quad.bounds,
                        quad.background,
                        quad.border_color,
                        quad.border_widths,
                    )
                })
                .collect::<Vec<_>>()
        };
        let before = cx.update(|window, _| painted(window));
        // Both the inactive tab and the close button must retain their normal paint on hover.
        let second = cx.debug_bounds("side-tab-two").unwrap();
        for point in [
            second.center(),
            point(second.right() - px(14.), second.center().y),
        ] {
            cx.simulate_mouse_move(point, None, Default::default());
            cx.update(|window, cx| window.draw(cx).clear(cx));
            assert_eq!(cx.update(|window, _| painted(window)), before);
        }
    }
}
