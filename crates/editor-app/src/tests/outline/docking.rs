//! Four-direction outline dragging, native resizing, file-refresh stability and workspace restoration.
use super::*;
use gpui_base::{
    Placement,
    dock::{DockPlacement, NodeId, PanelId},
};

/// Read a panel's Base leaf identity after every normalized move instead of retaining obsolete node IDs.
fn panel_node(app: &Entity<EditorApp>, id: PanelId, visual: &mut VisualTestContext) -> NodeId {
    visual.update(|_, cx| {
        let area = app.read(cx).dock_area.read(cx);
        [
            DockPlacement::Center,
            DockPlacement::Left,
            DockPlacement::Right,
            DockPlacement::Bottom,
        ]
        .into_iter()
        .find_map(|placement| area.layout(placement)?.find_panel_node(id))
        .unwrap()
    })
}

/// Bounds come from the project renderer after Base has reconciled and painted its real layout.
fn bounds(visual: &mut VisualTestContext, node: NodeId, title: bool) -> Bounds<Pixels> {
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let selector = format!(
        "local-dock-{}-{}",
        if title { "title" } else { "content" },
        node.as_u64()
    );
    visual
        .debug_bounds(Box::leak(selector.into_boxed_str()))
        .unwrap()
}

/// Resolve the current leaf before borrowing the renderer context for its measured bounds.
fn panel_bounds(
    app: &Entity<EditorApp>,
    id: PanelId,
    visual: &mut VisualTestContext,
    title: bool,
) -> Bounds<Pixels> {
    let node = panel_node(app, id, visual);
    bounds(visual, node, title)
}

/// Use native title-bar gestures, live drop preview and mouse release; no layout method substitutes for the drag.
fn drag(visual: &mut VisualTestContext, source: Bounds<Pixels>, destination: Point<Pixels>) {
    visual.simulate_mouse_down(source.center(), MouseButton::Left, Default::default());
    visual.simulate_mouse_move(
        source.center() + point(px(12.), px(0.)),
        MouseButton::Left,
        Default::default(),
    );
    visual.simulate_mouse_move(destination, MouseButton::Left, Default::default());
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        visual.debug_bounds("local-dock-drop-indicator").is_some(),
        "native drag must preview a split"
    );
    visual.simulate_mouse_up(destination, MouseButton::Left, Default::default());
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
}

/// Coordinate comparisons test the rendered direction and ratio, not only serialized field equality.
fn close(actual: Pixels, expected: Pixels) {
    assert!(
        (actual - expected).abs() < px(2.),
        "{actual:?} != {expected:?}"
    );
}

