//! Independent packages and a native crash fixture exercise the production LSP recovery boundary.
use super::*;
use gpui_kit::{TestAppContext, VisualTestContext, gpui};
use std::time::Duration;

/// The same worker publication and native button route recover a fault while the editor keeps unsaved text.
#[gpui::test]
#[ignore = "build capability-example first"]
fn native_restart_recovers_a_fault_without_blocking_document_input(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
    });
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("editing.txt");
    std::fs::write(&path, "original").unwrap();
    let workspace = Workspace::open(temp.path()).unwrap();
    let package = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
    )
    .unwrap();
    let mut manager = plugin_runtime::Manager::open(
        temp.path().join("plugins"),
        protocol::Environment {
            workspace: workspace.root().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let initial = path.clone();
    let (_, editor_cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, Some(initial), window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    editor_cx.run_until_parked();
    editor_cx.update(|window, cx| {
        app.read(cx)
            .editor
            .clone()
            .update(cx, |editor, cx| editor.focus(window, cx))
    });
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let start = barrier.clone();
    let fault = std::thread::spawn(move || {
        start.wait();
        assert!(
            manager
                .invoke_command("capability-example", "fault-spin", serde_json::json!(null))
                .is_err()
        );
        manager
    });
    barrier.wait();
    editor_cx.simulate_input("仍可编辑 ");
    editor_cx.run_until_parked();
    let mut manager = fault.join().unwrap();
    assert!(editor_cx.update(|_, cx| {
        app.read(cx)
            .editor
            .read(cx)
            .value()
            .to_string()
            .contains("仍可编辑")
    }));
    let owner = editor_cx.update(|_, cx| app.read(cx).extensions.clone());
    let editor_window = editor_cx.update(|window, _| window.window_handle());
    editor_cx.update(|window, cx| {
        owner.update(cx, |owner, cx| {
            let mut state = owner.worker.state.lock().unwrap();
            state.entries = manager.published_entries();
            state.diagnostics.insert(
                "capability-example".into(),
                manager.diagnostics("capability-example"),
            );
            drop(state);
            owner.poll(cx);
        });
        app.update(cx, |app, cx| app.toggle_extensions(window, cx));
    });
    let dialog = editor_cx
        .update(|_, cx| cx.windows())
        .into_iter()
        .find(|window| *window != editor_window)
        .unwrap();
    let form = VisualTestContext::from_window(dialog, editor_cx).into_mut();
    form.run_until_parked();
    form.update(|window, cx| window.draw(cx).clear(cx));
    let restart = form.debug_bounds("plugin-restart-action").unwrap();
    form.simulate_click(restart.center(), Default::default());
    let restart = form
        .update(|_, cx| {
            owner
                .read(cx)
                .worker
                .recorded
                .lock()
                .unwrap()
                .try_iter()
                .find_map(|work| {
                    if let Work::Restart(id) = work {
                        Some(id)
                    } else {
                        None
                    }
                })
        })
        .unwrap();
    manager.restart_plugin(&restart).unwrap();
    assert!(!manager.live["capability-example"].scenes.is_empty());
    assert!(editor_cx.update(|_, cx| {
        app.read(cx)
            .editor
            .read(cx)
            .value()
            .to_string()
            .contains("仍可编辑")
    }));
    assert_eq!(std::fs::read_to_string(path).unwrap(), "original");
}

/// A controlled monotonic clock verifies backoff without sleeping through production delays.
#[test]
#[ignore = "build capability-example and plugin-runtime lsp_fixture first"]
fn crashing_lsp_stops_retrying_and_manual_recovery_uses_a_new_transport() {
    let temp = tempfile::tempdir().unwrap();
    let exe = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug/examples/lsp_fixture.exe")
        .canonicalize()
        .unwrap();
    let log = temp.path().join("wire.jsonl");
    let marker = temp.path().join("crash");
    std::fs::write(&marker, "crash on initialize").unwrap();
    let mut files = lsp_tests::package(&exe, &log).files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["services"]["analysis"]["args"] = serde_json::json!([log, "--crash-marker", marker]);
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let package = language_tests::packages::repack(files).unwrap();
    let mut manager = plugin_runtime::Manager::open(
        temp.path().join("plugins"),
        protocol::Environment {
            workspace: temp.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let plan = manager.language_services()["capability-example/analysis"]
        .as_ref()
        .unwrap()
        .clone();
    let server = language_navigation::LanguageServer::from_service(plan).unwrap();
    assert!(server.prepare_at(Duration::ZERO).is_err());
    assert!(server.prepare_at(Duration::ZERO).is_err());
    // A long offline interval followed by one successful handshake must retain the previous failure.
    std::fs::remove_file(&marker).unwrap();
    server.prepare_at(Duration::from_secs(100)).unwrap();
    assert!(server.recovery_status().unwrap().contains("1/3"));
    std::fs::write(&marker, "crash again").unwrap();
    assert!(
        server
            .definitions(
                language_navigation::file_uri(&temp.path().join("x.novel")).unwrap(),
                "text".into(),
                lsp_types::Position::default()
            )
            .is_err()
    );
    assert!(server.recovery_status().unwrap().contains("2/3"));
    assert!(server.prepare_at(Duration::ZERO).is_err());
    assert!(server.prepare_at(Duration::from_secs(105)).is_err());
    assert!(server.prepare_at(Duration::from_secs(200)).is_err());
    let wire = std::fs::read_to_string(&log).unwrap();
    assert_eq!(
        wire.lines()
            .filter(|line| line.contains("startupArgs"))
            .count(),
        3
    );
    assert!(server.recovery_status().unwrap().contains("暂停"));
    std::fs::remove_file(marker).unwrap();
    manager.restart_plugin("capability-example").unwrap();
    let plan = manager.language_services()["capability-example/analysis"]
        .as_ref()
        .unwrap()
        .clone();
    let replacement = language_navigation::LanguageServer::from_service(plan).unwrap();
    replacement.prepare_until_ready().unwrap();
    assert!(
        server.prepare().is_err(),
        "retired adapter cannot receive replacement events"
    );
}
