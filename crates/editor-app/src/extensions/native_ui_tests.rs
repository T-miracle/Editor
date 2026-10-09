//! Verify a native control reaches the existing worker with plugin and panel identity intact.
use super::*;
use gpui_kit::{TestAppContext, gpui};

/// A real host command reveals a hidden component panel and preserves its argument in rendered output.
#[gpui::test]
#[ignore = "build and package capability-example through the current public SDK first"]
fn host_command_reveals_real_panel_and_preserves_arguments(cx: &mut TestAppContext) {
    crate::tests::with_shortcut_editor(cx, false, vec![], |visual, app, path| {
        let workspace = path.parent().unwrap();
        std::fs::write(workspace.join("source.txt"), "workspace").unwrap();
        let runtime = tempfile::tempdir().unwrap();
        let mut manager = plugin_runtime::Manager::open(
            runtime.path().to_path_buf(),
            protocol::Environment {
                workspace: workspace.display().to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        let package = Package::read(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/plugin-api-test/capability-example.zip"),
        )
        .unwrap();
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
        let mut renderer = images::VectorRenderer::default();
        composable_tests::publish(&mut manager, &mut renderer, &app, visual);
        visual.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.hide_plugin_panel("capability-example", "welcome", cx)
            });
        });
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("plugin-ui-welcome-text").is_none());
        visual.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.invoke_plugin_command(
                    "capability-example",
                    "scope-write",
                    serde_json::json!({"text":"运行项目"}),
                    window,
                    cx,
                )
                .unwrap();
            });
        });
        composable_tests::pump(&mut manager, &app, visual);
        composable_tests::publish(&mut manager, &mut renderer, &app, visual);
        assert!(visual.debug_bounds("plugin-ui-welcome-text").is_some());
        // The guest's published UI is the rendered result of its workspace/private-data request,
        // so observing its text proves argument delivery beyond the host's queued work item.
        assert!(matches!(
            &manager.live["capability-example"].views["welcome"].root.kind,
            protocol::ui::Kind::Text { text } if text == "workspace|运行项目"
        ));
        visual.update(|window, cx| {
            let owner = app.read(cx).extensions.read(cx);
            assert!(
                owner
                    .invoke_command("capability-example", "undeclared", serde_json::Value::Null)
                    .is_err()
            );
            assert!(
                app.read(cx).plugin_panels["capability-example/welcome"]
                    .read(cx)
                    .focus
                    .is_focused(window)
            );
        });
        manager.disable("capability-example").unwrap();
        composable_tests::publish(&mut manager, &mut renderer, &app, visual);
        visual.update(|_, cx| {
            assert!(
                app.read(cx)
                    .extensions
                    .read(cx)
                    .invoke_command(
                        "capability-example",
                        "scope-write",
                        serde_json::json!({"text":"must not run"})
                    )
                    .is_err()
            );
        });
        assert!(visual.debug_bounds("plugin-ui-welcome-text").is_none());
    });
}

