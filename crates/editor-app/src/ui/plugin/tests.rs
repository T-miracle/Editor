//! Native interaction tests cover event routing, focus-preserving reconciliation and modality.
use super::*;
use gpui_kit::Styled as _;
use gpui_kit::{Focusable as _, TestAppContext, component::Root, gpui};
use plugin_runtime::plugin_protocol::ui::{Dialog, Input, Node};
use std::cell::RefCell;

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::ui::typography::init(cx);
        crate::ui::theme::apply_theme(crate::ui::theme::builtin_theme(false), cx);
    });
}

fn fixture() -> Document {
    Document::new(
        Node::column(
            "root",
            vec![
                Node::button("run", "运行"),
                Node::button("disabled", "不可用").disabled(true),
                Node::checkbox("check", "启用", false),
                Node::input(
                    "name",
                    Input {
                        value: "初始".into(),
                        ..Default::default()
                    },
                ),
            ],
        )
        .gap(12.)
        .padding(12.),
    )
    .revision(4)
}

#[gpui::test]
fn native_clicks_emit_typed_events_and_disabled_controls_are_inert(cx: &mut TestAppContext) {
    init(cx);
    let events = Rc::new(RefCell::new(vec![]));
    let output = events.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            PluginView::new(
                "test".into(),
                fixture(),
                Environment::default(),
                move |event, _| output.borrow_mut().push(event),
                window,
                cx,
            )
        });
        Root::new(view, window, cx)
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    for name in ["plugin-ui-run", "plugin-ui-disabled", "plugin-ui-check"] {
        let bounds = cx.debug_bounds(name).unwrap();
        // Wrapper spans the column, but the native control starts at its left edge.
        cx.simulate_click(
            gpui_kit::point(bounds.left() + gpui_kit::px(15.), bounds.center().y),
            Default::default(),
        );
        cx.run_until_parked();
    }
    let events = events.borrow();
    assert!(
        events
            .iter()
            .any(|e| e.node == "run" && e.revision == 4 && e.action == Action::Click)
    );
    assert!(
        events
            .iter()
            .any(|e| e.node == "check" && e.action == Action::Toggle(true))
    );
    assert!(!events.iter().any(|e| e.node == "disabled"));
}

#[gpui::test]
fn native_input_survives_stale_echo_and_explicit_reset_does_not_echo(cx: &mut TestAppContext) {
    init(cx);
    let events = Rc::new(RefCell::new(vec![]));
    let output = events.clone();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            PluginView::new(
                "test".into(),
                fixture(),
                Environment::default(),
                move |event, _| output.borrow_mut().push(event),
                window,
                cx,
            )
        });
        *capture.borrow_mut() = Some(view.clone());
        Root::new(view, window, cx)
    });
    let view = slot.borrow_mut().take().unwrap();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let input = cx.update(|window, cx| {
        let input = view.read(cx).inputs["name"].state.clone();
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.replace_all("中文编辑中", window, cx);
        });
        input
    });
    cx.run_until_parked();
    assert!(
        events
            .borrow()
            .iter()
            .any(|e| e.action == Action::Change("中文编辑中".into()))
    );
    events.borrow_mut().clear();
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.update_document(fixture().revision(5), Environment::default(), window, cx)
        });
        assert_eq!(input.read(cx).value().as_str(), "中文编辑中");
        assert_eq!(
            view.read(cx).inputs["name"].state.entity_id(),
            input.entity_id()
        );
        assert!(input.focus_handle(cx).is_focused(window));
        let reset = Document::new(Node::input(
            "name",
            Input {
                value: "重置".into(),
                value_revision: 1,
                ..Default::default()
            },
        ));
        view.update(cx, |view, cx| {
            view.update_document(reset, Environment::default(), window, cx)
        });
        assert_eq!(input.read(cx).value().as_str(), "重置");
    });
    cx.run_until_parked();
    assert!(
        events.borrow().is_empty(),
        "programmatic resets must not masquerade as user edits"
    );
}

