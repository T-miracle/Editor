//! Installed SDK-built guests and a real stdio fixture must activate an already-open unsaved document.
use super::*;
use gpui_kit::{TestAppContext, gpui};
use serde_json::json;
use std::time::Instant;

/// Experimental plugin data reaches the real initialize request without overriding host transport capabilities.
#[test]
#[ignore = "build SDK capability-example and lsp_fixture first"]
fn arbitrary_client_capabilities_reach_the_native_lsp_handshake() {
    let directory = tempfile::tempdir().unwrap();
    let exe = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug/examples/lsp_fixture.exe")
        .canonicalize()
        .unwrap();
    let log = directory.path().join("wire.jsonl");
    let mut files = package(&exe, &log).files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["language_servers"][0]["client_experimental"] =
        json!({"unfamiliarFeature":{"version":2}});
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let package = super::language_tests::packages::repack(files).unwrap();
    let mut manager = plugin_runtime::Manager::open(
        directory.path().join("installed"),
        protocol::Environment {
            workspace: directory.path().display().to_string(),
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
    let server = crate::language::navigation::LanguageServer::from_service(plan).unwrap();
    server.prepare_until_ready().unwrap();
    let wire = std::fs::read_to_string(log).unwrap();
    let initialize = wire
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|message| message["method"] == "initialize")
        .unwrap();
    assert_eq!(
        initialize["params"]["capabilities"]["experimental"],
        json!({"unfamiliarFeature":{"version":2}})
    );
    assert_eq!(
        initialize["params"]["capabilities"]["general"]["positionEncodings"],
        json!(["utf-16"])
    );
    assert_eq!(
        initialize["params"]["capabilities"]["textDocument"]["completion"]["completionItem"]["snippetSupport"],
        false
    );
}

/// Preserve package inspection and explicit grants while supplying the user-selected local fixture.
pub(crate) fn package(exe: &Path, log: &Path) -> Package {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/plugin-api-test/capability-example.zip");
    let mut files = Package::read(&root).unwrap().files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["api"]["required"]["language.lsp"] = json!("^1");
    manifest["api"]["required"]["process"] = json!("^1");
    manifest["services"] = json!({"analysis":{"program":exe,"args":[log]}});
    manifest["permissions"]
        .as_array_mut()
        .unwrap()
        .push(json!("process.service.analysis"));
    manifest["settings"]["tool"] = json!({"title":"Executable", "value_type":{"kind":"string"},"default":"", "scope":"project"});
    manifest["language_servers"] = json!([{"id":"analysis","language":"novel","service":"analysis","hook":true,"executable_setting":"tool",
        "readiness":{"notification":"fixture/status","pointer":"/state","expected":"ready","timeout_ms":5000}},
        {"id":"secondary","language":"another-language","service":"analysis"}]);
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    super::language_tests::packages::repack(files).unwrap()
}

/// Publish the production worker's prepared plans together with its registry snapshot.
pub(crate) fn publish(
    app: &Entity<EditorApp>,
    manager: &mut plugin_runtime::Manager,
    cx: &mut gpui_kit::VisualTestContext,
) {
    let services = manager.language_services();
    // Shared GPUI fixtures mirror both production publications without exposing the worker as an API.
    let structures = manager.structure_providers();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.extensions.update(cx, |panel, cx| {
                {
                    let mut state = panel.worker.state.lock().unwrap();
                    state.entries = manager.published_entries();
                    // The public manager has finished preparation; mirror the actor's completed
                    // publication so layout restoration does not wait for a nonexistent test worker.
                    state.startup.clear();
                    state.language_services = services;
                    state.structure_providers = structures;
                    state.configuration_revision += 1;
                }
                panel.poll(cx);
            });
            app.sync_runtime_contributions(window, cx);
        })
    });
    cx.run_until_parked();
}

