//! Faults enter through installed public WASM packages and ordinary native configuration actions.
#![cfg(windows)]
use super::*;
use crate::extensions::native_configuration_tests::{
    Driver, click, edit, fixture, open_form, probe,
};
use gpui_kit::{TestAppContext, VisualContext as _, gpui};
use plugin_runtime::Manager;

/// Shared native setup creates a real provider form, with no private schema writes in the test.
fn add(
    driver: &mut Driver,
    manager: &mut Manager,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
) -> String {
    click(cx, "run-config-add");
    driver.wait(manager, app, cx, |cx| {
        cx.debug_bounds("run-template-configuration-alpha-program")
            .is_some()
    });
    click(cx, "run-template-configuration-alpha-program");
    driver.wait(manager, app, cx, |cx| {
        cx.debug_bounds("plugin-ui-name").is_some()
    });
    cx.update(|_, cx| {
        app.read(cx)
            .run_form
            .as_ref()
            .unwrap()
            .read(cx)
            .plugin
            .as_ref()
            .unwrap()
            .selected
            .clone()
            .unwrap()
    })
}
fn saved(
    driver: &mut Driver,
    manager: &mut Manager,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
    parent: gpui_kit::AnyWindowHandle,
) {
    click(cx, "run-config-save");
    *cx = gpui_kit::VisualTestContext::from_window(parent, &cx.cx);
    driver.wait(manager, app, cx, |cx| {
        cx.update(|_, cx| app.read(cx).run_form.is_none())
    });
}

/// C20/C21: actual form errors retain unacknowledged input, recover on reopen, and persist timeouts.
#[gpui::test]
#[ignore = "build independent configuration examples and terminal package with the public SDK first"]
fn plugin_configuration_faults_preserve_input_recover_and_timeout(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (mut manager, app, cx) = fixture(cx, root.path());
    let mut driver = Driver::default();
    driver.frame(&mut manager, &app, cx);
    let parent = open_form(&app, cx);
    let id = add(&mut driver, &mut manager, &app, cx);
    let marker = root.path().join(".configuration-form-offline");
    std::fs::write(&marker, "offline").unwrap();
    edit(cx, "plugin-ui-name", "原值在插件故障后仍保留");
    saved(&mut driver, &mut manager, &app, cx, parent);
    let key = cx.update(|_, cx| app.read(cx).workspace_key());
    let stored = editor_core::load(&root.path().join("private-runs"), &key).unwrap();
    assert!(matches!(
        stored.plugin_configurations[&id].validation,
        ConfigurationValidation::Unavailable(_)
    ));
    assert!(!stored.plugin_configurations[&id].pending_events.is_empty());
    assert!(driver.launches.is_empty());
    std::fs::remove_file(&marker).unwrap();
    open_form(&app, cx);
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.update(|_, cx| {
            app.read(cx)
                .run_form
                .as_ref()
                .unwrap()
                .read(cx)
                .plugin
                .as_ref()
                .unwrap()
                .draft
                .plugin_configurations[&id]
                .name
                == "原值在插件故障后仍保留"
        })
    });
    saved(&mut driver, &mut manager, &app, cx, parent);
    let restored = editor_core::load(&root.path().join("private-runs"), &key).unwrap();
    assert!(
        restored.plugin_configurations[&id]
            .pending_events
            .is_empty()
    );
    assert_eq!(
        restored.plugin_configurations[&id].name,
        "原值在插件故障后仍保留"
    );

    // These are provider failures, including a genuinely retained invocation with the 30s deadline.
    let marker = root.path().join(".configuration-validation");
    for failure in ["error", "malformed", "timeout"] {
        open_form(&app, cx);
        driver.wait(&mut manager, &app, cx, |cx| {
            cx.debug_bounds("plugin-ui-name").is_some()
        });
        std::fs::write(&marker, failure).unwrap();
        let started = std::time::Instant::now();
        saved(&mut driver, &mut manager, &app, cx, parent);
        let data = editor_core::load(&root.path().join("private-runs"), &key)
            .unwrap()
            .plugin_configurations[&id]
            .clone();
        assert!(
            matches!(data.validation, ConfigurationValidation::Unavailable(_)),
            "{failure}: {:?}",
            data.validation
        );
        assert_eq!(data.name, "原值在插件故障后仍保留");
        if failure == "timeout" {
            assert!(started.elapsed() >= std::time::Duration::from_secs(29));
        }
        assert!(started.elapsed() < std::time::Duration::from_secs(40));
        assert!(driver.launches.is_empty());
        std::fs::remove_file(&marker).unwrap();
    }
    // Removing a provider also terminates Save, and another independent provider still supplies a form.
    manager.disable("configuration-alpha").unwrap();
    open_form(&app, cx);
    saved(&mut driver, &mut manager, &app, cx, parent);
    assert!(cx.update(|_, cx| {
        app.read(cx)
            .run_controls
            .plugin_configuration_blocker(&id)
            .is_some()
    }));
    open_form(&app, cx);
    click(cx, "run-config-add");
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.debug_bounds("run-template-configuration-beta-compact")
            .is_some()
    });
    click(cx, "run-template-configuration-beta-compact");
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.debug_bounds("plugin-ui-name").is_some()
    });
    edit(cx, "plugin-ui-name", "healthy provider");
    saved(&mut driver, &mut manager, &app, cx, parent);
    assert!(
        cx.update(|_, cx| app.read(cx).run_controls.selected().unwrap().name == "healthy provider")
    );
    assert!(driver.launches.is_empty());
    manager.shutdown();
}

