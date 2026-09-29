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
        background: menu.surface,
        border: menu.border,
        active: menu.hover,
        active_foreground: menu.foreground,
        menu,
        close_background: None,
        close_foreground: None,
        rename_background: None,
        rename_foreground: None,
    }
}

#[gpui::test]
fn chrome_sidebar_select_reorder_and_capture_resize(cx: &mut TestAppContext) {
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
    cx.simulate_mouse_up(outside, MouseButton::Left, Default::default());
    cx.run_until_parked();
    assert!(events.borrow().contains(&Action::Resize(220.)));
}

#[gpui::test]
fn chrome_sidebar_preserves_native_rename_during_guest_updates(cx: &mut TestAppContext) {
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