#[gpui::test]
#[ignore = "build capability-example and cargo build -p plugin-runtime --example lsp_fixture first"]
fn installed_hook_starts_unknown_lsp_for_unsaved_open_document_and_retires_it(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sample.novel");
    std::fs::write(&path, "disk = 1\n").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let mut manager = plugin_runtime::Manager::open(
        workspace.root().join(".runtime-plugin-test"),
        protocol::Environment {
            workspace: workspace.root().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(view.clone());
        Root::new(view, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_file(path.clone(), window, cx)));
    cx.run_until_parked();
    cx.simulate_input("draft");
    let source = cx.update(|_, cx| app.read(cx).editor.read(cx).text().to_string());
    assert!(source.contains("draft"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "disk = 1\n");
    let recognition = super::language_tests::packages::language_package("novel-recognition");
    manager.install(&recognition, Default::default()).unwrap();
    let exe = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug/examples/lsp_fixture.exe")
        .canonicalize()
        .unwrap();
    let log = directory.path().join("wire.jsonl");
    let package = package(&exe, &log);
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    publish(&app, &mut manager, cx);
    let mut server = cx.update(|_, cx| {
        let app = app.read(cx);
        assert!(app.editor.read(cx).lsp().definition_provider.is_some());
        app.language_servers["novel"].clone()
    });
    server.prepare_until_ready().unwrap();
    // Recognition can change while both LSP plans remain exactly the same published instances.
    let mut other_files =
        super::language_tests::packages::language_package("other-recognition").files;
    let other_source = String::from_utf8(other_files["plugin.toml"].clone())
        .unwrap()
        .replace("id = \"novel\"", "id = \"other\"")
        .replace("language = \"novel\"", "language = \"other\"");
    other_files.insert("plugin.toml".into(), other_source.into_bytes());
    let other = super::language_tests::packages::repack(other_files).unwrap();
    manager.install(&other, Default::default()).unwrap();
    publish(&app, &mut manager, cx);
    let original_document = server
        .document(&language_navigation::file_uri(&path).unwrap())
        .unwrap();
    crate::language::providers::choose(
        protocol::settings::Scope::User,
        "recognition:ext:novel",
        Some("other-recognition/other"),
    )
    .unwrap();
    cx.update(|_, cx| app.update(cx, |app, cx| app.sync_dynamic_languages(cx)));
    cx.run_until_parked();
    assert!(
        !original_document.is_active(),
        "recognition change must retire in-flight document requests"
    );
    cx.update(|_, cx| {
        let app = app.read(cx);
        assert!(
            app.editor.read(cx).lsp().definition_provider.is_none(),
            "recognition change must revoke old document providers"
        );
        assert!(
            Arc::ptr_eq(&server, &app.language_servers["novel"]),
            "unaffected service must retain its instance"
        );
    });
    crate::language::providers::choose(
        protocol::settings::Scope::User,
        "recognition:ext:novel",
        Some("novel-recognition/novel"),
    )
    .unwrap();
    cx.update(|_, cx| app.update(cx, |app, cx| app.sync_dynamic_languages(cx)));
    cx.run_until_parked();
    let navigation = cx.update(|window, cx| {
        let (provider, text) = {
            let editor = app.read(cx).editor.read(cx);
            (
                editor.lsp().definition_provider.clone().unwrap(),
                editor.text().clone(),
            )
        };
        provider.definitions(&text, 0, window, cx)
    });
    let result = Rc::new(RefCell::new(None));
    let outcome = result.clone();
    cx.update(|_, cx| {
        app.update(cx, |_, cx| {
            cx.spawn(async move |_, _| {
                *outcome.borrow_mut() = Some(navigation.await);
            })
            .detach();
        })
    });
    cx.run_until_parked();
    let links = result
        .borrow_mut()
        .take()
        .expect("navigation completed")
        .unwrap();
    assert_eq!(links.len(), 1);
    let uri = language_navigation::file_uri(&path).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(items) = server.diagnostics(uri.clone(), &source).unwrap() {
            assert!(
                items
                    .iter()
                    .any(|item| item.message == format!("unsaved:{source}"))
            );
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    server.document_closed(uri.clone()).unwrap();
    // A response on the same stream provides an ordering barrier after didClose.
    server.document_saved(uri.clone(), source.clone()).unwrap();
    let wire = std::fs::read_to_string(&log).unwrap();
    assert!(wire.contains("sdkHook") && wire.contains("Discovered label"));
    assert!(wire.contains("server-config") && wire.contains("enabled"));
    // Competing LSPs use the same explicit project selection rules as independent highlighters.
    let mut competitor = super::language_tests::packages::language_package("novel-second").files;
    let mut declaration: serde_json::Value =
        serde_json::from_slice(&competitor["manifest.json"]).unwrap();
    declaration["api"]["required"] = json!({"language.lsp":"^1", "process":"^1"});
    declaration["services"] =
        json!({"analysis":{"program":exe,"args":[directory.path().join("second.jsonl")]}});
    declaration["permissions"] = json!(["process.service.analysis"]);
    declaration["language_servers"] =
        json!([{"id":"analysis","language":"novel","service":"analysis"}]);
    competitor.insert(
        "manifest.json".into(),
        serde_json::to_vec(&declaration).unwrap(),
    );
    let competitor = super::language_tests::packages::repack(competitor).unwrap();
    manager
        .install(&competitor, competitor.manifest.permissions.clone())
        .unwrap();
    publish(&app, &mut manager, cx);
    assert!(Arc::ptr_eq(
        &server,
        &cx.update(|_, cx| app.read(cx).language_servers["novel"].clone())
    ));
    crate::language::providers::choose(
        protocol::settings::Scope::Project,
        "lsp:novel",
        Some("novel-second/analysis"),
    )
    .unwrap();
    cx.update(|_, cx| app.update(cx, |app, cx| app.sync_dynamic_language_servers(cx)));
    cx.run_until_parked();
    assert!(!server.is_active());
    manager.uninstall("novel-second", false).unwrap();
    publish(&app, &mut manager, cx);
    server = cx.update(|_, cx| app.read(cx).language_servers["novel"].clone());
    server.prepare_until_ready().unwrap();
    let secondary = manager.language_services()["capability-example/secondary"]
        .as_ref()
        .unwrap()
        .clone();
    let lease = manager.language_services()["capability-example/analysis"]
        .as_ref()
        .unwrap()
        .clone();
    assert!(Arc::ptr_eq(
        &lease,
        manager.language_services()["capability-example/analysis"]
            .as_ref()
            .unwrap()
    ));
    // Explicit bad configuration fails visibly and cannot silently fall back to the declaration.
    manager
        .update_setting(
            "capability-example",
            protocol::settings::Scope::Project,
            "tool",
            Some(json!(directory.path().join("missing.exe"))),
        )
        .unwrap();
    assert!(!lease.is_active());
    assert!(Arc::ptr_eq(
        &secondary,
        manager.language_services()["capability-example/secondary"]
            .as_ref()
            .unwrap()
    ));
    assert!(manager.language_services()["capability-example/analysis"].is_err());
    assert!(server.diagnostics(uri.clone(), &source).is_err());
    manager
        .update_setting(
            "capability-example",
            protocol::settings::Scope::Project,
            "tool",
            None,
        )
        .unwrap();
    publish(&app, &mut manager, cx);
    let restored = manager.language_services()["capability-example/analysis"]
        .as_ref()
        .unwrap()
        .clone();
    manager
        .update_setting(
            "capability-example",
            protocol::settings::Scope::Project,
            "label",
            Some(json!("emit-ui")),
        )
        .unwrap();
    let error = manager.language_services()["capability-example/analysis"]
        .as_ref()
        .err()
        .unwrap()
        .clone();
    assert!(error.contains("only a language proposal"), "{error}");
    // Invalid hook output may fault and withdraw the instance, but must never publish the forbidden view.
    assert!(
        manager.live["capability-example"]
            .views
            .values()
            .all(|scene| {
                !serde_json::to_string(scene.as_ref())
                    .unwrap()
                    .contains("forbidden hook view")
            })
    );
    // Protocol-invalid output uses the same explicit recovery gate as any other paused instance.
    manager.restart_plugin("capability-example").unwrap();
    manager
        .update_setting(
            "capability-example",
            protocol::settings::Scope::Project,
            "label",
            Some(json!("dynamic-start")),
        )
        .unwrap();
    assert!(
        manager.language_services()["capability-example/analysis"]
            .as_ref()
            .err()
            .unwrap()
            .contains("process.exec")
    );
    let mut dynamic_files = package.files.clone();
    let mut declaration: serde_json::Value =
        serde_json::from_slice(&dynamic_files["manifest.json"]).unwrap();
    declaration["permissions"]
        .as_array_mut()
        .unwrap()
        .push(json!("process.exec"));
    dynamic_files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&declaration).unwrap(),
    );
    let dynamic = super::language_tests::packages::repack(dynamic_files).unwrap();
    manager
        .install(&dynamic, dynamic.manifest.permissions.clone())
        .unwrap();
    publish(&app, &mut manager, cx);
    cx.update(|_, cx| app.read(cx).language_servers["novel"].clone())
        .prepare_until_ready()
        .unwrap();
    assert!(
        std::fs::read_to_string(&log)
            .unwrap()
            .contains("--hook-selected")
    );
    manager
        .update_setting(
            "capability-example",
            protocol::settings::Scope::Project,
            "label",
            Some(json!("escaped-project")),
        )
        .unwrap();
    assert!(manager.language_services()["capability-example/analysis"].is_err());
    manager.uninstall("capability-example", false).unwrap();
    assert!(!restored.is_active() && restored.spawn().is_err());
    publish(&app, &mut manager, cx);
    assert!(cx.update(|_, cx| {
        app.read(cx)
            .editor
            .read(cx)
            .lsp()
            .definition_provider
            .is_none()
    }));
    assert!(manager.language_services().is_empty());
}