/// C23/C25/C26: changed environment is rechecked for all execution intents; a retired receipt cannot launch.
#[gpui::test]
#[ignore = "build independent configuration examples and terminal package with the public SDK first"]
fn plugin_configuration_execution_rechecks_environment_and_rejects_retired_receipts(
    cx: &mut TestAppContext,
) {
    let root = tempfile::tempdir().unwrap();
    probe(root.path());
    let (mut manager, app, cx) = fixture(cx, root.path());
    let mut driver = Driver::default();
    driver.frame(&mut manager, &app, cx);
    let parent = open_form(&app, cx);
    let id = add(&mut driver, &mut manager, &app, cx);
    saved(&mut driver, &mut manager, &app, cx, parent);
    assert!(cx.update(|_, cx| {
        app.read(cx)
            .run_controls
            .plugin_configuration_blocker(&id)
            .is_none()
    }));
    let marker = root.path().join(".configuration-validation");
    std::fs::write(&marker, "environment changed").unwrap();
    for intent in [0, 1, 2] {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| match intent {
                0 => app.start_selected_run(window, cx),
                1 => app.build_selected(window, cx),
                _ => app.debug_configuration(&id, window, cx),
            })
        });
        driver.wait(&mut manager, &app, cx, |cx| {
            cx.update(|_, cx| app.read(cx).status.contains("Example validation rejected"))
        });
        assert!(driver.launches.is_empty());
        assert!(!root.path().join("argv.txt").exists());
        // Clear the visible message, so the next intent must receive a new actual plugin result.
        cx.update(|_, cx| app.update(cx, |app, _| app.status.clear()));
    }
    std::fs::remove_file(&marker).unwrap();
    // The actor completes validation without letting the UI consume it. Retirement in this gap
    // must invalidate that receipt even though its success was already published.
    cx.update(|window, cx| app.update(cx, |app, cx| app.start_selected_run(window, cx)));
    driver.collect_without_paint(&mut manager, &app, cx);
    manager.disable("configuration-alpha").unwrap();
    driver.frame(&mut manager, &app, cx);
    assert!(driver.launches.is_empty());
    assert!(!root.path().join("argv.txt").exists());
    assert!(cx.update(|_, cx| {
        app.read(cx)
            .run_controls
            .plugin_configuration_blocker(&id)
            .is_some()
    }));
    // Admission also closes the UI -> actor gap, using the same opaque public runtime origin.
    manager.enable("configuration-alpha").unwrap();
    driver.frame(&mut manager, &app, cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.start_selected_run(window, cx)));
    driver.collect_without_paint(&mut manager, &app, cx);
    driver.publish_without_collect(&mut manager, &app, cx);
    manager.disable("configuration-alpha").unwrap();
    driver.frame(&mut manager, &app, cx);
    assert!(driver.launches.is_empty());
    assert!(!root.path().join("argv.txt").exists());
    manager.enable("configuration-alpha").unwrap();
    driver.frame(&mut manager, &app, cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.start_selected_run(window, cx)));
    driver.collect_without_paint(&mut manager, &app, cx);
    let other = tempfile::tempdir().unwrap();
    manager
        .switch_workspace(
            plugin_runtime::plugin_protocol::Environment {
                workspace: other.path().display().to_string(),
                os: "windows".into(),
                ..Default::default()
            },
            true,
        )
        .unwrap();
    driver.frame(&mut manager, &app, cx);
    assert!(driver.launches.is_empty());
    assert!(!other.path().join("argv.txt").exists());
    manager
        .switch_workspace(
            plugin_runtime::plugin_protocol::Environment {
                workspace: root.path().display().to_string(),
                os: "windows".into(),
                ..Default::default()
            },
            true,
        )
        .unwrap();
    driver.frame(&mut manager, &app, cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.start_selected_run(window, cx)));
    driver.collect_without_paint(&mut manager, &app, cx);
    // Native deletion has its own pointer acceptance. Here its ordinary durable transaction lands
    // in the service/UI gap: a success for an identity that no longer exists must be discarded.
    let original = cx.update(|_, cx| app.read(cx).run_controls.configuration_set());
    cx.update(|_, cx| {
        app.update(cx, |app, _| {
            let mut set = app.run_controls.configuration_set();
            set.remove(&id);
            app.run_controls
                .commit_configuration_set(set, &app.workspace_key())
                .unwrap();
        })
    });
    driver.frame(&mut manager, &app, cx);
    assert!(driver.launches.is_empty());
    assert!(!root.path().join("argv.txt").exists());
    cx.update(|_, cx| {
        app.update(cx, |app, _| {
            app.run_controls
                .commit_configuration_set(original, &app.workspace_key())
                .unwrap();
        })
    });
    manager.set_workspace_trust(false).unwrap();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.start_configuration_without_environment(&id, window, cx)
        })
    });
    driver.frame(&mut manager, &app, cx);
    assert!(driver.launches.is_empty());
    manager.shutdown();
}

