//! Verify a native control reaches the existing worker with plugin and panel identity intact.
use super::*;
use gpui_kit::{TestAppContext, gpui};

/// A host command targets its plugin and reveals a hidden panel without depending on current focus.
#[gpui::test]
fn host_command_reveals_hidden_terminal_and_preserves_arguments(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
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
    let arguments = serde_json::json!({ "name": "运行项目", "cwd": "C:/project" });
    cx.update(|window, cx| {
        let owner = app.read(cx).extensions.clone();
        owner.update(cx, |owner, cx| {
            let manifest: protocol::Manifest =
                serde_json::from_str(include_str!("../../../../plugins/terminal/manifest.json"))
                    .unwrap();
            let mut state = owner.worker.state.lock().unwrap();
            state.entries = vec![Installed {
                grants: manifest.permissions.clone(),
                manifest,
                digest: "fixture".into(),
                enabled: true,
                project_enabled: Default::default(),
                global_enabled: None,
                error: None,
            }];
            state.scenes.insert(
                "me.terminal/terminal".into(),
                Arc::new(Scene {
                    panel: "terminal".into(),
                    ..Default::default()
                }),
            );
            drop(state);
            owner.poll(cx);
        });
        app.update(cx, |app, cx| {
            app.sync_plugin_panels(window, cx);
            let panel = app.plugin_panels["me.terminal/terminal"].clone();
            assert!(!panel.read(cx).visible.get());
            app.invoke_plugin_command("me.terminal", "terminal.new", arguments.clone(), window, cx)
                .unwrap();
            assert!(panel.read(cx).visible.get());
            assert!(panel.read(cx).focus.is_focused(window));
        });
        assert!(owner.read(cx).worker.recorded.lock().unwrap().try_iter().any(|work| matches!(work,
            Work::Invoke { plugin, command, arguments: received }
                if plugin == "me.terminal" && command == "terminal.new" && received == arguments
        )));
        owner.update(cx, |owner, _| {
            let mut state = owner.worker.state.lock().unwrap();
            state.entries[0].enabled = false;
        });
        assert!(
            owner
                .read(cx)
                .invoke_command("me.terminal", "terminal.new", arguments.clone())
                .is_err()
        );
        owner.update(cx, |owner, _| {
            owner.worker.state.lock().unwrap().entries[0].enabled = true
        });
        assert!(
            owner
                .read(cx)
                .invoke_command("me.terminal", "undeclared", arguments.clone())
                .is_err()
        );
        assert!(
            owner
                .read(cx)
                .worker
                .recorded
                .lock()
                .unwrap()
                .try_iter()
                .all(|work| !matches!(work, Work::Invoke { .. }))
        );
    });
}

#[gpui::test]
fn native_panel_clicks_are_scoped_to_the_declared_surface(cx: &mut TestAppContext) {
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
    let manifest: protocol::Manifest =
        serde_json::from_str(include_str!("../../../../plugins/example/manifest.json")).unwrap();
    let scene = Scene {
        panel: "counter".into(),
        ui: Some(
            protocol::ui::Document::new(protocol::ui::Node::button("increment", "增加"))
                .revision(42),
        ),
        ..Default::default()
    };
    let owner = cx.update(|window, cx| {
        let owner = app.read(cx).extensions.clone();
        owner.update(cx, |owner, cx| {
            let mut state = owner.worker.state.lock().unwrap();
            state.entries = vec![Installed {
                manifest,
                digest: "fixture".into(),
                grants: Default::default(),
                enabled: true,
                project_enabled: Default::default(),
                global_enabled: None,
                error: None,
            }];
            state
                .scenes
                .insert("me.example/counter".into(), Arc::new(scene));
            drop(state);
            owner.poll(cx);
        });
        app.update(cx, |app, cx| app.sync_plugin_panels(window, cx));
        window.draw(cx).clear(cx);
        owner
    });
    let bounds = cx
        .debug_bounds("plugin-ui-increment")
        .expect("native protocol rendered in a dock panel");
    cx.simulate_click(bounds.center(), Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert!(owner.read(cx).worker.recorded.lock().unwrap().try_iter().any(|work| {
            matches!(work, Work::Event(plugin, PluginEvent::Surface { panel, event })
                if plugin == "me.example" && panel == "counter" && matches!(*event, PluginEvent::Ui(protocol::ui::UiEvent {
                    revision: 42, ref node, action: protocol::ui::Action::Click
                }) if node == "increment"))
        }));
    });
}

/// A canvas and its native sidebar share a panel, while pointer clicks stay out of the canvas.
#[gpui::test]
fn canvas_controls_sidebar_routes_ui_without_canvas_pointer_events(cx: &mut TestAppContext) {
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
        serde_json::from_str(include_str!("../../../../plugins/terminal/manifest.json")).unwrap();
    manifest.panels[0].default_visible = true;
    let scene = Scene {
        panel: "terminal".into(),
        font: "Cascadia Mono".into(),
        font_size: 14.,
        controls: Some(protocol::ui::CanvasControls {
            revision: 19,
            sidebar: Some(protocol::ui::SideTabs {
                id: "sessions".into(),
                width: 180.,
                min_width: 112.,
                max_width: 480.,
                selected: Some("one".into()),
                rename: None,
                items: vec![protocol::ui::SideTab {
                    id: "one".into(),
                    label: "会话".into(),
                    status: None,
                    closable: true,
                    disabled: false,
                }],
            }),
            menu: None,
        }),
        ..Default::default()
    };
    let owner = cx.update(|window, cx| {
        let owner = app.read(cx).extensions.clone();
        owner.update(cx, |owner, cx| {
            let mut state = owner.worker.state.lock().unwrap();
            state.entries = vec![Installed {
                manifest,
                digest: "fixture".into(),
                grants: Default::default(),
                enabled: true,
                project_enabled: Default::default(),
                global_enabled: None,
                error: None,
            }];
            state
                .scenes
                .insert("me.terminal/terminal".into(), Arc::new(scene));
            drop(state);
            owner.poll(cx);
        });
        app.update(cx, |app, cx| app.sync_plugin_panels(window, cx));
        window.draw(cx).clear(cx);
        owner
    });
    let bounds = cx.debug_bounds("side-tab-one").unwrap();
    cx.simulate_click(bounds.center(), Default::default());
    cx.run_until_parked();
    cx.update(|_,cx| {
        let messages:Vec<_>=owner.read(cx).worker.recorded.lock().unwrap().try_iter().collect();
        assert!(messages.iter().any(|work| matches!(work,Work::Event(plugin,PluginEvent::Surface{panel,event}) if plugin=="me.terminal" && panel=="terminal" && matches!(event.as_ref(),PluginEvent::Ui(protocol::ui::UiEvent {node,action:protocol::ui::Action::Select(id),..}) if node=="sessions" && id=="one"))));
        assert!(!messages.iter().any(|work| matches!(work,Work::Event(_,PluginEvent::Surface{event,..}) if matches!(event.as_ref(),PluginEvent::Pointer{..}|PluginEvent::Text(_)))));
    });
}
