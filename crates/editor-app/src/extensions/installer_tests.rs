//! Real GPUI consent interactions drive the same install token consumed by the native manager.
use super::*;
use gpui_kit::{TestAppContext, VisualTestContext, gpui};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

/// Build a data-only package with a native fixture: no application-private installer test entry exists.
fn package(marker: &std::path::Path) -> Package {
    let bytes = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/debug/examples/installer_fixture.exe"),
    )
    .unwrap();
    let manifest = json!({
        "id":"installer-ui","name":"Installer UI","version":"1.0.0","protocol":7,
        "api":{"base":"^1","required":{"language.lsp":"^1","process":"^1","dependencies":"^1"}},
        "contributions":"plugin.toml","storage_limit":1024,
        "permissions":["dependencies.prepare","dependencies.install","process.service.analysis"],
        "language_servers":[{"id":"analysis","language":"arbitrary","service":"analysis"}],
        "services":{"analysis":{"program":"unused","installation":{
            "executable":"server/installed/tool.exe","artifacts":[{
                "id":"server","version":"1","platform":format!("{}-{}",std::env::consts::OS,std::env::consts::ARCH),
                "sha256":format!("{:x}",Sha256::digest(&bytes)),"source":{"kind":"package","path":"installer.exe"},
                "format":{"kind":"file","path":"installer.exe"},"installer":{
                    "program":"installer.exe","args":["${target}",marker,"success"],"target":"installed",
                    "purpose":"Prepare private test tool","kind":"project_sdk"
                }
            }]
        }}}
    });
    super::language_tests::packages::repack(BTreeMap::from([
        ("manifest.json".into(),serde_json::to_vec(&manifest).unwrap()),
        ("installer.exe".into(),bytes),
        ("plugin.toml".into(),b"[plugin]\nid='installer-ui'\nname='Installer UI'\nversion='1.0.0'\nhost_version='^0.1'\n".to_vec()),
    ])).unwrap()
}

/// Disabled confirm, explicit SDK selection and final execution confirmation are separate real clicks.
#[gpui::test]
#[ignore = "build installer_fixture first"]
fn installer_dialog_requires_sdk_choice_and_concrete_execution_confirmation(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("executed.txt");
    let package = package(&marker);
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let captured = slot.clone();
    let (_, editor_cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *captured.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    let editor_window = editor_cx.update(|window, _| window.window_handle());
    let control = editor_cx.update(|_, cx| {
        app.read(cx).extensions.clone().update(cx, |panel, _| {
            assert!(panel.queue_lifecycle(Work::Install(package.clone())));
            panel
                .worker
                .state
                .lock()
                .unwrap()
                .install_control
                .clone()
                .unwrap()
        })
    });
    let runtime = directory.path().join("plugins");
    let workspace = directory.path().display().to_string();
    let token = control.clone();
    let worker = std::thread::spawn(move || {
        let mut manager = plugin_runtime::Manager::open(
            runtime,
            protocol::Environment {
                workspace,
                ..Default::default()
            },
        )
        .unwrap();
        manager.install_with_control(&package, package.manifest.permissions.clone(), &token)
    });
    let first = wait_prompt(&control, None);
    editor_cx.update(|window, cx| {
        app.read(cx)
            .extensions
            .clone()
            .update(cx, |panel, cx| panel.open_installation_progress(window, cx))
    });
    editor_cx.run_until_parked();
    let dialog = editor_cx
        .update(|_, cx| cx.windows())
        .into_iter()
        .find(|handle| *handle != editor_window)
        .unwrap();
    let dialog_cx = VisualTestContext::from_window(dialog, cx).into_mut();
    dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
    let confirm = dialog_cx
        .debug_bounds("plugin-install-native-confirm-region")
        .unwrap();
    dialog_cx.simulate_click(confirm.center(), Default::default());
    assert_eq!(control.installer_prompt().unwrap().id, first.id);
    assert!(!marker.exists());
    let checkbox = dialog_cx.debug_bounds("plugin-install-sdk-region").unwrap();
    dialog_cx.simulate_click(checkbox.center(), Default::default());
    dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
    dialog_cx.simulate_click(confirm.center(), Default::default());
    let second = wait_prompt(&control, Some(first.id));
    assert!(!second.preparation_only);
    assert!(!marker.exists());
    dialog_cx.update(|_, cx| {
        app.read(cx)
            .extensions
            .clone()
            .update(cx, |_, cx| cx.notify())
    });
    dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
    let confirm = dialog_cx
        .debug_bounds("plugin-install-native-confirm-region")
        .unwrap();
    dialog_cx.simulate_click(confirm.center(), Default::default());
    worker.join().unwrap().unwrap();
    assert!(marker.exists());
}

/// Bound the test's wait across the real worker/UI handoff, including prompt replacement.
fn wait_prompt(
    control: &plugin_runtime::InstallControl,
    previous: Option<u64>,
) -> plugin_runtime::InstallerPrompt {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(prompt) = control
            .installer_prompt()
            .filter(|prompt| Some(prompt.id) != previous)
        {
            return prompt;
        }
        assert!(Instant::now() < deadline, "no new installer prompt");
        std::thread::sleep(Duration::from_millis(20));
    }
}
