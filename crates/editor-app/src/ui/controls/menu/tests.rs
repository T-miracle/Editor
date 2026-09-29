//! Native popup navigation must skip disabled commands and dismiss once.
use super::*;
use gpui_kit::{AppContext as _, TestAppContext, component::Root, gpui};
use std::cell::RefCell;
#[gpui::test]
fn chrome_menu_keyboard_navigation_and_escape(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::ui::typography::init(cx);
        crate::ui::theme::apply_theme(crate::ui::theme::builtin_theme(false), cx);
    });
    let events = Rc::new(RefCell::new(vec![]));
    let sink = events.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let items = [("one", false), ("disabled", true), ("last", false)]
            .into_iter()
            .map(|(id, disabled)| MenuItem {
                id: id.into(),
                label: id.into(),
                disabled,
                separator_before: false,
            })
            .collect();
        let view = cx.new(|cx| {
            PopupMenu::new(
                items,
                MenuStyle::current(cx),
                point(px(30.), px(30.)),
                move |action, _, _| sink.borrow_mut().push(action),
                window,
                cx,
            )
        });
        Root::new(view, window, cx)
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    assert_eq!(*events.borrow(), vec![Action::Select("last".into())]);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(events.borrow().len(), 1);
}
#[gpui::test]
fn chrome_menu_pointer_selects_command_without_dismissal(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::ui::typography::init(cx);
        crate::ui::theme::apply_theme(crate::ui::theme::builtin_theme(false), cx);
    });
    let events = Rc::new(RefCell::new(vec![]));
    let sink = events.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            PopupMenu::new(
                vec![MenuItem {
                    id: "run".into(),
                    label: "运行".into(),
                    disabled: false,
                    separator_before: false,
                }],
                MenuStyle::current(cx),
                point(px(30.), px(30.)),
                move |action, _, _| sink.borrow_mut().push(action),
                window,
                cx,
            )
        });
        Root::new(view, window, cx)
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let bounds = cx.debug_bounds("native-menu-run").unwrap();
    cx.simulate_click(bounds.center(), Default::default());
    cx.run_until_parked();
    assert_eq!(*events.borrow(), vec![Action::Select("run".into())]);
}
