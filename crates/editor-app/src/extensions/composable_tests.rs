//! Compose real native controls and a keyed drawing through the existing worker publication seam.
use super::*;
use gpui_kit::{TestAppContext, gpui};
use protocol::ui::{Action, Canvas, CanvasEvent, Document, Input, Kind, Node};

/// Standard text input, optional grid and canvas focus must not share a keyboard/IME event target.
#[gpui::test]
fn native_input_and_canvas_route_text_to_their_own_nodes_and_hide_reclaims_targets(
    cx: &mut TestAppContext,
) {
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
    let document = Document::new(
        Node::column(
            "root",
            vec![
                Node::row(
                    "toolbar",
                    vec![
                        Node::button("zoom", "放大"),
                        Node::input("caption", Input::default()).grow(),
                    ],
                )
                .height(40.),
                Node::new(
                    "viewport",
                    Kind::Canvas(Canvas {
                        focusable: true,
                        paint: vec![protocol::Paint::Fill {
                            rect: protocol::Rect {
                                x: 0.,
                                y: 0.,
                                w: 200.,
                                h: 200.,
                            },
                            color: 0x5599aa,
                            extend_to_bottom: true,
                        }],
                        ..Default::default()
                    }),
                )
                .grow(),
            ],
        )
        .grow(),
    )
    .revision(9);
    cx.update(|window, cx| {
        app.read(cx).extensions.clone().update(cx, |owner, cx| {
            let manifest: protocol::Manifest = serde_json::from_str(include_str!(
                "../../../../plugins/capability-example/manifest.json"
            ))
            .unwrap();
            let mut state = owner.worker.state.lock().unwrap();
            state.entries = vec![Installed {
                grants: manifest.permissions.clone(),
                manifest,
                digest: "composed".into(),
                enabled: true,
                project_enabled: Default::default(),
                global_enabled: None,
                error: None,
            }];
            state.scenes.insert(
                "capability-example/welcome".into(),
                Arc::new(Scene {
                    panel: "welcome".into(),
                    ui: Some(document),
                    ..Default::default()
                }),
            );
            drop(state);
            owner.poll(cx);
        });
        app.update(cx, |app, cx| app.sync_plugin_panels(window, cx));
    });
    cx.simulate_resize(size(px(1200.), px(800.)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    let viewport = cx.debug_bounds("plugin-ui-viewport").unwrap();
    let input = cx.debug_bounds("plugin-ui-caption").unwrap();
    assert!(viewport.size.width > px(0.) && viewport.size.height > px(0.));
    assert!(viewport.top() >= input.bottom());
    let initial = events(&app, cx);
    assert!(initial.iter().any(|event| matches!(
        event.action,
        Action::Canvas(CanvasEvent::Resize { grid: None, .. })
    )));
    cx.simulate_click(input.center(), Default::default());
    cx.simulate_input("中文 空格");
    cx.run_until_parked();
    let typed = events(&app, cx);
    assert!(typed.iter().any(|event| event.node == "caption"
        && matches!(&event.action,Action::Change(text) if text.contains("中文 空格"))));
    assert!(!typed.iter().any(|event| matches!(
        event.action,
        Action::Canvas(CanvasEvent::Text { .. } | CanvasEvent::Key { .. })
    )));
    cx.simulate_click(viewport.center(), Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_input("画 布");
    cx.run_until_parked();
    let typed = events(&app, cx);
    let text = typed
        .iter()
        .filter_map(|event| {
            if let Action::Canvas(CanvasEvent::Text { text }) = &event.action {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<String>();
    assert_eq!(text, "画 布");
    assert!(
        !typed
            .iter()
            .any(|event| event.node == "caption" && matches!(event.action, Action::Change(_)))
    );
    cx.update(|_, cx| {
        app.update(cx, |app, cx| {
            app.hide_plugin_panel("capability-example", "welcome", cx)
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("plugin-ui-viewport").is_none());
    assert!(cx.update(|_, cx| {
        app.read(cx).plugin_panels["capability-example/welcome"]
            .read(cx)
            .native_ui
            .is_none()
    }));
}

/// Observe public node events as they enter the production worker channel, without reaching into widgets.
fn events(
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
) -> Vec<protocol::ui::UiEvent> {
    cx.update(|_, cx| {
        app.read(cx)
            .extensions
            .read(cx)
            .worker
            .recorded
            .lock()
            .unwrap()
            .try_iter()
            .filter_map(|work| {
                if let Work::Event(_, _, PluginEvent::Surface { event, .. }) = work {
                    if let PluginEvent::Ui(event) = *event {
                        return Some(event);
                    }
                }
                None
            })
            .collect()
    })
}

/// Native clicks and unsaved editor input complete a real WASM → raster → GPUI round trip.
#[gpui::test]
#[ignore = "build capability-example through scripts/build-capability-example.ps1 first"]
fn composed_wasm_preview_follows_memory_and_reclaims_the_editor_split(cx: &mut TestAppContext) {
    use std::io::{Cursor, Write};
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("drawing.svg");
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="160" height="100"><rect width="160" height="100" fill="red"/></svg>"#;
    std::fs::write(&path, svg).unwrap();
    let mut files = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
    )
    .unwrap()
    .files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["settings_hook"] = serde_json::json!(false);
    manifest["settings"]["label"]["default"] = serde_json::json!("composable-ui");
    manifest["panels"][0]["position"] = serde_json::json!("editor");
    manifest["panels"][0]["file_extensions"] = serde_json::json!(["svg"]);
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        archive
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(&bytes).unwrap();
    }
    let package = Package::from_bytes(&archive.finish().unwrap().into_inner()).unwrap();
    let mut manager = plugin_runtime::Manager::open(
        directory.path().join("runtime"),
        protocol::Environment {
            workspace: directory.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let mut renderer = images::VectorRenderer::default();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    cx.simulate_resize(size(px(1200.), px(800.)));
    publish(&manager, &mut renderer, &app, cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_file(path.clone(), window, cx)));
    cx.run_until_parked();
    pump(&mut manager, &app, cx);
    publish(&manager, &mut renderer, &app, cx);
    assert!(cx.debug_bounds("editor-preview-pane").is_some());
    assert!(cx.debug_bounds("plugin-ui-caption").is_some());
    cx.update(|_, cx| {
        let owner = app.read(cx).extensions.read(cx);
        let images = &owner.worker.state.lock().unwrap().images;
        assert!(images["capability-example/welcome/canvas/viewport"][0].is_some());
    });
    let zoom = cx.debug_bounds("plugin-ui-zoom").unwrap();
    cx.simulate_click(
        point(zoom.left() + px(20.), zoom.center().y),
        Default::default(),
    );
    cx.run_until_parked();
    pump(&mut manager, &app, cx);
    publish(&manager, &mut renderer, &app, cx);
    let tree = manager.live["capability-example"].scenes["welcome"]
        .ui
        .as_ref()
        .unwrap();
    let Kind::Canvas(canvas) = &tree.active_node("viewport").unwrap().kind else {
        unreachable!()
    };
    assert!(matches!(canvas.paint[0], protocol::Paint::Svg {rect,..} if rect.w==200.));
    // The document's memory revision changes while disk content remains untouched.
    cx.update(|window, cx| {
        app.read(cx)
            .editor
            .clone()
            .update(cx, |editor, cx| editor.focus(window, cx))
    });
    cx.simulate_input("未保存 ");
    cx.run_until_parked();
    pump(&mut manager, &app, cx);
    publish(&manager, &mut renderer, &app, cx);
    let tree = manager.live["capability-example"].scenes["welcome"]
        .ui
        .as_ref()
        .unwrap();
    assert!(serde_json::to_string(tree).unwrap().contains("未保存 "));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), svg);
    assert!(tree.source.is_some());
    manager.uninstall("capability-example", true).unwrap();
    publish(&manager, &mut renderer, &app, cx);
    assert!(cx.debug_bounds("editor-preview-pane").is_none());
    assert!(cx.debug_bounds("plugin-ui-viewport").is_none());
    assert!(cx.update(|_, cx| app.read(cx).plugin_panels.is_empty()));
}

/// Replay only production worker events; an old native callback is explicitly rejected, not a crash.
fn pump(
    manager: &mut plugin_runtime::Manager,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
) {
    let work: Vec<_> = cx.update(|_, cx| {
        app.read(cx)
            .extensions
            .read(cx)
            .worker
            .recorded
            .lock()
            .unwrap()
            .try_iter()
            .collect()
    });
    for work in work {
        if let Work::Event(id, _, event) = work {
            if let Err(error) = manager.event(&id, event) {
                assert!(
                    matches!(error.downcast_ref::<protocol::api::Failure>(), Some(error) if error.code==protocol::api::ErrorCode::StaleRevision),
                    "{error:#}"
                );
            }
        }
    }
}

/// The existing worker publication seam also supplies the actual asynchronous vector renderer output.
fn publish(
    manager: &plugin_runtime::Manager,
    renderer: &mut images::VectorRenderer,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
) {
    let scenes: BTreeMap<_, _> = manager
        .live
        .iter()
        .flat_map(|(id, instance)| {
            instance
                .scenes
                .iter()
                .map(move |(panel, scene)| (format!("{id}/{panel}"), scene.clone()))
        })
        .collect();
    let images = renderer.prepare(&scenes);
    cx.update(|window, cx| {
        app.read(cx).extensions.clone().update(cx, |owner, cx| {
            let mut state = owner.worker.state.lock().unwrap();
            state.entries = manager.published_entries();
            state.scenes = scenes;
            state.images = images;
            drop(state);
            owner.poll(cx);
        });
        app.update(cx, |app, cx| app.sync_plugin_panels(window, cx));
        for panel in app
            .read(cx)
            .plugin_panels
            .values()
            .cloned()
            .collect::<Vec<_>>()
        {
            panel.update(cx, |panel, cx| panel.poll(cx));
        }
        window.draw(cx).clear(cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}
