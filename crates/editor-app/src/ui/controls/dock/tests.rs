//! Native split dragging and the host's single-panel layout policy.

use super::*;
use gpui_base::Placement;
use gpui_base::dock::{DockPlacement, PaneRef, Panel, PanelEvent, PanelId};
use gpui_kit::{EventEmitter, FocusHandle, Focusable, Render, TestAppContext, gpui};

/// Minimal content exercises real DockArea registration without a plugin worker.
struct TestPanel(FocusHandle);

impl EventEmitter<PanelEvent> for TestPanel {}

impl Focusable for TestPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        // Keep the panel's focus identity stable across split reconciliation.
        self.0.clone()
    }
}

impl Render for TestPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // Only layout structure matters in this regression test.
        Empty
    }
}

impl Panel for TestPanel {
    fn panel_name(&self) -> &'static str {
        // Instances deliberately share a type name; entity IDs identify panels.
        "LocalDockTestPanel"
    }
}

#[gpui::test]
fn panels_stay_in_independent_slots(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::ui::typography::init(cx);
        crate::ui::theme::apply_theme(crate::ui::theme::builtin_theme(false), cx);
    });
    let slot = Rc::new(std::cell::RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let area = LocalDock::new(28.).create_area("independent-panels", None, window, cx);
        *capture.borrow_mut() = Some(area.clone());
        gpui_kit::component::Root::new(area, window, cx)
    });
    let area = slot.borrow_mut().take().unwrap();
    cx.update(|window, cx| {
        area.update(cx, |area, cx| {
            assert!(!area.is_locked(), "panel rearrangement must remain enabled");
            for placement in [
                DockPlacement::Left,
                DockPlacement::Right,
                DockPlacement::Bottom,
                DockPlacement::Center,
            ] {
                let mut ids = Vec::new();
                for _ in 0..3 {
                    let panel = cx.new(|cx| TestPanel(cx.focus_handle()));
                    ids.push(PanelId::from(panel.entity_id()));
                    add_panel_view(
                        area,
                        Arc::new(panel.clone()),
                        placement,
                        Some(px(180.)),
                        window,
                        cx,
                    );
                    // Re-registering an existing panel must leave the layout intact.
                    add_panel_view(area, Arc::new(panel), placement, Some(px(180.)), window, cx);
                }
                let tree = area.layout(placement).unwrap();
                assert_eq!(tree.panels().collect::<Vec<_>>(), ids);
                tree.root().walk(&mut |node| {
                    if let PaneRef::Tabs { panels, .. } = node.kind() {
                        assert_eq!(panels.len(), 1, "panels must never share a tab group");
                    }
                });
                // The center has no dock extent; peripheral regions remain resizable.
                if placement != DockPlacement::Center {
                    area.set_dock_size(placement, px(220.), window, cx);
                    assert_eq!(area.dock_size(placement), Some(px(220.)));
                }
            }
        });
    });
}
/// Real mouse events must move panels across docks without allowing central merges.
fn exercise_native_split_drag(cx: &mut TestAppContext, placement: Placement, immediate: bool) {
    use gpui_kit::{MouseButton, point, size};
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::ui::typography::init(cx);
        crate::ui::theme::apply_theme(crate::ui::theme::builtin_theme(false), cx);
    });
    let slot = Rc::new(std::cell::RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let area = LocalDock::new(28.).create_area("drag-panels", None, window, cx);
        *capture.borrow_mut() = Some(area.clone());
        gpui_kit::component::Root::new(area, window, cx)
    });
    let area = slot.borrow_mut().take().unwrap();
    cx.simulate_resize(size(px(1000.), px(700.)));
    let (source_id, target_id, source_node, target_node) = cx.update(|window, cx| {
        area.update(cx, |area, cx| {
            let source = cx.new(|cx| TestPanel(cx.focus_handle()));
            let target = cx.new(|cx| TestPanel(cx.focus_handle()));
            let source_id = PanelId::from(source.entity_id());
            let target_id = PanelId::from(target.entity_id());
            add_panel_view(
                area,
                Arc::new(source),
                DockPlacement::Left,
                Some(px(240.)),
                window,
                cx,
            );
            add_panel_view(
                area,
                Arc::new(target),
                DockPlacement::Right,
                Some(px(240.)),
                window,
                cx,
            );
            (
                source_id,
                target_id,
                area.layout(DockPlacement::Left)
                    .unwrap()
                    .find_panel_node(source_id)
                    .unwrap(),
                area.layout(DockPlacement::Right)
                    .unwrap()
                    .find_panel_node(target_id)
                    .unwrap(),
            )
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let title = cx
        .debug_bounds(Box::leak(
            format!("local-dock-title-{}", source_node.as_u64()).into_boxed_str(),
        ))
        .unwrap();
    let target = cx
        .debug_bounds(Box::leak(
            format!("local-dock-content-{}", target_node.as_u64()).into_boxed_str(),
        ))
        .unwrap();
    let edge = match placement {
        Placement::Left => point(target.left() + px(10.), target.center().y),
        Placement::Right => point(target.right() - px(10.), target.center().y),
        Placement::Top => point(target.center().x, target.top() + px(10.)),
        Placement::Bottom => point(target.center().x, target.bottom() - px(10.)),
    };
    for destination in [target.center(), edge] {
        // Start a new drag for each attempt; the center attempt must leave the source intact.
        cx.simulate_mouse_down(title.center(), MouseButton::Left, Default::default());
        cx.simulate_mouse_move(
            title.center() + point(px(12.), px(0.)),
            MouseButton::Left,
            Default::default(),
        );
        if immediate {
            // Release in a new zone before another frame can refresh a renderer snapshot.
            let previous = if destination == target.center() {
                edge
            } else {
                target.center()
            };
            cx.simulate_mouse_move(previous, MouseButton::Left, Default::default());
            cx.update(|window, cx| {
                window.draw(cx).clear(cx);
                window.dispatch_event(
                    gpui_kit::PlatformInput::MouseMove(gpui_kit::MouseMoveEvent {
                        position: destination,
                        pressed_button: Some(MouseButton::Left),
                        modifiers: Default::default(),
                    }),
                    cx,
                );
                window.dispatch_event(
                    gpui_kit::PlatformInput::MouseUp(gpui_kit::MouseUpEvent {
                        position: destination,
                        button: MouseButton::Left,
                        modifiers: Default::default(),
                        click_count: 1,
                    }),
                    cx,
                );
            });
        } else {
            cx.simulate_mouse_move(destination, MouseButton::Left, Default::default());
            cx.update(|window, cx| window.draw(cx).clear(cx));
            if destination != target.center() {
                // Native split dragging previews the half-pane before committing the drop.
                let indicator = cx
                    .debug_bounds("local-dock-drop-indicator")
                    .expect("edge drops must retain DockArea's split preview");
                let expected = match placement {
                    Placement::Left | Placement::Right => {
                        size(target.size.width / 2., target.size.height)
                    }
                    Placement::Top | Placement::Bottom => {
                        size(target.size.width, target.size.height / 2.)
                    }
                };
                assert_eq!(indicator.size, expected);
            }
            cx.simulate_mouse_up(destination, MouseButton::Left, Default::default());
        }
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.update(|_, cx| {
            let area = area.read(cx);
            let tree = area.layout(DockPlacement::Right).unwrap();
            if destination == target.center() {
                assert_eq!(tree.panels().collect::<Vec<_>>(), vec![target_id]);
                assert_eq!(
                    area.layout(DockPlacement::Left)
                        .unwrap()
                        .panels()
                        .collect::<Vec<_>>(),
                    vec![source_id]
                );
            } else {
                let expected = match placement {
                    Placement::Left | Placement::Top => vec![source_id, target_id],
                    Placement::Right | Placement::Bottom => vec![target_id, source_id],
                };
                assert_eq!(tree.panels().collect::<Vec<_>>(), expected);
                let PaneRef::Split { axis, .. } = tree.root().kind() else {
                    panic!("edge drops must produce a split, not a tab group");
                };
                assert_eq!(
                    axis,
                    match placement {
                        Placement::Left | Placement::Right => gpui_kit::Axis::Horizontal,
                        Placement::Top | Placement::Bottom => gpui_kit::Axis::Vertical,
                    }
                );
                assert!(
                    area.layout(DockPlacement::Left)
                        .unwrap()
                        .panels()
                        .next()
                        .is_none()
                );
            }
            tree.root().walk(&mut |node| {
                if let PaneRef::Tabs { panels, .. } = node.kind() {
                    assert_eq!(panels.len(), 1);
                }
            });
        });
    }
}
/// Both horizontal drop directions use Base's split geometry and layout edits.
#[gpui::test]
fn native_horizontal_splits_reject_tab_merges(cx: &mut TestAppContext) {
    exercise_native_split_drag(cx, Placement::Left, false);
    exercise_native_split_drag(cx, Placement::Right, false);
}

/// Both vertical drop directions use Base's split geometry and layout edits.
#[gpui::test]
fn native_vertical_splits_reject_tab_merges(cx: &mut TestAppContext) {
    exercise_native_split_drag(cx, Placement::Top, false);
    exercise_native_split_drag(cx, Placement::Bottom, false);
}
/// Drag/drop decisions must use live native hit state even before the next frame.
#[gpui::test]
fn immediate_release_preserves_splits_and_rejects_merges(cx: &mut TestAppContext) {
    exercise_native_split_drag(cx, Placement::Right, true);
    exercise_native_split_drag(cx, Placement::Bottom, true);
}
/// Explicit horizontal and vertical split composition remains native.
#[gpui::test]
fn nested_split_layouts_keep_their_axes(cx: &mut TestAppContext) {
    use gpui_base::dock::DockLayout;
    use gpui_kit::Axis;
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::ui::typography::init(cx);
        crate::ui::theme::apply_theme(crate::ui::theme::builtin_theme(false), cx);
    });
    cx.add_window_view(|window, cx| {
        let area = LocalDock::new(28.).create_area("nested-layout", None, window, cx);
        area.update(cx, |area, cx| {
            let panels = (0..3)
                .map(|_| cx.new(|cx| TestPanel(cx.focus_handle())))
                .collect::<Vec<_>>();
            area.set_center(
                DockLayout::h_split()
                    .child(DockLayout::tabs().panel(panels[0].clone()), Some(px(200.)))
                    .child(
                        DockLayout::v_split()
                            .child(DockLayout::tabs().panel(panels[1].clone()), None)
                            .child(DockLayout::tabs().panel(panels[2].clone()), None),
                        None,
                    ),
                window,
                cx,
            );
            let tree = area.layout(DockPlacement::Center).unwrap();
            let PaneRef::Split { axis, children, .. } = tree.root().kind() else {
                panic!("missing horizontal split")
            };
            assert_eq!(axis, Axis::Horizontal);
            let PaneRef::Split { axis, .. } = children[1].kind() else {
                panic!("missing vertical split")
            };
            assert_eq!(axis, Axis::Vertical);
            assert_eq!(tree.panels().count(), 3);
        });
        gpui_kit::component::Root::new(area, window, cx)
    });
}
