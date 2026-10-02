//! Exercise a real capability component through existing consent, publication and native UI seams.
use super::*;
use gpui_kit::{TestAppContext, VisualTestContext, gpui};

/// The fixture must be compiled by the public host SDK before this explicit integration test.
#[gpui::test]
#[ignore = "build with scripts/build-capability-example.ps1 first"]
fn capability_package_consent_displays_native_text_and_uninstall_reclaims_panel(
    cx: &mut TestAppContext,
) {
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
        protocol::Environment::default(),
    )
    .unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, editor_cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    editor_cx.simulate_resize(size(px(1200.), px(800.)));
    let editor_window = editor_cx.update(|window, _| window.window_handle());
    editor_cx.update(|window, cx| {
        app.read(cx).extensions.clone().update(cx, |owner, cx| {
            owner.pending = Some(package.clone());
            cx.notify();
        });
        app.update(cx, |app, cx| app.toggle_extensions(window, cx));
    });
    let dialog_window = editor_cx
        .update(|_, cx| cx.windows())
        .into_iter()
        .find(|handle| *handle != editor_window)
        .unwrap();
    let dialog_cx = VisualTestContext::from_window(dialog_window, editor_cx).into_mut();
    dialog_cx.run_until_parked();
    for _ in 0..3 {
        dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    assert!(dialog_cx.debug_bounds("plugin-install-consent").is_some());
    assert!(
        manager.installed.is_empty(),
        "consent must precede installation"
    );
    let confirm = dialog_cx.debug_bounds("plugin-install-confirm").unwrap();
    dialog_cx.simulate_click(confirm.center(), Default::default());
    let approved = dialog_cx.update(|_, cx| {
        let owner = app.read(cx).extensions.clone();
        let received = owner
            .read(cx)
            .worker
            .recorded
            .lock()
            .unwrap()
            .try_iter()
            .collect::<Vec<_>>();
        received
            .into_iter()
            .find_map(|work| match work {
                Work::Install(package) => Some(package),
                _ => None,
            })
            .expect("confirmation sends the approved real package to the existing worker seam")
    });
    manager
        .install(&approved, approved.manifest.permissions.clone())
        .unwrap();
    editor_cx.update(|window, cx| {
        app.read(cx).extensions.clone().update(cx, |owner, cx| {
            // Publish exactly the manager result through the same seam used by existing GPUI tests.
            let mut state = owner.worker.state.lock().unwrap();
            state.entries = manager.installed.values().cloned().collect();
            state.progress = None;
            for (id, instance) in &manager.live {
                for (panel, scene) in &instance.scenes {
                    state.scenes.insert(format!("{id}/{panel}"), scene.clone());
                }
            }
            drop(state);
            owner.poll(cx);
        });
        app.update(cx, |app, cx| app.sync_plugin_panels(window, cx));
        window.draw(cx).clear(cx);
    });
    editor_cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        editor_cx.debug_bounds("plugin-ui-welcome-text").is_some(),
        "installed guest text is rendered natively"
    );
    // Local host trust wins over a stale worker publication and persisted global enablement.
    editor_cx.update(|window, cx| {
        app.update(cx, |app, cx| app.set_workspace_trusted(false, cx));
        window.draw(cx).clear(cx);
    });
    editor_cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(editor_cx.debug_bounds("plugin-ui-welcome-text").is_none());
    editor_cx.update(|_, cx| {
        let owner = app.read(cx).extensions.read(cx);
        assert!(owner.entries.iter().all(|entry| !entry.enabled));
        assert!(
            owner
                .worker
                .recorded
                .lock()
                .unwrap()
                .try_iter()
                .any(|work| matches!(work, Work::SetTrust(false)))
        );
    });
    manager.set_workspace_trust(false).unwrap();
    assert_eq!(manager.resource_count(), 0);
    manager.uninstall(&approved.manifest.id, true).unwrap();
    editor_cx.update(|window, cx| {
        app.read(cx).extensions.clone().update(cx, |owner, cx| {
            let mut state = owner.worker.state.lock().unwrap();
            state.entries = manager.installed.values().cloned().collect();
            state.scenes.clear();
            drop(state);
            owner.poll(cx);
        });
        app.update(cx, |app, cx| app.sync_plugin_panels(window, cx));
        window.draw(cx).clear(cx);
    });
    editor_cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        editor_cx.debug_bounds("plugin-ui-welcome-text").is_none(),
        "uninstall removes the native panel without restart"
    );
}
