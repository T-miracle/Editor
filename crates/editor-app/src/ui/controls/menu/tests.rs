//! Native popup navigation must skip disabled commands and dismiss once.
use super::*;
use gpui_kit::{AppContext as _, TestAppContext, component::Root, gpui};
use std::cell::RefCell;

/// A title dropdown uses its measured trigger, and follows later dock relocation.
#[gpui::test]
fn popup_menu_follows_nonzero_trigger_bounds(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::ui::typography::init(cx);
        crate::ui::theme::apply_theme(crate::ui::theme::builtin_theme(false), cx);
    });
    let retained = Rc::new(RefCell::new(None));
    let capture = retained.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            PopupMenu::new(
                vec![MenuItem {
                    id: "run".into(),
                    label: "Run".into(),
                    disabled: false,
                    separator_before: false,
                }],
                MenuStyle::current(cx),
                Point::default(),
                |_, _, _| {},
                window,
                cx,
            )
        });
        capture.replace(Some(view.clone()));
        Root::new(view, window, cx)
    });
    cx.simulate_resize(gpui_kit::size(px(1000.), px(700.)));
    let popup = retained.borrow().clone().unwrap();
    for (x, y, width) in [(700., 300., 1000.), (500., 420., 900.)] {
        cx.simulate_resize(gpui_kit::size(px(width), px(700.)));
        cx.update(|window, cx| {
            popup.update(cx, |popup, cx| {
                popup.anchor_to(
                    gpui_kit::Bounds::new(point(px(x), px(y)), gpui_kit::size(px(24.), px(24.))),
                    cx,
                )
            });
            window.draw(cx).clear(cx);
        });
        // A later paint must keep the trigger anchor instead of reverting to the viewport origin.
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let menu = cx.debug_bounds("native-popup-menu").unwrap();
        assert_eq!(menu.right(), px(x + 24.));
        assert_eq!(menu.top(), px(y + 24.));
    }
}
#[gpui::test]
fn popup_menu_keyboard_navigation_and_escape(cx: &mut TestAppContext) {
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
fn popup_menu_pointer_selects_command_without_dismissal(cx: &mut TestAppContext) {
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

/// Long dropdowns retain their lower-edge anchor and a visible footer; scrolling under a stationary
/// pointer must preserve End/Enter's keyboard target rather than silently selecting another row.
#[gpui::test]
fn long_dropdown_keeps_footer_and_keyboard_selection(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::ui::typography::init(cx);
        crate::ui::theme::apply_theme(crate::ui::theme::builtin_theme(false), cx);
    });
    let events = Rc::new(RefCell::new(vec![]));
    let sink = events.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let mut items: Vec<_> = (0..80)
            .map(|index| MenuItem {
                id: format!("row-{index}"),
                label: format!("Configuration {index}"),
                disabled: false,
                separator_before: false,
            })
            .collect();
        items.push(MenuItem {
            id: "edit".into(),
            label: "Edit configurations".into(),
            disabled: false,
            separator_before: true,
        });
        let view = cx.new(|cx| {
            PopupMenu::new(
                items,
                MenuStyle::current(cx),
                point(px(30.), px(40.)),
                move |action, _, _| sink.borrow_mut().push(action),
                window,
                cx,
            )
            .fixed_footer(1)
            .below_anchor()
        });
        Root::new(view, window, cx)
    });
    cx.simulate_resize(gpui_kit::size(px(400.), px(300.)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let menu = cx.debug_bounds("native-popup-menu").unwrap();
    let footer = cx.debug_bounds("native-menu-edit").unwrap();
    assert_eq!(menu.top(), px(40.));
    assert!(
        menu.bottom() <= px(292.) && menu.contains(&footer.center()),
        "menu={menu:?}; footer={footer:?}"
    );
    let pointer = cx.debug_bounds("native-menu-row-0").unwrap().center();
    let movement = gpui_kit::MouseMoveEvent {
        position: pointer,
        pressed_button: None,
        modifiers: Default::default(),
    };
    cx.simulate_event(movement.clone());
    cx.run_until_parked();
    cx.simulate_keystrokes("end");
    cx.run_until_parked();
    cx.simulate_event(movement);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(*events.borrow(), [Action::Select("edit".into())]);
}