/// Plugin panel hiding cannot reach a peer's panel, persists visibility and releases the final dock.
#[gpui::test]
fn plugin_hide_requests_are_scoped_and_reclaim_the_empty_dock(cx: &mut TestAppContext) {
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
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let full_editor = cx.debug_bounds("editor-panel-content").unwrap();
    let owner = cx.update(|window, cx| {
        let owner = app.read(cx).extensions.clone();
        owner.update(cx, |owner, cx| {
            let mut manifest: protocol::Manifest =
                crate::extensions::test_manifest(include_str!("fixtures/panel-contract.json"));
            manifest.panels[0].default_visible = true;
            let mut state = owner.worker.state.lock().unwrap();
            for plugin in ["terminal", "peer"] {
                let mut manifest = manifest.clone();
                manifest.id = plugin.into();
                state.entries.push(Installed {
                    grants: manifest.permissions.clone(),
                    manifest,
                    digest: "fixture".into(),
                    enabled: true,
                    project_enabled: Default::default(),
                    global_enabled: None,
                    retired_ui_contract: false,
                    error: None,
                });
                state.views.insert(
                    format!("{plugin}/terminal"),
                    Arc::new(protocol::ui::Document::new(protocol::ui::Node::text(
                        "empty", "",
                    ))),
                );
            }
            drop(state);
            owner.poll(cx);
        });
        app.update(cx, |app, cx| app.sync_plugin_panels(window, cx));
        window.draw(cx).clear(cx);
        owner
    });
    // A guessed peer identity stays under the caller's own namespace and matches no panel.
    for (plugin, requested, terminal_visible, peer_visible) in [
        ("terminal", "peer/terminal", true, true),
        ("terminal", "terminal", false, true),
        ("peer", "terminal", false, false),
    ] {
        cx.update(|_, cx| {
            owner.update(cx, |owner, cx| {
                owner.poll(cx);
            });
        });
        cx.update(|_, cx| app.update(cx, |app, cx| app.hide_plugin_panel(plugin, requested, cx)));
        cx.run_until_parked();
        cx.update(|window, cx| {
            let app = app.read(cx);
            assert_eq!(
                app.plugin_panels["terminal/terminal"]
                    .read(cx)
                    .visible
                    .get(),
                terminal_visible
            );
            assert_eq!(
                app.plugin_panels["peer/terminal"].read(cx).visible.get(),
                peer_visible
            );
            if !terminal_visible {
                assert_eq!(
                    app.session_state.plugin_panel_visibility["terminal/terminal"],
                    false
                );
            }
            window.draw(cx).clear(cx);
        });
    }
    assert_eq!(
        cx.debug_bounds("editor-panel-content").unwrap(),
        full_editor
    );
    // The same panel can be reopened without installing another plugin or changing its dock size.
    cx.update(|window, cx| {
        let panel = app.read(cx).plugin_panels["terminal/terminal"].clone();
        panel.update(cx, |panel, cx| {
            panel.show(window, cx);
            cx.notify();
        });
        app.update(cx, |app, cx| app.sync_plugin_panels(window, cx));
        window.draw(cx).clear(cx);
        assert!(panel.read(cx).visible.get());
    });
    assert!(cx.debug_bounds("editor-panel-content").unwrap().size.height < full_editor.size.height);
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
        crate::extensions::test_manifest(include_str!("../../../../plugins/example/manifest.json"));
    let scene =
        protocol::ui::Document::new(protocol::ui::Node::button("increment", "增加")).revision(42);
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
                retired_ui_contract: false,
                error: None,
            }];
            state
                .views
                .insert("example/counter".into(), Arc::new(scene));
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
            matches!(work, Work::Event(plugin, _, Some(panel), event)
                if plugin == "example" && panel == "counter" && matches!(event, PluginEvent::Ui(protocol::ui::UiEvent {
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
        crate::extensions::test_manifest(include_str!("fixtures/panel-contract.json"));
    manifest.panels[0].default_visible = true;
    // The collection is an ordinary tree node; canvas routing remains a separate keyed target.
    let scene = protocol::ui::Document::new(
        protocol::ui::Node::row(
            "root",
            vec![
                protocol::ui::Node::new("drawing", protocol::ui::Kind::Canvas(Default::default()))
                    .grow(),
                protocol::ui::Node::new(
                    "sessions",
                    protocol::ui::Kind::SideTabs(protocol::ui::SideTabs {
                        id: "sessions".into(),
                        position: protocol::ui::SideTabsPosition::Right,
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
                ),
            ],
        )
        .grow(),
    )
    .revision(19);
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
                retired_ui_contract: false,
                error: None,
            }];
            state
                .views
                .insert("terminal/terminal".into(), Arc::new(scene));
            drop(state);
            owner.poll(cx);
        });
        app.update(cx, |app, cx| app.sync_plugin_panels(window, cx));
        window.draw(cx).clear(cx);
        owner
    });
    let bounds = cx.debug_bounds("side-tab-one").unwrap();
    // The real dock and canvas overlay must leave the selected inner edge visibly accented.
    cx.update(|window, _| {
        let scale = window.scale_factor();
        let accent = window
            .painted_quads()
            .into_iter()
            .find(|quad| {
                quad.background == gpui_kit::rgb(0x3574f0).into()
                    && quad.bounds.left().0 / scale == bounds.left() / px(1.)
                    && quad.bounds.top().0 / scale == bounds.top() / px(1.)
            })
            .expect("the docked sidebar paints its selected inner edge");
        let visible = accent.bounds.intersect(&accent.content_mask.bounds);
        assert_eq!(visible.size.width.0 / scale, 1.);
        assert_eq!(visible.size.height.0 / scale, bounds.size.height / px(1.));
        assert_eq!(visible.left().0 / scale, bounds.left() / px(1.));
    });
    cx.simulate_click(bounds.center(), Default::default());
    cx.run_until_parked();
    cx.update(|_,cx| {
        let messages:Vec<_>=owner.read(cx).worker.recorded.lock().unwrap().try_iter().collect();
        assert!(messages.iter().any(|work| matches!(work,Work::Event(plugin, _, Some(panel), event) if plugin=="terminal" && panel=="terminal" && matches!(event,PluginEvent::Ui(protocol::ui::UiEvent {node,action:protocol::ui::Action::Select(id),..}) if node=="sessions" && id=="one"))));
        assert!(!messages.iter().any(|work| matches!(work,Work::Event(_, _, Some(_), event) if matches!(event,PluginEvent::Ui(protocol::ui::UiEvent { action: protocol::ui::Action::Canvas(protocol::ui::CanvasEvent::Pointer { .. } | protocol::ui::CanvasEvent::Text { .. }), .. })))));
    });
}
