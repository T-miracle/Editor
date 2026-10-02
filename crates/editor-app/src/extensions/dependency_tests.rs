//! A separately built SDK guest requests a downloaded private LSP and activates through normal publication.
use super::*;
use gpui_kit::{TestAppContext, gpui};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::io::{Cursor, Read, Write};

/// Trust revocation reaches the active install token immediately, without waiting behind the worker queue.
#[gpui::test]
fn restricting_workspace_cancels_queued_dependency_preparation(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let package = super::language_tests::packages::language_package("restricted-install");
    let slot = Rc::new(RefCell::new(None));
    let captured = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *captured.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    let control = cx.update(|_, cx| {
        app.read(cx).extensions.clone().update(cx, |panel, cx| {
            assert!(panel.queue_lifecycle(Work::Install(package.clone())));
            let control = panel
                .worker
                .state
                .lock()
                .unwrap()
                .install_control
                .clone()
                .unwrap();
            panel.set_workspace_trusted(false, cx);
            control
        })
    });
    let mut manager = plugin_runtime::Manager::open(
        directory.path().join("plugins"),
        protocol::Environment {
            workspace: directory.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        manager
            .install_with_control(&package, Default::default(), &control)
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
    assert!(manager.installed.is_empty());
}

#[gpui::test]
#[ignore = "build capability-example SDK guest and lsp_fixture executable first"]
fn wasm_dependency_plan_downloads_verifies_unpacks_and_starts_lsp(cx: &mut TestAppContext) {
    run_dependency_install(cx, false);
}

#[gpui::test]
#[ignore = "build current capability-example SDK guest, lsp_fixture and installer_fixture first"]
fn wasm_installer_plan_requires_consent_then_starts_private_lsp(cx: &mut TestAppContext) {
    run_dependency_install(cx, true);
}

/// Both data-only and native preparation must reach the same real LSP/document publication boundary.
fn run_dependency_install(cx: &mut TestAppContext, native_install: bool) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let exe = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug/examples/lsp_fixture.exe")
        .canonicalize()
        .unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("server.exe", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(&std::fs::read(&exe).unwrap()).unwrap();
    if native_install {
        zip.start_file("installer.exe", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&std::fs::read(exe.with_file_name("installer_fixture.exe")).unwrap())
            .unwrap();
    }
    let archive = zip.finish().unwrap().into_inner();
    let checksum = format!("{:x}", Sha256::digest(&archive));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/service.zip", listener.local_addr().unwrap());
    let download = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 4096];
        stream.read(&mut request).unwrap();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            archive.len()
        )
        .unwrap();
        stream.write_all(&archive).unwrap();
    });
    let log = directory.path().join("wire.jsonl");
    let mut files = super::lsp_tests::package(&exe, &log).files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["api"]["required"]["dependencies"] = json!("^1");
    manifest["permissions"]
        .as_array_mut()
        .unwrap()
        .push(json!("dependencies.prepare"));
    manifest["settings"]["label"]["default"] = json!("managed-dependency");
    manifest["settings_hook"] = json!(false);
    if native_install {
        manifest["permissions"]
            .as_array_mut()
            .unwrap()
            .push(json!("dependencies.install"));
    }
    manifest["services"]["analysis"]["program"] = json!("missing-global-program");
    manifest["language_servers"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let mut plan = json!({
        "executable":"server/server.exe", "artifacts":[{"id":"server","version":"1.0.0",
        "platform":format!("{}-{}",std::env::consts::OS,std::env::consts::ARCH),"sha256":checksum,
        "source":{"kind":"url","url":url},"format":{"kind":"zip"}}]
    });
    let marker = directory.path().join("installer-ran.txt");
    if native_install {
        plan["executable"] = json!("server/installed/tool.exe");
        plan["artifacts"][0]["installer"] = json!({"program":"installer.exe",
            "args":["${target}",marker,"install-lsp","${source}"],"target":"installed",
            "purpose":"Install the private analysis service","kind":"service"});
    }
    files.insert(
        "dependency-plan.json".into(),
        serde_json::to_vec(&plan).unwrap(),
    );
    let package = super::language_tests::packages::repack(files).unwrap();
    let path = directory.path().join("before-install.novel");
    std::fs::write(&path, "unsaved = false\n").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let mut manager = plugin_runtime::Manager::open(
        directory.path().join("plugins"),
        protocol::Environment {
            workspace: directory.path().display().to_string(),
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
    cx.simulate_input("draft");
    let language = super::language_tests::packages::language_package("managed-recognition");
    manager.install(&language, Default::default()).unwrap();
    let control = plugin_runtime::InstallControl::default().with_installer_prompts();
    let ui = control.clone();
    let marker_check = marker.clone();
    let consent = native_install.then(|| {
        std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(30);
            loop {
                if let Some(prompt) = ui.installer_prompt() {
                    assert!(!marker_check.exists());
                    assert!(ui.approve_installer(prompt.id, false));
                    break;
                }
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(20));
            }
        })
    });
    manager
        .install_with_control(&package, package.manifest.permissions.clone(), &control)
        .unwrap();
    if let Some(consent) = consent {
        consent.join().unwrap();
        assert!(marker.exists());
    }
    download.join().unwrap();
    super::lsp_tests::publish(&app, &mut manager, cx);
    let server = cx.update(|_, cx| app.read(cx).language_servers["novel"].clone());
    server.prepare_until_ready().unwrap();
    let uri = language_navigation::file_uri(&path).unwrap();
    let document = server.open_document(uri);
    let source = cx.update(|_, cx| app.read(cx).editor.read(cx).text().to_string());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(diagnostics) = server.diagnostics_for(document.clone(), &source).unwrap() {
            assert!(
                diagnostics
                    .iter()
                    .any(|item| item.message == format!("unsaved:{source}"))
            );
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let wire = std::fs::read_to_string(&log).unwrap();
    assert!(wire.contains("managed-dependency") && wire.contains("draft"));
    assert_eq!(std::fs::read_to_string(path).unwrap(), "unsaved = false\n");
    assert_eq!(manager.collect_dependency_cache().unwrap(), 0);
    server.retire();
    // The server is now offline: reinstallation can succeed only from verified private cache entries.
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    manager.uninstall("capability-example", false).unwrap();
}
