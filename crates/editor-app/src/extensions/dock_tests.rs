//! Dock resize regression through actual GPUI pointer input and current canvas notifications.
use super::*;
use gpui_kit::{TestAppContext, gpui};

/// Native dock dragging forwards canvas sizes and retains the user's size after hiding.
#[gpui::test]
fn native_dock_drag_preserves_canvas_viewport_and_saved_size(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        init(cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    let mut manifest: protocol::Manifest =
        crate::extensions::test_manifest(include_str!("fixtures/panel-contract.json"));
    // This test explicitly opens the dock; the packaged terminal now starts hidden.
    manifest.panels[0].default_visible = true;
    let scene = protocol::ui::Document::new(
        protocol::ui::Node::new(
            "output",
            protocol::ui::Kind::Canvas(protocol::ui::Canvas {
                focusable: true,
                grid: true,
                ..Default::default()
            }),
        )
        .grow(),
    )
    .revision(1);
    cx.simulate_resize(size(px(1100.), px(800.)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let full_editor = cx.debug_bounds("editor-panel-content").unwrap();
    let owner = cx.update(|window, cx| {
        let owner = app.read(cx).extensions.clone();
        owner.update(cx, |owner, cx| {
            let mut state = owner.worker.state.lock().unwrap();
            state.entries = vec![Installed {
                manifest: manifest.clone(),
                digest: "fixture".into(),
                grants: manifest.permissions.clone(),
                enabled: true,
                project_enabled: Default::default(),
                global_enabled: None,
                retired_ui_contract: false,
                error: None,
            }];
            state
                .views
                .insert("terminal/terminal".into(), Arc::new(scene.clone()));
            drop(state);
            owner.poll(cx);
        });
        app.update(cx, |app, cx| app.sync_plugin_panels(window, cx));
        let panel = app.read(cx).plugin_panels["terminal/terminal"].clone();
        panel.update(cx, |panel, cx| {
            panel.poll(cx);
            panel.focus(window, cx);
        });
        window.draw(cx).clear(cx);
        owner
    });
    let bounds = cx
        .debug_bounds("plugin-ui-output")
        .expect("manifest panel is visible without restarting");
    assert!(bounds.top() > px(300.));
    assert!(bounds.size.height > px(100.));
    // Drag the dock's upper edge, then verify both the visible size and persisted height.
    let edge = point(bounds.center().x, bounds.top() - px(PANEL_HEADER_HEIGHT));
    let target = edge - point(px(0.), px(100.));
    cx.simulate_mouse_down(edge, MouseButton::Left, Default::default());
    cx.simulate_mouse_move(
        edge - point(px(0.), px(10.)),
        MouseButton::Left,
        Default::default(),
    );
    cx.simulate_mouse_move(target, MouseButton::Left, Default::default());
    cx.simulate_mouse_up(target, MouseButton::Left, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let resized = cx.debug_bounds("plugin-ui-output").unwrap();
    assert!(
        resized.size.height > bounds.size.height + px(60.),
        "dragging the dock edge must enlarge the terminal: {bounds:?} -> {resized:?}"
    );
    cx.update(|_, cx| {
        let height = app.read(cx).session_state.plugin_dock_sizes["bottom"];
        assert!((height - (resized.size.height / px(1.) + PANEL_HEADER_HEIGHT)).abs() < 2.);
    });
    // The same handle must shrink the dock and forward its new viewport to the guest.
    let edge = point(resized.center().x, resized.top() - px(PANEL_HEADER_HEIGHT));
    let target = edge + point(px(0.), px(70.));
    cx.simulate_mouse_down(edge, MouseButton::Left, Default::default());
    cx.simulate_mouse_move(
        edge + point(px(0.), px(10.)),
        MouseButton::Left,
        Default::default(),
    );
    cx.simulate_mouse_move(target, MouseButton::Left, Default::default());
    cx.simulate_mouse_up(target, MouseButton::Left, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let shrunk = cx.debug_bounds("plugin-ui-output").unwrap();
    assert!(shrunk.size.height < resized.size.height - px(40.));
    let messages = cx.update(|_, cx| {
        owner
            .read(cx)
            .worker
            .recorded
            .lock()
            .unwrap()
            .try_iter()
            .collect::<Vec<_>>()
    });
    assert!(messages.iter().any(|work| matches!(work,
        Work::Event(_, _, Some(_), PluginEvent::Ui(protocol::ui::UiEvent {
            action: protocol::ui::Action::Canvas(protocol::ui::CanvasEvent::Resize {height,..}), ..
        })) if (*height - shrunk.size.height / px(1.)).abs() < 2.)));
    // Hiding reclaims editor space; reopening restores the persisted user height.
    cx.update(|_, cx| {
        app.update(cx, |app, cx| {
            app.hide_plugin_panel("terminal", "terminal", cx)
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("plugin-ui-output").is_none());
    assert!(
        cx.debug_bounds("editor-panel-content").unwrap().size.height >= full_editor.size.height
    );
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            // This layout fixture restores dock geometry, not a fabricated plugin command.
            // Real command admission and guest execution have separate package-driven coverage.
            app.plugin_panels["terminal/terminal"].update(cx, |panel, cx| {
                panel.show(window, cx);
                cx.notify();
            });
            app.dock_area.update(cx, |_, cx| cx.notify());
            cx.notify();
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let restored = cx.debug_bounds("plugin-ui-output").unwrap();
    assert!((restored.size.height - shrunk.size.height).abs() < px(2.));
}
