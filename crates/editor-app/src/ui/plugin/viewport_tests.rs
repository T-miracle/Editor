//! Native Base scroll and real layout prove semantic events, receipt ownership and scene withdrawal.
use super::*;
use gpui_kit::{TestAppContext, VisualTestContext, component::Root, gpui, point, px, size};
use plugin_runtime::plugin_protocol::{
    api,
    ui::{Dialog, Node, Tab},
};
use std::cell::RefCell;

/// Fixed-height, uniquely mapped leaves make visible positions independent of font metrics.
fn fixture(revision: u64) -> Document {
    let rows = (0..30)
        .map(|index| {
            Node::text(format!("row-{index}"), format!("段落 {index}"))
                .height(40.)
                .source_range(index * 20..(index + 1) * 20)
        })
        .collect();
    let mut document = Document::new(
        Node::scroll("viewport", Node::column("group", rows).source_range(0..600)).height(120.),
    )
    .revision(revision);
    document.source = Some(api::DocumentVersion {
        id: "source:1".into(),
        path: "notes.md".into(),
        revision: 4,
    });
    document.editor_viewport = Some("viewport".into());
    document
}

/// The fixture uses the same local native view and event sink as an authorized host surface.
fn mount(
    cx: &mut TestAppContext,
    document: Document,
    enabled: bool,
) -> (
    Entity<PluginView>,
    Rc<RefCell<Vec<UiEvent>>>,
    &mut VisualTestContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::ui::typography::init(cx);
        crate::ui::theme::apply_theme(crate::ui::theme::builtin_theme(false), cx);
    });
    let events = Rc::new(RefCell::new(Vec::new()));
    let output = events.clone();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            let mut view = PluginView::new(
                "test".into(),
                document,
                Environment::default(),
                move |event, _| output.borrow_mut().push(event),
                window,
                cx,
            );
            view.set_viewport_enabled(enabled, cx);
            view
        });
        *capture.borrow_mut() = Some(view.clone());
        Root::new(view, window, cx)
    });
    let view = slot.borrow_mut().take().unwrap();
    settle(cx);
    (view, events, cx)
}

/// Program setters wake a following layout; a few explicit draws drain that finite native work in tests.
fn settle(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
}

/// A platform wheel event exercises Base's genuine offset, hover and clipping behavior.
fn wheel(cx: &mut VisualTestContext, pixels: f32) {
    let bounds = cx.debug_bounds("plugin-ui-viewport").unwrap();
    cx.simulate_event(gpui_kit::ScrollWheelEvent {
        position: bounds.center(),
        delta: gpui_kit::ScrollDelta::Pixels(point(px(0.), px(pixels))),
        modifiers: Default::default(),
        touch_phase: gpui_kit::TouchPhase::Moved,
    });
    settle(cx);
}

/// Assert the public emitted position rather than consulting the adapter's internal tracking fields.
fn position(events: &Rc<RefCell<Vec<UiEvent>>>) -> api::PreviewViewport {
    let events = events.borrow();
    assert_eq!(
        events.len(),
        1,
        "one complete scroll frame emits one semantic event"
    );
    assert_eq!(events[0].node, "viewport");
    let Action::Viewport(position) = &events[0].action else {
        panic!("expected viewport event")
    };
    position.clone()
}

/// A default view is inert. Enabled reporting selects a leaf and coalesces all mapped block callbacks.
#[gpui::test]
fn viewport_wheel_reports_one_live_leaf_without_container_feedback(cx: &mut TestAppContext) {
    let (view, events, cx) = mount(cx, fixture(0), false);
    wheel(cx, -90.);
    assert!(events.borrow().is_empty());
    cx.update(|_, cx| view.update(cx, |view, cx| view.set_viewport_enabled(true, cx)));
    settle(cx);
    let initial = position(&events);
    assert_eq!(initial.block, "row-2");
    assert_eq!(initial.source_range.start, 40);
    assert!((initial.fraction - 0.25).abs() < 0.01);
    assert!(initial.layout);
    events.borrow_mut().clear();
    wheel(cx, -45.);
    let manual = position(&events);
    assert_eq!(manual.block, "row-3");
    assert!((manual.fraction - 0.375).abs() < 0.01);
    assert_eq!(manual.origin, None);
    assert!(
        !manual.layout,
        "pure native translation is not source reflow"
    );
    let viewport = cx.debug_bounds("plugin-ui-viewport").unwrap();
    let row = cx.debug_bounds("plugin-ui-row-3").unwrap();
    assert!(row.top() < viewport.top() && row.bottom() > viewport.top());
    events.borrow_mut().clear();
    settle(cx);
    assert!(
        events.borrow().is_empty(),
        "painting an unchanged frame must not feed back"
    );
}