#[gpui::test]
fn modal_escape_emits_once_and_background_events_are_blocked(cx: &mut TestAppContext) {
    init(cx);
    let events = Rc::new(RefCell::new(vec![]));
    let output = events.clone();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let document =
            fixture().dialog(Dialog::new("modal", "测试弹窗", Node::button("ok", "确定")));
        let view = cx.new(|cx| {
            PluginView::new(
                "test".into(),
                document,
                Environment::default(),
                move |event, _| output.borrow_mut().push(event),
                window,
                cx,
            )
        });
        *capture.borrow_mut() = Some(view.clone());
        Root::new(view, window, cx)
    });
    let view = slot.borrow_mut().take().unwrap();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|_, cx| view.update(cx, |view, cx| view.emit("run", Action::Click, cx)));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(
        *events.borrow(),
        vec![UiEvent {
            revision: 4,
            node: "modal".into(),
            action: Action::Dismiss
        }]
    );
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.update_document(fixture().revision(6), Environment::default(), window, cx)
        })
    });
    cx.update(|_, cx| view.update(cx, |view, cx| view.emit("run", Action::Click, cx)));
    assert_eq!(events.borrow().last().unwrap().action, Action::Click);
}

#[gpui::test]
fn live_theme_roles_override_native_control_colors_and_fonts(cx: &mut TestAppContext) {
    init(cx);
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let mut environment = Environment::default();
        environment
            .theme_colors
            .insert("test.ui.button.foreground".into(), 0x123456);
        environment.ui_font.family = Some("Segoe UI".into());
        environment.theme_text_styles.insert(
            "test.button".into(),
            plugin_runtime::plugin_protocol::FontStyle {
                size_px: Some(20.),
                bold: Some(true),
                ..Default::default()
            },
        );
        let view = cx.new(|cx| {
            PluginView::new("test".into(), fixture(), environment, |_, _| {}, window, cx)
        });
        assert_eq!(
            view.read(cx).colors("button", cx).foreground,
            gpui_kit::rgb(0x123456).into()
        );
        let mut styled = view.read(cx).font(gpui_kit::div(), "button");
        assert_eq!(styled.style().text.font_family.as_deref(), Some("Segoe UI"));
        assert_eq!(
            styled.style().text.font_size,
            Some(gpui_kit::px(20.).into())
        );
        assert_eq!(
            styled.style().text.font_weight,
            Some(gpui_kit::FontWeight::BOLD)
        );
        view.update(cx, |view, cx| {
            view.update_document(fixture(), Environment::default(), window, cx)
        });
        assert_ne!(
            view.read(cx).colors("button", cx).foreground,
            gpui_kit::rgb(0x123456).into()
        );
        Root::new(view, window, cx)
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn growing_children_fill_constrained_layout_and_scrolling_has_a_viewport(cx: &mut TestAppContext) {
    init(cx);
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let document = Document::new(
            Node::column(
                "root",
                vec![
                    Node::text("top", "顶部").height(40.),
                    Node::new("space", Kind::Spacer).grow(),
                    Node::scroll(
                        "scroll",
                        Node::column(
                            "rows",
                            (0..20)
                                .map(|i| Node::text(format!("row-{i}"), "项目").height(20.))
                                .collect(),
                        ),
                    )
                    .height(60.),
                ],
            )
            .height(240.),
        );
        let view = cx.new(|cx| {
            PluginView::new(
                "test".into(),
                document,
                Environment::default(),
                |_, _| {},
                window,
                cx,
            )
        });
        Root::new(view, window, cx)
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(
        cx.debug_bounds("plugin-ui-space").unwrap().size.height,
        gpui_kit::px(140.)
    );
    assert_eq!(
        cx.debug_bounds("plugin-ui-scroll").unwrap().size.height,
        gpui_kit::px(60.)
    );
}

#[gpui::test]
fn closing_modal_restores_input_focus(cx: &mut TestAppContext) {
    init(cx);
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            PluginView::new(
                "test".into(),
                fixture(),
                Environment::default(),
                |_, _| {},
                window,
                cx,
            )
        });
        *capture.borrow_mut() = Some(view.clone());
        Root::new(view, window, cx)
    });
    let view = slot.borrow_mut().take().unwrap();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| {
        let input = view.read(cx).inputs["name"].state.clone();
        input.focus_handle(cx).focus(window, cx);
        view.update(cx, |view, cx| {
            view.update_document(
                fixture().dialog(Dialog::new("modal", "弹窗", Node::text("body", "内容"))),
                Environment::default(),
                window,
                cx,
            )
        });
        window.draw(cx).clear(cx);
        assert!(!input.focus_handle(cx).is_focused(window));
        view.update(cx, |view, cx| {
            view.update_document(fixture(), Environment::default(), window, cx)
        });
        window.draw(cx).clear(cx);
        assert!(input.focus_handle(cx).is_focused(window));
    });
}
