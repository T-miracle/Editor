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

/// A drawing provider uses the same public visual input as an image provider, without an ID branch.
#[gpui::test]
fn visual_viewport_canvas_emits_shared_input(cx: &mut TestAppContext) {
    use gpui_kit::{point, px, size};
    init(cx);
    let events = Rc::new(RefCell::new(Vec::new()));
    let output = events.clone();
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let mut node = Node::new("drawing", Kind::Canvas(ui::Canvas::default())).grow();
        node.viewport = Some(ui::VisualViewport {
            content: Some(ui::ContentSize {
                width: 100.,
                height: 50.,
            }),
            transform: Some(ui::ContentTransform {
                scale: 2.4,
                ..Default::default()
            }),
        });
        let document = Document::new(node).revision(7);
        document.validate().unwrap();
        let view = cx.new(|cx| {
            PluginView::new(
                "diagram-provider".into(),
                document,
                Environment::default(),
                move |event, _| output.borrow_mut().push(event),
                window,
                cx,
            )
        });
        Root::new(view, window, cx)
    });
    visual.simulate_resize(size(px(400.), px(300.)));
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(events.borrow().iter().any(
        |event| matches!(&event.action, Action::ViewportInput(input)
        if event.revision == 7 && input.content == (ui::ContentSize {width: 100., height: 50.})
            && matches!(input.event, ui::CanvasEvent::Resize {width: 400., height: 300., ..}))
    ));
    visual.simulate_event(gpui_kit::ScrollWheelEvent {
        position: point(px(200.), px(150.)),
        delta: gpui_kit::ScrollDelta::Pixels(point(px(0.), px(14.))),
        ..Default::default()
    });
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        events
            .borrow()
            .iter()
            .any(|event| matches!(&event.action, Action::ViewportInput(input)
        if matches!(input.event, ui::CanvasEvent::Wheel {delta_y: 14., ..})))
    );
    assert!(
        events
            .borrow()
            .iter()
            .all(|event| !matches!(event.action, Action::Canvas(_)))
    );
}

/// A pointer gesture belongs to the scene pressed, even when a replacement reuses its node ID.
#[gpui::test]
fn checkbox_press_cannot_activate_a_replacement_scene(cx: &mut TestAppContext) {
    use gpui_kit::MouseButton;
    init(cx);
    let events = Rc::new(RefCell::new(Vec::new()));
    let output = events.clone();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let document = Document::new(Node::new(
        "task",
        Kind::Checkbox {
            label: "旧任务".into(),
            checked: false,
        },
    ));
    let replacement = Document::new(Node::new(
        "task",
        Kind::Checkbox {
            label: "新任务".into(),
            checked: false,
        },
    ))
    .revision(1);
    let (_, cx) = cx.add_window_view(move |window, cx| {
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
    let position = cx
        .debug_bounds("plugin-checkbox-marker-task")
        .unwrap()
        .center();
    cx.simulate_mouse_down(position, MouseButton::Left, Default::default());
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.update_document(replacement, Environment::default(), window, cx);
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_mouse_up(position, MouseButton::Left, Default::default());
    cx.run_until_parked();
    assert!(
        events.borrow().is_empty(),
        "a stale press must not become a new scene's Toggle"
    );
    cx.simulate_click(position, Default::default());
    cx.run_until_parked();
    assert_eq!(events.borrow().len(), 1, "a fresh gesture remains usable");
    assert_eq!(events.borrow()[0].revision, 1);
}

/// Reopening a native popup preserves canvas layout and allows one dismissal on every opening.
#[gpui::test]
fn popup_reopens_without_resizing_canvas_or_losing_escape(cx: &mut TestAppContext) {
    use plugin_runtime::plugin_protocol::ui::{Canvas, MenuItem, PopupMenu};
    init(cx);
    let events = Rc::new(RefCell::new(Vec::new()));
    let output = events.clone();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let document = Document::new(
        Node::new(
            "drawing",
            Kind::Canvas(Canvas {
                focusable: true,
                ..Default::default()
            }),
        )
        .grow(),
    );
    let initial = document.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            PluginView::new(
                "test".into(),
                initial,
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
    cx.run_until_parked();
    let bounds = cx.debug_bounds("plugin-ui-drawing").unwrap();
    for revision in [1, 3] {
        let mut opened = document.clone().revision(revision);
        opened.menu = Some(PopupMenu {
            id: "popup".into(),
            x: 20.,
            y: 20.,
            items: vec![MenuItem {
                id: "item".into(),
                label: "Action".into(),
                disabled: false,
                separator_before: false,
            }],
        });
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.update_document(opened, Environment::default(), window, cx)
            })
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        assert_eq!(cx.debug_bounds("plugin-ui-drawing").unwrap(), bounds);
        events.borrow_mut().clear();
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert_eq!(
            events
                .borrow()
                .iter()
                .filter(|event| event.node == "popup" && matches!(event.action, Action::Dismiss))
                .count(),
            1
        );
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.update_document(
                    document.clone().revision(revision + 1),
                    Environment::default(),
                    window,
                    cx,
                )
            })
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
}