/// A completed locate echoes one origin; the next real preview wheel takes ownership without that origin.
#[gpui::test]
fn viewport_locate_aligns_a_live_block_and_manual_scroll_clears_origin(cx: &mut TestAppContext) {
    let (view, events, cx) = mount(cx, fixture(7), true);
    events.borrow_mut().clear();
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.locate_viewport("row-10", 0.5, 91, 7, cx).unwrap()
        })
    });
    settle(cx);
    let located = position(&events);
    assert_eq!(located.block, "row-10");
    assert_eq!(located.origin, Some(91));
    assert!((located.fraction - 0.5).abs() < 0.01);
    let viewport = cx.debug_bounds("plugin-ui-viewport").unwrap();
    let row = cx.debug_bounds("plugin-ui-row-10").unwrap();
    assert!((row.top() + row.size.height * 0.5 - viewport.top()).abs() < px(0.1));
    events.borrow_mut().clear();
    settle(cx);
    assert!(events.borrow().is_empty());
    wheel(cx, -45.);
    let manual = position(&events);
    assert_eq!(manual.block, "row-11");
    assert_eq!(manual.origin, None);
    assert!(!manual.layout);
    assert!((manual.fraction - 0.625).abs() < 0.01);
}

/// Native input revokes program receipts even when dispatch paints a pending locate before input capture.
#[gpui::test]
fn viewport_manual_pointer_and_keyboard_cancel_queued_native_locations(cx: &mut TestAppContext) {
    let (view, events, cx) = mount(cx, fixture(0), true);
    events.borrow_mut().clear();
    let bounds = cx.debug_bounds("plugin-ui-viewport").unwrap();
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.locate_viewport("row-20", 0., 101, 0, cx).unwrap()
        })
    });
    cx.simulate_event(gpui_kit::MouseDownEvent {
        position: bounds.center(),
        button: gpui_kit::MouseButton::Left,
        ..Default::default()
    });
    // A guest request may reach the view after the pointer press, while Base still owns the drag.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            // The same native scroll owner also survives an ordinary theme/environment projection.
            let mut environment = Environment::default();
            environment.dark = true;
            view.update_document(fixture(0), environment, window, cx);
            let error = view.locate_viewport("row-8", 0., 103, 0, cx).unwrap_err();
            assert_eq!(error.code, api::ErrorCode::Cancelled);
        })
    });
    let after_press = cx.debug_bounds("plugin-ui-row-0").unwrap();
    cx.simulate_event(gpui_kit::MouseMoveEvent {
        position: point(bounds.right() + px(100.), bounds.center().y),
        pressed_button: Some(gpui_kit::MouseButton::Left),
        ..Default::default()
    });
    cx.simulate_event(gpui_kit::MouseUpEvent {
        position: bounds.center(),
        button: gpui_kit::MouseButton::Left,
        ..Default::default()
    });
    settle(cx);
    assert_eq!(
        cx.debug_bounds("plugin-ui-row-0").unwrap(),
        after_press,
        "a late request cannot change the manually owned visible position, even outside the pane"
    );
    assert!(
        events.borrow().iter().all(|event| matches!(&event.action,
        Action::Viewport(position) if position.origin.is_none())),
        "manual press withdraws the program receipt"
    );
    let after_pointer = cx.debug_bounds("plugin-ui-row-0").unwrap();
    events.borrow_mut().clear();
    settle(cx);
    assert_eq!(cx.debug_bounds("plugin-ui-row-0").unwrap(), after_pointer);
    cx.update(|window, cx| {
        window.activate_window();
        view.update(cx, |view, cx| {
            view.view_focus.focus(window, cx);
            view.locate_viewport("row-5", 0., 102, 0, cx).unwrap();
        });
    });
    let keystroke = gpui_kit::Keystroke::parse("pagedown").unwrap();
    cx.simulate_event(gpui_kit::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.simulate_event(gpui_kit::KeyUpEvent { keystroke });
    settle(cx);
    assert!(
        events.borrow().iter().all(|event| matches!(&event.action,
        Action::Viewport(position) if position.origin.is_none())),
        "focused keys withdraw the program receipt"
    );
    let after_key = cx.debug_bounds("plugin-ui-row-0").unwrap();
    events.borrow_mut().clear();
    settle(cx);
    assert_eq!(cx.debug_bounds("plugin-ui-row-0").unwrap(), after_key);
    assert!(
        events.borrow().is_empty(),
        "later paints cannot restore an old program intent"
    );
}