/// C24/C26: saving a failed configuration keeps its real process stoppable; missing grants stay missing.
#[gpui::test]
#[ignore = "build independent configuration examples and terminal package with the public SDK first"]
fn plugin_configuration_invalid_edit_keeps_owned_stop_and_permissions(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    probe(root.path());
    let (mut manager, app, cx) = fixture(cx, root.path());
    let mut driver = Driver::default();
    driver.frame(&mut manager, &app, cx);
    let parent = open_form(&app, cx);
    let id = add(&mut driver, &mut manager, &app, cx);
    edit(cx, "plugin-ui-argument-0", "hold");
    saved(&mut driver, &mut manager, &app, cx, parent);
    click(cx, "run-start");
    driver.wait(&mut manager, &app, cx, |_| {
        root.path().join("argv.txt").exists()
    });
    let session = driver.launches[0].0;
    assert!(
        manager
            .execution(session)
            .unwrap()
            .snapshot()
            .state
            .is_active()
    );
    open_form(&app, cx);
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.debug_bounds("plugin-ui-name").is_some()
    });
    edit(cx, "plugin-ui-name", "");
    saved(&mut driver, &mut manager, &app, cx, parent);
    assert!(cx.update(|_, cx| {
        app.read(cx)
            .run_controls
            .plugin_configuration_blocker(&id)
            .is_some()
    }));
    assert_eq!(
        manager.execution(session).unwrap().request().args,
        vec!["hold"]
    );
    click(cx, "run-stop");
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.update(|_, cx| {
            app.read(cx)
                .run_controls
                .sessions()
                .iter()
                .any(|session| session.config == id && !session.state.is_active())
        })
    });
    assert!(
        !manager
            .execution(session)
            .unwrap()
            .snapshot()
            .state
            .is_active()
    );
    assert_eq!(driver.launches.len(), 1);
    manager.shutdown();

    // The same package is installed through public management with process.exec deliberately ungranted.
    let no_grant = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        no_grant.path().join("runtime"),
        plugin_runtime::plugin_protocol::Environment {
            workspace: no_grant.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let package = plugin_runtime::Package::read(
        &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/run-config-plugin-tree/configuration-alpha.zip"),
    )
    .unwrap();
    let grants = package
        .manifest
        .permissions
        .iter()
        .filter(|permission| permission.as_str() != "process.exec")
        .cloned()
        .collect();
    assert!(
        manager.install(&package, grants).is_err(),
        "configuration data cannot supply process.exec confirmation"
    );
    assert!(manager.executions().is_empty());
    manager.shutdown();
}
