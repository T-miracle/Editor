//! A service-created session reaches the ordinary native dock through the existing request publication queue.
#![cfg(windows)]
use super::composable_tests::publish;
use super::*;
use gpui_kit::{TestAppContext, gpui};
#[path = "../../../plugin-runtime/tests/support/interactive_packages.rs"]
mod packages;

/// The caller does not name a provider; its selected implementation requests its own normal panel.
#[gpui::test]
#[ignore = "build terminal and capability-example through the exported SDK first"]
fn execution_service_reveals_native_panel_and_reclaims_it_on_disable(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let root = tempfile::tempdir().unwrap();
    let mut manager = plugin_runtime::Manager::open(
        root.path().join("runtime"),
        protocol::Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let terminal = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/terminal.zip"),
    )
    .unwrap();
    for package in [terminal, packages::fixture("execution-client", false, true)] {
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
    }
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let workspace = Workspace::open(root.path()).unwrap();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    cx.simulate_resize(size(px(1400.), px(900.)));
    let mut renderer = images::VectorRenderer::default();
    publish(&manager, &mut renderer, &app, cx);
    assert!(cx.debug_bounds("plugin-ui-output").is_none());
    manager
        .invoke_command(
            "execution-client",
            "service-open",
            serde_json::json!(packages::CONTRACT),
        )
        .unwrap();
    manager.invoke_command("execution-client", "service-call", serde_json::json!({
        "method":"execute", "value":{"program":"powershell.exe", "args":["-NoProfile","-Command","Start-Sleep -Seconds 60"], "name":"服务执行"}
    })).unwrap();
    manager.poll();
    publish(&manager, &mut renderer, &app, cx);
    let requests = manager
        .live
        .get_mut("terminal")
        .unwrap()
        .take_editor_requests();
    assert_eq!(requests.len(), 1);
    let request = requests[0].clone();
    cx.update(|_, cx| {
        app.read(cx).extensions.clone().update(cx, |owner, cx| {
            owner.worker.state.lock().unwrap().editor_requests.extend(
                requests
                    .into_iter()
                    .map(|request| ("terminal".into(), request)),
            );
            owner.poll(cx);
        });
    });
    cx.run_until_parked();
    manager.poll();
    publish(&manager, &mut renderer, &app, cx);
    assert!(matches!(
        request.status(),
        protocol::api::RequestUpdate::Completed {
            result: Ok(protocol::api::EditorValue::PanelVisibility { visible: true, .. })
        }
    ));
    assert!(cx.debug_bounds("plugin-ui-output").is_some());
    assert!(cx.debug_bounds("plugin-ui-sessions").is_some());
    manager.disable("terminal").unwrap();
    publish(&manager, &mut renderer, &app, cx);
    assert!(cx.debug_bounds("plugin-ui-output").is_none());
    assert!(cx.update(|_, cx| !app.read(cx).plugin_panels.contains_key("terminal/terminal")));
}