/// Base clamps the bottom position once; even an unreachable target fraction produces a finite receipt.
#[gpui::test]
fn viewport_locate_receipt_uses_the_actual_clamped_visible_block(cx: &mut TestAppContext) {
    let (view, events, cx) = mount(cx, fixture(0), true);
    events.borrow_mut().clear();
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.locate_viewport("row-29", 1., 92, 0, cx).unwrap()
        })
    });
    settle(cx);
    let located = position(&events);
    assert_eq!(located.block, "row-27");
    assert_eq!(located.origin, Some(92));
    assert!(located.fraction < 0.01);
    let viewport = cx.debug_bounds("plugin-ui-viewport").unwrap();
    let row = cx.debug_bounds("plugin-ui-row-29").unwrap();
    assert!((row.bottom() - viewport.bottom()).abs() < px(0.1));
    events.borrow_mut().clear();
    settle(cx);
    assert!(
        events.borrow().is_empty(),
        "a clamp must not retry an impossible locate"
    );
}

/// A replaced source scene cannot execute pending geometry from an earlier UI revision or identity.
#[gpui::test]
fn viewport_replacement_withdraws_pending_locates_and_rejects_stale_requests(
    cx: &mut TestAppContext,
) {
    let (view, events, cx) = mount(cx, fixture(0), true);
    events.borrow_mut().clear();
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.locate_viewport("row-20", 0., 93, 0, cx).unwrap();
            let mut replacement = fixture(1);
            replacement.source.as_mut().unwrap().id = "source:reopened".into();
            view.update_document(replacement, Environment::default(), window, cx);
            assert_eq!(
                view.locate_viewport("row-20", 0., 94, 0, cx)
                    .unwrap_err()
                    .code,
                api::ErrorCode::StaleRevision
            );
        })
    });
    settle(cx);
    let current = position(&events);
    assert_eq!(current.block, "row-0");
    assert_eq!(current.origin, None);
    assert_eq!(events.borrow()[0].revision, 1);
    let viewport = cx.debug_bounds("plugin-ui-viewport").unwrap();
    let row = cx.debug_bounds("plugin-ui-row-0").unwrap();
    assert!((row.top() - viewport.top()).abs() < px(0.1));
}

/// Disabling between paint and defer revokes its queued event and leaves explicit navigation usable.
#[gpui::test]
fn viewport_disabling_withdraws_deferred_events_and_preserves_link_reveal(cx: &mut TestAppContext) {
    let (view, events, cx) = mount(cx, fixture(0), false);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| view.set_viewport_enabled(true, cx));
        window.draw(cx).clear(cx);
        view.update(cx, |view, cx| {
            view.set_viewport_enabled(false, cx);
            assert_eq!(
                view.locate_viewport("row-12", 0., 95, 0, cx)
                    .unwrap_err()
                    .code,
                api::ErrorCode::InvalidState
            );
            view.reveal_node("row-12", 0, cx).unwrap();
        });
    });
    settle(cx);
    assert!(events.borrow().is_empty());
    let viewport = cx.debug_bounds("plugin-ui-viewport").unwrap();
    let row = cx.debug_bounds("plugin-ui-row-12").unwrap();
    assert!((row.top() - viewport.top()).abs() < px(0.1));
}

/// Existing heading navigation reports only its final visible leaf; newer locates supersede queued reveals.
#[gpui::test]
fn viewport_link_reveal_waits_for_current_geometry_and_latest_intent(cx: &mut TestAppContext) {
    let (view, events, cx) = mount(cx, fixture(0), true);
    events.borrow_mut().clear();
    cx.update(|_, cx| view.update(cx, |view, cx| view.reveal_node("row-12", 0, cx).unwrap()));
    settle(cx);
    let revealed = position(&events);
    assert_eq!(revealed.block, "row-12");
    assert_eq!(revealed.origin, None);
    assert!(revealed.fraction < 0.01);
    events.borrow_mut().clear();
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.reveal_node("row-4", 0, cx).unwrap();
            view.locate_viewport("row-18", 0.5, 99, 0, cx).unwrap();
        })
    });
    settle(cx);
    let located = position(&events);
    assert_eq!(located.block, "row-18");
    assert_eq!(located.origin, Some(99));
    assert!((located.fraction - 0.5).abs() < 0.01);
}

