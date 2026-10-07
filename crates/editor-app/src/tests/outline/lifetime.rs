//! A real independent WASM failure revokes the current native tree without a subsequent user event.
use super::*;

/// Add recognition to the independently SDK-built hostile guest; no fake LSP or grammar is needed for structure.
fn hostile_package() -> plugin_runtime::Package {
    let original = plugin_runtime::Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
    )
    .unwrap();
    let mut files = original.files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["structure_providers"] =
        serde_json::json!([{"id":"definitions", "language":"unfamiliar-language"}]);
    manifest["api"]["required"]["language.structure"] = serde_json::json!("^1");
    manifest["api"]["optional"]
        .as_object_mut()
        .unwrap()
        .remove("language.structure");
    manifest["contributions"] = serde_json::json!("plugin.toml");
    files.insert("plugin.toml".into(), format!("[plugin]\nid = \"capability-example\"\nname = \"Pure hostile structure\"\nversion = \"{}\"\nhost_version = \">=0.1.0\"\n[[language_definitions]]\nid = \"unfamiliar-language\"\nname = \"Pure structure\"\nextensions = [\"pure\"]\n", original.manifest.version).into_bytes());
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
    plugin_runtime::Package::from_bytes(&archive.finish().unwrap().into_inner()).unwrap()
}

/// Completion itself must clear its revoked target and request a paint, rather than wait for another editor operation.
#[gpui::test]
#[ignore = "build capability-example through the current public SDK first"]
fn native_structure_trap_clears_tree_without_another_document_event(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("document.pure");
    std::fs::write(&path, "healthy source").unwrap();
    let package = hostile_package();
    let workspace = Workspace::open(directory.path()).unwrap();
    let mut manager = plugin_runtime::Manager::open(
        workspace.root().join(".runtime-plugin-test"),
        plugin_runtime::plugin_protocol::Environment {
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
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    publish(&app, &mut manager, visual);
    visual.update(|window, cx| app.update(cx, |app, cx| app.open_file(path.clone(), window, cx)));
    wait_node(visual, "outline-node-/0");
    visual.update(|window, cx| {
        app.read(cx).editor.clone().update(cx, |editor, cx| {
            editor.set_selected_range(0..editor.text().len(), cx);
            editor.focus(window, cx);
        })
    });
    visual.simulate_input("trap");
    // This one input frame starts the new immutable request; later failure must invalidate it on its own completion.
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        let empty = visual.update(|_, cx| app.read(cx).outline.tree.read(cx).entry(0).is_none());
        let active = manager.structure_providers()["capability-example/definitions"]
            .as_ref()
            .unwrap()
            .is_active();
        // Do not force another frame while the callback is running: its own notification must revoke this target.
        if empty && !active {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "trapped structure did not revoke its native outline"
        );
        std::thread::sleep(Duration::from_millis(15));
    }
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("outline-empty").is_some());
    assert!(visual.debug_bounds("outline-node-/0").is_none());
    assert!(
        visual.debug_bounds("outline-loading").is_none(),
        "a revoked callback must not leave the panel indefinitely loading"
    );
    visual.update(|_, cx| assert_eq!(app.read(cx).editor.read(cx).text().to_string(), "trap"));
}
