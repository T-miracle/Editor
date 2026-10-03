//! Package cutover must rebind the native editor and resynchronize its unsaved source to a fresh LSP.
use super::*;
use gpui_kit::{TestAppContext, VisualTestContext, gpui};
use serde_json::json;
use std::time::Instant;

/// The public SDK fixture owns migration/failure behavior; the application sees ordinary packages.
fn package(exe: &Path, log: &Path, version: u32, policy: &str) -> Package {
    let mut files = super::lsp_tests::package(exe, log).files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["version"] = json!(format!("{version}.0.0"));
    manifest["data_format"] = json!({"version":version,"migration_hook":true});
    manifest["api"]["required"]["storage.migration"] = json!("^1");
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    files.insert("migration-policy.txt".into(), policy.as_bytes().to_vec());
    super::language_tests::packages::repack(files).unwrap()
}

/// Advance the normal diagnostics debounce rather than manufacturing a didOpen through a transport helper.
fn wait_for_open(log: &Path, source: &str, count: usize, cx: &mut VisualTestContext) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        let wire = std::fs::read_to_string(log).unwrap_or_default();
        let matching = wire
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|event| {
                event["method"] == "textDocument/didOpen"
                    && event["params"]["textDocument"]["text"] == source
            })
            .count();
        if matching >= count {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "expected {count} didOpen frames carrying the editor draft; wire={wire}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Both commit and rollback replace the adapter while preserving the same open document's in-memory text.
#[gpui::test]
#[ignore = "build capability-example SDK package and lsp_fixture first"]
fn hot_update_and_rollback_resync_unsaved_open_document(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("open.novel");
    std::fs::write(&path, "disk text\n").unwrap();
    std::fs::write(directory.path().join("source.txt"), "workspace").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let mut manager = plugin_runtime::Manager::open(
        // The test panel resolves declarative assets from this same isolated production root.
        directory.path().join(".runtime-plugin-test"),
        protocol::Environment {
            workspace: workspace.root().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_file(path.clone(), window, cx)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        app.read(cx)
            .editor
            .clone()
            .update(cx, |editor, cx| editor.focus(window, cx))
    });
    cx.simulate_input("unsaved draft ");
    let draft = cx.update(|_, cx| app.read(cx).editor.read(cx).text().to_string());
    assert!(draft.contains("unsaved draft"));
    let recognition = super::language_tests::packages::language_package("novel-recognition");
    manager.install(&recognition, Default::default()).unwrap();
    let exe = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug/examples/lsp_fixture.exe")
        .canonicalize()
        .unwrap();
    let log = directory.path().join("wire.jsonl");
    let original = package(&exe, &log, 1, "ok");
    manager
        .install(&original, original.manifest.permissions.clone())
        .unwrap();
    manager
        .invoke_command(
            "capability-example",
            "scope-write",
            json!({"text":"private"}),
        )
        .unwrap();
    super::lsp_tests::publish(&app, &mut manager, cx);
    cx.update(|_, cx| {
        // A published server alone is insufficient: recognition must resolve from its installed package.
        assert_eq!(editor::language_for_path(&path), "novel");
        assert!(
            app.read(cx)
                .editor
                .read(cx)
                .lsp()
                .definition_provider
                .is_some()
        );
    });
    wait_for_open(&log, &draft, 1, cx);

    for (version, policy, expected_opens) in [(2, "ok", 2), (3, "activate-fail", 3)] {
        let previous = cx.update(|_, cx| app.read(cx).language_servers["novel"].clone());
        let candidate = package(&exe, &log, version, policy);
        let result = manager.install(&candidate, candidate.manifest.permissions.clone());
        assert_eq!(result.is_ok(), policy == "ok", "{result:?}");
        super::lsp_tests::publish(&app, &mut manager, cx);
        let replacement = cx.update(|_, cx| app.read(cx).language_servers["novel"].clone());
        assert!(
            !Arc::ptr_eq(&previous, &replacement),
            "cutover must create a new document transport owner"
        );
        assert!(!previous.is_active());
        wait_for_open(&log, &draft, expected_opens, cx);
        assert_eq!(
            cx.update(|_, cx| app.read(cx).editor.read(cx).text().to_string()),
            draft
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "disk text\n");
    }
    manager.uninstall("capability-example", false).unwrap();
    super::lsp_tests::publish(&app, &mut manager, cx);
}