/// Selected tab and modal ownership apply before measuring or accepting a semantic locate.
#[gpui::test]
fn viewport_inactive_tabs_and_modals_cannot_borrow_source_ownership(cx: &mut TestAppContext) {
    let mut document = fixture(0);
    document.root = Node::scroll(
        "viewport",
        Node::new(
            "tabs",
            Kind::Tabs {
                tabs: vec![
                    Tab::new(
                        "one",
                        "第一页",
                        Node::text("selected", "活动段落")
                            .height(500.)
                            .source_range(0..20),
                    ),
                    Tab::new(
                        "two",
                        "第二页",
                        Node::text("hidden", "隐藏段落")
                            .height(500.)
                            .source_range(20..40),
                    ),
                ],
                selected: "one".into(),
            },
        ),
    )
    .height(120.);
    let (view, events, cx) = mount(cx, document.clone(), true);
    assert_eq!(position(&events).block, "selected");
    events.borrow_mut().clear();
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            assert_eq!(
                view.locate_viewport("hidden", 0., 96, 0, cx)
                    .unwrap_err()
                    .code,
                api::ErrorCode::InvalidState
            );
            view.locate_viewport("selected", 0.5, 97, 0, cx).unwrap();
            document.revision = 1;
            document.dialog = Some(Dialog::new(
                "modal",
                "弹窗",
                Node::text("dialog-text", "内容"),
            ));
            view.update_document(document, Environment::default(), window, cx);
            assert_eq!(
                view.locate_viewport("selected", 0.5, 98, 1, cx)
                    .unwrap_err()
                    .code,
                api::ErrorCode::InvalidState
            );
        })
    });
    settle(cx);
    assert!(events.borrow().is_empty());
}

/// Width changes remeasure wrapped text without a new source revision and emit a single layout event.
#[gpui::test]
fn viewport_width_reflow_reports_live_wrapped_block_geometry(cx: &mut TestAppContext) {
    let mut document = fixture(0);
    let text = "long wrapped paragraph with visible words ".repeat(80);
    document.root = Node::scroll(
        "viewport",
        Node::column(
            "group",
            vec![
                Node::rich_text("wrapped", format!("<p>{text}</p>")).source_range(0..300),
                Node::text("after", "下一个段落")
                    .height(400.)
                    .source_range(300..600),
            ],
        )
        .source_range(0..600),
    )
    .height(120.);
    let (_, events, cx) = mount(cx, document, true);
    cx.simulate_resize(size(px(700.), px(500.)));
    settle(cx);
    let wide = cx.debug_bounds("plugin-ui-wrapped").unwrap();
    events.borrow_mut().clear();
    cx.simulate_resize(size(px(320.), px(500.)));
    settle(cx);
    let wrapped = cx.debug_bounds("plugin-ui-wrapped").unwrap();
    let reflow = position(&events);
    assert!(
        wrapped.size.height > wide.size.height,
        "the actual native paragraph must wrap after resize"
    );
    assert_eq!(reflow.block, "wrapped");
    assert!(reflow.layout);
    assert_eq!(reflow.origin, None);
}

/// An inner Scroll frame and its mapped content are never anchors of the outer native viewport.
#[gpui::test]
fn viewport_nested_scroll_ranges_cannot_borrow_the_outer_native_owner(cx: &mut TestAppContext) {
    let mut document = fixture(0);
    document.root = Node::scroll(
        "viewport",
        Node::column(
            "content",
            vec![
                Node::text("outer-leaf", "外层内容")
                    .height(200.)
                    .source_range(0..20),
                Node::scroll(
                    "inner",
                    Node::text("inner-leaf", "内层内容")
                        .height(400.)
                        .source_range(20..40),
                )
                .height(100.)
                .source_range(20..40),
            ],
        ),
    )
    .height(120.);
    let (view, events, cx) = mount(cx, document, true);
    assert_eq!(position(&events).block, "outer-leaf");
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            for node in ["inner", "inner-leaf"] {
                assert_eq!(
                    view.locate_viewport(node, 0., 120, 0, cx).unwrap_err().code,
                    api::ErrorCode::InvalidState
                );
            }
        })
    });
}
