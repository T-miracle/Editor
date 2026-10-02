//! A native schema-driven form sends confirmed host work and observes the real guest's new configuration.
use super::*;
use gpui_kit::{TestAppContext, VisualTestContext, gpui};

#[gpui::test]
#[ignore = "build with scripts/build-capability-example.ps1 first"]
fn native_plugin_settings_apply_and_show_effective_source(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let package = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
    )
    .unwrap();
    let mut manager = plugin_runtime::Manager::open(
        directory.path().join("runtime"),
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
    let (_, editor_cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    let owner = editor_cx.update(|_, cx| app.read(cx).extensions.clone());
    editor_cx.update(|_, cx| {
        owner.update(cx, |owner, cx| {
            let mut state = owner.worker.state.lock().unwrap();
            state.entries = manager.published_entries();
            state.configurations.insert(
                package.manifest.id.clone(),
                manager
                    .effective_settings(&package.manifest.id)
                    .map_err(|e| e.to_string()),
            );
            drop(state);
            owner.poll(cx);
        })
    });
    // Enter through the actual settings dialog and its Plugins category, as a user would.
    editor_cx.run_until_parked();
    let editor_window = editor_cx.update(|window, _| window.window_handle());
    let trigger = editor_cx.debug_bounds("settings-trigger").unwrap();
    editor_cx.simulate_click(trigger.center(), Default::default());
    let dialog = editor_cx
        .update(|_, cx| cx.windows())
        .into_iter()
        .find(|handle| *handle != editor_window)
        .unwrap();
    let form_cx = VisualTestContext::from_window(dialog, editor_cx).into_mut();
    form_cx.run_until_parked();
    let nav = form_cx.debug_bounds("settings-nav-plugins").unwrap();
    form_cx.simulate_click(nav.center(), Default::default());
    form_cx.simulate_resize(size(px(1000.), px(800.)));
    form_cx.run_until_parked();
    let boolean = form_cx
        .debug_bounds("setting-capability-example-enabled-value")
        .unwrap();
    form_cx.simulate_click(boolean.center(), Default::default());
    let apply = form_cx
        .debug_bounds("setting-capability-example-enabled-apply")
        .unwrap();
    form_cx.simulate_click(apply.center(), Default::default());
    let work = form_cx.update(|_, cx| {
        owner
            .read(cx)
            .worker
            .recorded
            .lock()
            .unwrap()
            .try_iter()
            .find(|work| matches!(work, Work::SetSetting { .. }))
            .unwrap()
    });
    let Work::SetSetting {
        request,
        plugin,
        scope,
        key,
        value,
    } = work
    else {
        unreachable!()
    };
    assert_eq!(value, Some(serde_json::json!(false)));
    let result = manager
        .update_setting(&plugin, scope, &key, value)
        .map_err(|e| e.to_string());
    assert!(result.is_ok());
    form_cx.update(|_, cx| {
        owner.update(cx, |owner, cx| {
            let mut state = owner.worker.state.lock().unwrap();
            state.configurations.insert(
                plugin.clone(),
                manager
                    .effective_settings(&plugin)
                    .map_err(|e| e.to_string()),
            );
            state.configuration_result = Some((request, result));
            state.configuration_revision += 1;
            drop(state);
            owner.poll(cx);
        })
    });
    form_cx.run_until_parked();
    assert!(
        form_cx
            .debug_bounds("setting-capability-example-enabled-source-user")
            .is_some()
    );
    assert_eq!(
        manager.effective_settings(&plugin).unwrap()["enabled"].value,
        serde_json::json!(false)
    );
    // A syntactically valid value rejected by the WASM hook stays visible as an error in the same form.
    let input = form_cx
        .debug_bounds("setting-capability-example-label-value")
        .unwrap();
    form_cx.simulate_click(input.center(), Default::default());
    form_cx.simulate_keystrokes("ctrl-a");
    form_cx.simulate_input("invalid");
    let apply = form_cx
        .debug_bounds("setting-capability-example-label-apply")
        .unwrap();
    form_cx.simulate_click(apply.center(), Default::default());
    let work = form_cx.update(|_, cx| {
        owner
            .read(cx)
            .worker
            .recorded
            .lock()
            .unwrap()
            .try_iter()
            .find(|work| matches!(work, Work::SetSetting { .. }))
            .unwrap()
    });
    let Work::SetSetting {
        request,
        plugin,
        scope,
        key,
        value,
    } = work
    else {
        unreachable!()
    };
    let result = manager
        .update_setting(&plugin, scope, &key, value)
        .map_err(|e| e.to_string());
    assert!(result.is_err());
    form_cx.update(|_, cx| {
        owner.update(cx, |owner, cx| {
            let mut state = owner.worker.state.lock().unwrap();
            state.configuration_result = Some((request, result));
            state.configuration_revision += 1;
            drop(state);
            owner.poll(cx);
        })
    });
    form_cx.run_until_parked();
    assert!(form_cx.debug_bounds("plugin-settings-error").is_some());
    assert_eq!(
        manager.effective_settings(&plugin).unwrap()["label"].value,
        serde_json::json!("Discovered label")
    );
}