/// All four edge paths are actual Base center-tree splits; their bands enforce the confirmed stacking orientation.
#[gpui::test]
#[ignore = "build actual XML ZIP with the current public SDK first"]
fn native_outline_four_directions_resize_and_workspace_restore(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tree.xml");
    std::fs::write(&path, "<root>\n  <item>\n    <leaf/>\n  </item>\n</root>").unwrap();
    let package = package(false);
    let plugin_workspace = Workspace::open(directory.path()).unwrap();
    let mut manager = plugin_runtime::Manager::open(
        plugin_workspace.root().join(".runtime-plugin-test"),
        plugin_runtime::plugin_protocol::Environment {
            workspace: plugin_workspace.root().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    for placement in [
        Placement::Left,
        Placement::Right,
        Placement::Top,
        Placement::Bottom,
    ] {
        // Each gesture begins from a fresh local layout in the same trusted test workspace/provider lease.
        SessionState::for_workspace(&directory.path().canonicalize().unwrap()).save();
        let case_path = path.clone();
        let workspace = Workspace::open(directory.path()).unwrap();
        let initial_workspace = workspace.clone();
        let slot = Rc::new(RefCell::new(None));
        let capture = slot.clone();
        let (_, visual) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| EditorApp::new(initial_workspace, None, window, cx));
            *capture.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = slot.borrow_mut().take().unwrap();
        visual.simulate_resize(size(px(1200.), px(820.)));
        publish(&app, &mut manager, visual);
        visual.update(|window, cx| {
            app.update(cx, |app, cx| app.open_file(case_path.clone(), window, cx))
        });
        wait_outline(visual);
        let (outline, explorer, editor) = visual.update(|_, cx| {
            let app = app.read(cx);
            (
                PanelId::from(app.outline_panel.entity_id()),
                PanelId::from(app.explorer_panel.entity_id()),
                PanelId::from(app.editor_panel.entity_id()),
            )
        });
        let initial_outline = panel_bounds(&app, outline, visual, false);
        let initial_explorer = panel_bounds(&app, explorer, visual, false);
        close(initial_outline.size.height, initial_explorer.size.height);
        close(initial_outline.left(), initial_explorer.left());
        let target = panel_bounds(&app, editor, visual, false);
        let destination = match placement {
            Placement::Left => point(target.left() + px(10.), target.center().y),
            Placement::Right => point(target.right() - px(10.), target.center().y),
            Placement::Top => point(target.center().x, target.top() + px(10.)),
            Placement::Bottom => point(target.center().x, target.bottom() - px(10.)),
        };
        let title = panel_bounds(&app, outline, visual, true);
        drag(visual, title, destination);
        let moved = panel_bounds(&app, outline, visual, false);
        let editor_bounds = panel_bounds(&app, editor, visual, false);
        match placement {
            Placement::Left => assert!(moved.right() <= editor_bounds.left() + px(2.)),
            Placement::Right => assert!(moved.left() >= editor_bounds.right() - px(2.)),
            Placement::Top => assert!(moved.bottom() <= editor_bounds.top() + px(2.)),
            Placement::Bottom => assert!(moved.top() >= editor_bounds.bottom() - px(2.)),
        }
        // Join Explorer to the new band via a perpendicular pointer edge; its stack must respect the band's axis.
        let explorer_title = panel_bounds(&app, explorer, visual, true);
        let join = point(moved.right() - px(10.), moved.bottom() - px(10.));
        drag(visual, explorer_title, join);
        let outline_bounds = panel_bounds(&app, outline, visual, false);
        let explorer_bounds = panel_bounds(&app, explorer, visual, false);
        let vertical = matches!(placement, Placement::Left | Placement::Right);
        if vertical {
            close(outline_bounds.left(), explorer_bounds.left());
            close(outline_bounds.size.width, explorer_bounds.size.width);
            close(outline_bounds.size.height, explorer_bounds.size.height);
            assert!(outline_bounds.bottom() <= explorer_bounds.top() + px(2.));
        } else {
            close(outline_bounds.top(), explorer_bounds.top());
            close(outline_bounds.size.height, explorer_bounds.size.height);
            close(outline_bounds.size.width, explorer_bounds.size.width);
            assert!(outline_bounds.right() <= explorer_bounds.left() + px(2.));
        }
        // Resize the actual split divider to a deliberately unequal ratio, then persist the resulting measurements.
        let boundary = if vertical {
            point(
                outline_bounds.center().x,
                explorer_bounds.top() - px(PANEL_HEADER_HEIGHT) - px(2.),
            )
        } else {
            point(explorer_bounds.left() - px(2.), outline_bounds.center().y)
        };
        let delta = if vertical {
            point(px(0.), px(45.))
        } else {
            point(px(45.), px(0.))
        };
        visual.simulate_mouse_down(boundary, MouseButton::Left, Default::default());
        visual.simulate_mouse_move(
            boundary + delta * 0.25,
            MouseButton::Left,
            Default::default(),
        );
        visual.simulate_mouse_move(boundary + delta, MouseButton::Left, Default::default());
        visual.simulate_mouse_up(boundary + delta, MouseButton::Left, Default::default());
        let resized_outline = panel_bounds(&app, outline, visual, false);
        let resized_explorer = panel_bounds(&app, explorer, visual, false);
        let extent = |rect: Bounds<Pixels>| {
            if vertical {
                rect.size.height
            } else {
                rect.size.width
            }
        };
        assert!(
            (extent(resized_outline) - extent(resized_explorer)).abs() > px(40.),
            "real divider dragging must change the ratio"
        );
        visual.update(|window, cx| {
            app.read(cx).editor.clone().update(cx, |editor, cx| {
                editor.set_selected_range(0..0, cx);
                editor.focus(window, cx);
            })
        });
        visual.simulate_input(" ");
        wait_outline(visual);
        close(
            extent(panel_bounds(&app, outline, visual, false)),
            extent(resized_outline),
        );
        let session = visual.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.capture_dock_layout(cx);
                app.session_state.save();
                app.session_state.clone()
            })
        });
        assert!(
            !directory.path().join(".me-editor").exists(),
            "dock state belongs to the local workspace session"
        );
        let restored_slot = Rc::new(RefCell::new(None));
        let capture = restored_slot.clone();
        let (_, restored_visual) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
            *capture.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let restored = restored_slot.borrow_mut().take().unwrap();
        restored_visual.simulate_resize(size(px(1200.), px(820.)));
        publish(&restored, &mut manager, restored_visual);
        let restored_ids = restored_visual.update(|_, cx| {
            let app = restored.read(cx);
            assert!(!app.pending_dock_restore);
            (
                PanelId::from(app.outline_panel.entity_id()),
                PanelId::from(app.explorer_panel.entity_id()),
            )
        });
        let recovered_outline = panel_bounds(&restored, restored_ids.0, restored_visual, false);
        let recovered_explorer = panel_bounds(&restored, restored_ids.1, restored_visual, false);
        close(recovered_outline.left(), resized_outline.left());
        close(recovered_outline.top(), resized_outline.top());
        close(extent(recovered_outline), extent(resized_outline));
        close(extent(recovered_explorer), extent(resized_explorer));
        assert!(session.dock_layout.is_some());
    }
}
