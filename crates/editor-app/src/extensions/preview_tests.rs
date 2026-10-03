//! Exercise file-scoped preview contributions through real document and pointer interactions.

use super::*;
use gpui_kit::{TestAppContext, gpui};

/// Opening a supported file creates an editor-local preview without adding an outer dock.
#[gpui::test]
fn svg_preview_follows_open_documents_and_unsaved_edits(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let svg_path = directory.path().join("sample.SVG");
    let text_path = directory.path().join("notes.txt");
    let original = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"100\"/>";
    std::fs::write(&svg_path, original).unwrap();
    std::fs::write(&text_path, "ordinary document").unwrap();
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
    cx.update(|window, cx| {
        let owner = app.read(cx).extensions.clone();
        owner.update(cx, |owner, cx| {
            // Publish through the existing runtime boundary; no installed user plugins are touched.
            let manifest: protocol::Manifest = serde_json::from_value(serde_json::json!({
                "id": "svg", "name": "SVG 预览", "version": "0.1.0",
                "protocol": 6, "component": "svg.wasm",
                "permissions": ["editor.commands"], "storage_limit": 1024,
                "panels": [{ "id": "preview", "title": "SVG 预览",
                    "position": "editor", "file_extensions": ["svg"] }]
            }))
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
                "svg/preview".into(),
                Arc::new(Scene {
                    panel: "preview".into(),
                    font: "Segoe UI".into(),
                    font_size: 14.,
                    ..Default::default()
                }),
            );
            drop(state);
            owner.poll(cx);
        });
        app.update(cx, |app, cx| app.open_file(svg_path.clone(), window, cx));
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let source = cx.debug_bounds("editor-source-pane").expect("source pane");
    let preview = cx
        .debug_bounds("editor-preview-pane")
        .expect("preview pane");
    assert!(source.right() <= preview.left() + px(2.));
    assert!(source.size.width > px(100.) && preview.size.width > px(100.));
    // Drag the native divider, including movement after leaving its one-pixel painted line.
    let divider = cx.debug_bounds("editor-preview-divider").unwrap().center();
    cx.simulate_mouse_down(divider, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(
        divider + point(px(12.), px(0.)),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let destination = divider + point(px(120.), px(0.));
    cx.simulate_mouse_move(destination, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(destination, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let resized = cx.debug_bounds("editor-source-pane").unwrap();
    assert!(resized.size.width > source.size.width + px(80.));
    let preview = cx.debug_bounds("editor-preview-pane").unwrap();
    cx.update(|window, cx| {
        // Wheel input goes only to this surface, with coordinates relative to the preview panel.
        window.dispatch_event(
            PlatformInput::ScrollWheel(ScrollWheelEvent {
                position: preview.center(),
                delta: gpui_kit::ScrollDelta::Lines(point(0., 2.)),
                ..Default::default()
            }),
            cx,
        );
    });
    cx.update(|_, cx| {
        let app = app.read(cx);
        let panel = &app.plugin_panels["svg/preview"];
        let panel_id = gpui_base::dock::PanelId::from(panel.entity_id());
        assert!(app.dock_area.read(cx).panel(panel_id).is_none());
        let owner = app.extensions.read(cx);
        let events = owner
            .worker
            .recorded
            .lock()
            .unwrap()
            .try_iter()
            .filter_map(|work| {
                if let Work::Event(_, _, event) = work {
                    Some(serde_json::to_value(event).unwrap())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert!(
            events
                .iter()
                .any(|event| event["Surface"]["event"]["Document"]["text"] == original)
        );
        assert!(
            events
                .iter()
                .any(|event| event["Surface"]["event"]["Wheel"]["delta"] == 2.)
        );
    });
    // The public input action must synchronize the in-memory document before it is saved.
    let editor = cx.update(|_, cx| app.read(cx).editor.clone());
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.focus(window, cx);
        })
    });
    cx.simulate_input("<!-- draft -->");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|_, cx| {
        let owner = app.read(cx).extensions.read(cx);
        assert!(
            owner
                .worker
                .recorded
                .lock()
                .unwrap()
                .try_iter()
                .any(|work| {
                    let Work::Event(_, _, event) = work else {
                        return false;
                    };
                    serde_json::to_value(event).unwrap()["Surface"]["event"]["Document"]["text"]
                        .as_str()
                        .is_some_and(|text| text.starts_with("<!-- draft -->"))
                })
        );
    });
    assert_eq!(std::fs::read_to_string(&svg_path).unwrap(), original);
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_file(text_path, window, cx)));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("editor-preview-pane").is_none());
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_file(svg_path, window, cx)));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("editor-preview-pane").is_some());
    cx.update(|_, cx| {
        let owner = app.read(cx).extensions.clone();
        owner
            .read(cx)
            .worker
            .recorded
            .lock()
            .unwrap()
            .try_iter()
            .for_each(drop);
        owner
            .read(cx)
            .worker
            .state
            .lock()
            .unwrap()
            .instance_epochs
            .insert("svg".into(), 1);
        let panel = app.read(cx).plugin_panels["svg/preview"].clone();
        // A restarted guest must receive the document even if its viewer notices the epoch later.
        panel.update(cx, |panel, cx| panel.poll(cx));
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|_, cx| {
        let owner = app.read(cx).extensions.read(cx);
        assert!(
            owner
                .worker
                .recorded
                .lock()
                .unwrap()
                .try_iter()
                .any(|work| {
                    let Work::Event(_, _, event) = work else {
                        return false;
                    };
                    serde_json::to_value(event).unwrap()["Surface"]["event"]["Document"]["text"]
                        .as_str()
                        .is_some_and(|text| text.starts_with("<!-- draft -->"))
                }),
            "hot update must resynchronize the active draft automatically"
        );
    });
    // A clean file changed on disk reloads without a user edit or a document revision increment.
    let reload_path = directory.path().join("reloaded.svg");
    std::fs::write(&reload_path, original).unwrap();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.open_file(reload_path.clone(), window, cx))
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let replacement = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"150\" height=\"60\"/>";
    std::fs::write(&reload_path, replacement).unwrap();
    cx.update(|window, cx| {
        let owner = app.read(cx).extensions.read(cx);
        owner
            .worker
            .recorded
            .lock()
            .unwrap()
            .try_iter()
            .for_each(drop);
        app.update(cx, |app, cx| {
            app.apply_reconciliation(
                Reconciliation {
                    snapshot: None,
                    documents: vec![(
                        reload_path.canonicalize().unwrap(),
                        Ok(replacement.into()),
                        Instant::now(),
                    )],
                    renames: vec![],
                    native: true,
                },
                window,
                cx,
            )
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|_, cx| {
        let app = app.read(cx);
        assert!(!app.tabs[app.active_tab_index().unwrap()].session.is_dirty());
        let owner = app.extensions.read(cx);
        assert!(
            owner
                .worker
                .recorded
                .lock()
                .unwrap()
                .try_iter()
                .any(|work| {
                    let Work::Event(_, _, event) = work else {
                        return false;
                    };
                    serde_json::to_value(event).unwrap()["Surface"]["event"]["Document"]["text"]
                        == replacement
                }),
            "disk reload must refresh the preview even when the revision stays unchanged"
        );
    });
}