/// Native collections occupy their own tree node and cannot steal the adjacent canvas's geometry.
#[gpui::test]
fn side_tabs_compose_with_canvas_and_emit_native_resize(cx: &mut TestAppContext) {
    use gpui_kit::{MouseButton, point, px};
    use plugin_runtime::plugin_protocol::ui::{Canvas, SideTab, SideTabs, SideTabsPosition};
    init(cx);
    let events = Rc::new(RefCell::new(Vec::new()));
    let output = events.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let tabs = SideTabs {
            id: "sessions".into(),
            position: SideTabsPosition::Left,
            width: 180.,
            min_width: 112.,
            max_width: 480.,
            selected: Some("first".into()),
            rename: None,
            items: ["first", "second"]
                .into_iter()
                .map(|id| SideTab {
                    id: id.into(),
                    label: id.into(),
                    status: None,
                    disabled: false,
                    closable: true,
                })
                .collect(),
        };
        let document = Document::new(
            Node::row(
                "row",
                vec![
                    Node::new("sessions", Kind::SideTabs(tabs)).width(180.),
                    Node::new(
                        "drawing",
                        Kind::Canvas(Canvas {
                            focusable: true,
                            grid: true,
                            ..Default::default()
                        }),
                    )
                    .grow(),
                ],
            )
            .grow(),
        );
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
        Root::new(view, window, cx)
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    let tabs = cx.debug_bounds("native-side-tabs").unwrap();
    let canvas = cx.debug_bounds("plugin-ui-drawing").unwrap();
    assert_eq!(tabs.size.width, px(180.));
    assert!(canvas.left() >= tabs.right());
    assert_eq!(canvas.size.height, tabs.size.height);
    // A grid line is its measured cell height, so one-notch wheels cannot truncate to zero.
    let height = events
        .borrow()
        .iter()
        .find_map(|event| match &event.action {
            Action::Canvas(plugin_runtime::plugin_protocol::ui::CanvasEvent::Resize {
                grid: Some(grid),
                ..
            }) => Some(grid.cell_height),
            _ => None,
        })
        .unwrap();
    for lines in [1., -1.] {
        cx.update(|window, cx| {
            window.dispatch_event(
                gpui_kit::PlatformInput::ScrollWheel(gpui_kit::ScrollWheelEvent {
                    position: canvas.center(),
                    delta: gpui_kit::ScrollDelta::Lines(point(0., lines)),
                    ..Default::default()
                }),
                cx,
            );
        });
        cx.run_until_parked();
        assert!(events.borrow().iter().any(|event|matches!(event.action,Action::Canvas(plugin_runtime::plugin_protocol::ui::CanvasEvent::Wheel{delta_y,..}) if delta_y==height*lines)));
    }
    let second = cx.debug_bounds("side-tab-second").unwrap().center();
    cx.simulate_click(second, Default::default());
    cx.run_until_parked();
    assert!(events.borrow().iter().any(|event| event.node == "sessions"
        && matches!(&event.action,Action::Select(id) if id=="second")));
    let divider = cx.debug_bounds("side-tabs-resize").unwrap().center();
    cx.simulate_mouse_down(divider, MouseButton::Left, Default::default());
    cx.simulate_mouse_move(
        divider + point(px(50.), px(0.)),
        Some(MouseButton::Left),
        Default::default(),
    );
    cx.simulate_mouse_up(
        divider + point(px(50.), px(0.)),
        MouseButton::Left,
        Default::default(),
    );
    cx.run_until_parked();
    assert!(events.borrow().iter().any(|event| event.node == "sessions"
        && matches!(event.action,Action::Resize(width) if width>180.)));
}

/// A keyed disabled canvas must receive its first grid measurement when activated at the same size.
#[gpui::test]
fn enabling_canvas_delivers_current_dimensions_without_a_resize(cx: &mut TestAppContext) {
    use plugin_runtime::plugin_protocol::ui::{Canvas, CanvasEvent};
    init(cx);
    let events = Rc::new(RefCell::new(vec![]));
    let output = events.clone();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let document = Document::new(
        Node::new(
            "canvas",
            Kind::Canvas(Canvas {
                grid: true,
                ..Default::default()
            }),
        )
        .grow(),
    );
    let mut disabled = document.clone();
    disabled.root.disabled = true;
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            PluginView::new(
                "test".into(),
                disabled,
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
    cx.run_until_parked();
    assert!(events.borrow().is_empty());
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.update_document(document.revision(1), Environment::default(), window, cx)
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    assert!(events.borrow().iter().any(|event|matches!(event.action, Action::Canvas(CanvasEvent::Resize {width,height,grid:Some(_),..}) if width>0. && height>0.)));
}

/// A drag belongs to the node where it began, including release outside that node's hit box.
#[gpui::test]
fn canvas_pointer_capture_keeps_foreign_drags_out_and_delivers_external_release(
    cx: &mut TestAppContext,
) {
    use gpui_kit::{MouseButton, point, px};
    use plugin_runtime::plugin_protocol::ui::{Canvas, CanvasEvent, PointerPhase};
    init(cx);
    let events = Rc::new(RefCell::new(vec![]));
    let output = events.clone();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let document = Document::new(Node::column(
            "root",
            vec![
                Node::input("input", Input::default()).height(40.),
                Node::new("canvas", Kind::Canvas(Canvas::default())).height(200.),
            ],
        ));
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
    cx.run_until_parked();
    let canvas = cx.debug_bounds("plugin-ui-canvas").unwrap().center();
    let input = cx.debug_bounds("plugin-ui-input").unwrap();
    let outside = point(input.left() + px(15.), input.center().y);
    events.borrow_mut().clear();
    cx.simulate_mouse_down(outside, MouseButton::Left, Default::default());
    cx.simulate_mouse_move(canvas, Some(MouseButton::Left), Default::default());
    cx.simulate_mouse_up(canvas, MouseButton::Left, Default::default());
    cx.run_until_parked();
    assert!(
        !events
            .borrow()
            .iter()
            .any(|event| matches!(event.action, Action::Canvas(CanvasEvent::Pointer { .. })))
    );
    for (button, index) in [
        (MouseButton::Left, 0),
        (MouseButton::Middle, 1),
        (MouseButton::Right, 2),
    ] {
        events.borrow_mut().clear();
        cx.simulate_mouse_down(canvas, button, Default::default());
        cx.run_until_parked();
        // A non-keyboard image canvas still owns its pointer gesture across guest repaints.
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                let mut document = view.document.clone();
                document.revision += 1;
                view.update_document(document, Environment::default(), window, cx);
            })
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.simulate_mouse_move(outside, Some(button), Default::default());
        cx.simulate_mouse_up(outside, button, Default::default());
        cx.run_until_parked();
        let phases: Vec<_> = events
            .borrow()
            .iter()
            .filter_map(|event| match &event.action {
                Action::Canvas(CanvasEvent::Pointer { phase, button, .. }) if *button == index => {
                    Some(phase.clone())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            phases,
            [PointerPhase::Down, PointerPhase::Move, PointerPhase::Up]
        );
    }
}

/// IME composition stays local until commit and uses UTF-16 selections without corrupting emoji.
#[gpui::test]
fn canvas_ime_preserves_marked_text_across_theme_changes_and_commits_once(cx: &mut TestAppContext) {
    use gpui_kit::EntityInputHandler;
    use plugin_runtime::plugin_protocol::ui::{Canvas, CanvasEvent};
    init(cx);
    let events = Rc::new(RefCell::new(vec![]));
    let output = events.clone();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let document = Document::new(
        Node::new(
            "drawing",
            Kind::Canvas(Canvas {
                focusable: true,
                ..Default::default()
            }),
        )
        .grow(),
    )
    .revision(8);
    let initial = document.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            PluginView::new(
                "test".into(),
                initial,
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
    let drawing = cx.update(|_, cx| view.read(cx).canvases["drawing"].clone());
    cx.update(|window, cx| {
        drawing.update(cx, |drawing, cx| {
            drawing.replace_and_mark_text_in_range(None, "中😀文", Some(1..3), window, cx);
            assert_eq!(drawing.marked_text_range(window, cx), Some(0..4));
            assert_eq!(
                drawing
                    .selected_text_range(false, window, cx)
                    .unwrap()
                    .range,
                1..3
            );
            let mut adjusted = None;
            assert_eq!(
                drawing.text_for_range(2..3, &mut adjusted, window, cx),
                Some("😀".into())
            );
            assert_eq!(adjusted, Some(1..3));
        })
    });
    cx.run_until_parked();
    assert!(
        !events
            .borrow()
            .iter()
            .any(|event| matches!(event.action, Action::Canvas(CanvasEvent::Text { .. })))
    );
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.update_document(
                document,
                Environment {
                    foreground: 0xf0f0f0,
                    ui_font: plugin_runtime::plugin_protocol::FontStyle {
                        size_px: Some(20.),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        assert_eq!(
            view.read(cx).canvases["drawing"].entity_id(),
            drawing.entity_id()
        );
        drawing.update(cx, |drawing, cx| {
            assert_eq!(drawing.marked_text_range(window, cx), Some(0..4));
            assert_eq!(drawing.font.size_px, Some(20.));
            drawing.replace_text_in_range(Some(1..3), "空格 ", window, cx);
            assert_eq!(drawing.marked_text_range(window, cx), None);
        });
    });
    cx.run_until_parked();
    let text: Vec<_> = events
        .borrow()
        .iter()
        .filter_map(|event| match &event.action {
            Action::Canvas(CanvasEvent::Text { text }) => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(text, ["中空格 文"]);
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
        let mut document = fixture();
        document
            .content_colors
            .insert("button.foreground".into(), 0x987654);
        let view = cx
            .new(|cx| PluginView::new("test".into(), document, environment, |_, _| {}, window, cx));
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
            let mut document = fixture();
            document
                .content_colors
                .insert("button.foreground".into(), 0x987654);
            view.update_document(document, Environment::default(), window, cx)
        });
        // Removing a user override restores this document's owned default, not another plugin's token.
        assert_eq!(
            view.read(cx).colors("button", cx).foreground,
            gpui_kit::rgb(0x987654).into()
        );
        Root::new(view, window, cx)
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Plugin-provided RGB defaults control arbitrary roles; native chrome retains transparent tool buttons.
#[gpui::test]
fn github_document_and_transparent_toolbar_roles_render_in_both_themes(cx: &mut TestAppContext) {
    init(cx);
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, ui) = cx.add_window_view(move |window, cx| {
        let document = Document::new(Node::rich_text("content",
            "<p>正文 <code>inline code 中文</code> <a href=\"https://example.com\">链接</a></p><table><tr><td><code>table code</code></td></tr></table>")
            .role("github"));
        let view = cx.new(|cx| PluginView::new("neutral-document".into(), document,
            Environment::default(), |_, _| {}, window, cx));
        *capture.borrow_mut() = Some(view.clone());
        Root::new(view, window, cx)
    });
    let view = slot.borrow_mut().take().unwrap();
    for (dark, foreground, background, link, code) in [
        (false, 0x1f2328, 0xffffff, 0x0969da, 0xf6f8fa),
        (true, 0xf0f6fc, 0x0d1117, 0x4493f8, 0x151b23),
    ] {
        ui.update(|window, cx| {
            crate::ui::theme::apply_theme(crate::ui::theme::builtin_theme(dark), cx);
            view.update(cx, |view, _| {
                // The package supplies domain colors; an arbitrary role has no host preset.
                for (key, value) in [
                    ("foreground", foreground),
                    ("background", background),
                    ("accent", link),
                    ("code_background", code),
                    (
                        "inline_code_background",
                        if dark { 0x1f232a } else { 0xf0f1f2 },
                    ),
                ] {
                    view.document
                        .content_colors
                        .insert(format!("github.{key}"), value);
                }
            });
            let colors = view.read(cx).colors("github", cx);
            assert_eq!(colors.foreground, gpui_kit::rgb(foreground).into());
            assert_eq!(colors.background, gpui_kit::rgb(background).into());
            assert_eq!(colors.accent, gpui_kit::rgb(link).into());
            assert_eq!(colors.code_background, gpui_kit::rgb(code).into());
            assert_ne!(colors.inline_code_background, colors.code_background);
            assert_eq!(colors.inline_code_background.a, 1.);
            assert_eq!(view.read(cx).colors("toolbar_button", cx).background.a, 0.);
            window.draw(cx).clear(cx);
        });
        assert!(ui.debug_bounds("plugin-ui-content").unwrap().size.height > gpui_kit::px(30.));
    }
}

/// A custom code role changes native glyph metrics, rather than only its surrounding container.
#[gpui::test]
fn code_block_custom_role_changes_native_line_height(cx: &mut TestAppContext) {
    init(cx);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let mut environment = Environment::default();
        environment.theme_text_styles.insert(
            "test.large_code".into(),
            plugin_runtime::plugin_protocol::FontStyle {
                size_px: Some(30.),
                ..Default::default()
            },
        );
        let document = Document::new(Node::column(
            "code-roles",
            vec![
                Node::code_block("ordinary-code", "let x = 1;", None),
                Node::code_block("custom-code", "let x = 1;", None).role("large_code"),
            ],
        ));
        let view = cx
            .new(|cx| PluginView::new("test".into(), document, environment, |_, _| {}, window, cx));
        Root::new(view, window, cx)
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let ordinary = cx.debug_bounds("plugin-ui-ordinary-code").unwrap();
    let custom = cx.debug_bounds("plugin-ui-custom-code").unwrap();
    assert!(custom.size.height > ordinary.size.height + gpui_kit::px(10.));
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
