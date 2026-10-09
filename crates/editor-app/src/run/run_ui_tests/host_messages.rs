//! Production admission retains host refusals without reopening retired native execution paths.

use super::*;

/// The real Run, Build and Debug guards explain unsupported identities and restricted workspaces.
/// These host refusals must remain visible after the transient status changes, once per user action.
#[gpui::test]
fn host_messages_run_admission_retains_host_refusals(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("project");
    std::fs::create_dir_all(&workspace).unwrap();
    let (app, cx) = open_editor(cx, &workspace);
    let mut retained = 0;
    for restricted in [false, true] {
        for action in ["run", "build", "debug"] {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    let key = app.workspace_key();
                    app.extensions.update(cx, |extensions, cx| {
                        extensions.set_workspace_trusted(!restricted, cx);
                    });
                    app.run_controls = crate::run::RunControls::default();
                    let mut draft =
                        crate::run::RunConfigDraft::from_config(None, "host-admission".into());
                    draft.name = "Admission fixture".into();
                    draft.program = "fixture.exe".into();
                    let config = draft.to_config().unwrap();
                    if restricted {
                        // Current storage keeps a plugin identity and an unchecked projection.
                        // No validation receipt is synthesized: trust refuses this identity before
                        // any provider call. Reloading proves this fixture uses the current format.
                        app.run_controls
                            .accept_configuration_projection(
                                config,
                                editor_core::PluginConfiguration {
                                    provider: "configuration-fixture".into(),
                                    template: "program".into(),
                                    values: "{}".into(),
                                    pending_events: vec![],
                                    name: "Admission fixture".into(),
                                    program: "fixture.exe".into(),
                                    revision: 0,
                                    validation: editor_core::ConfigurationValidation::Unchecked,
                                },
                            )
                            .unwrap();
                        let mut set = app.run_controls.configuration_set();
                        set.select("host-admission");
                        let storage = root.path().join("current-configurations");
                        editor_core::save(&storage, &key, &set).unwrap();
                        app.run_controls = crate::run::RunControls::load_plugin_configurations(
                            &key,
                            Some(storage),
                        );
                    } else {
                        // The retained native DTO deliberately has no plugin identity. The current
                        // production guard must refuse it; this does not assert legacy execution.
                        app.run_controls.upsert(config, &key).unwrap();
                        app.run_controls.select("host-admission", &key);
                    }
                    match action {
                        "run" => app.start_configuration_without_environment(
                            "host-admission",
                            window,
                            cx,
                        ),
                        "build" => app.build_selected(window, cx),
                        "debug" => app.debug_selected(cx),
                        _ => unreachable!(),
                    }
                    let expected = if restricted {
                        t!("run.restricted").to_string()
                    } else {
                        t!("run.legacy_configuration").to_string()
                    };
                    assert_eq!(app.status, expected);
                    assert!(app.run_controls.sessions().is_empty());
                });
            });
            cx.run_until_parked();
            cx.update(|window, cx| window.draw(cx).clear(cx));
            retained += 1;
            let row = Box::leak(format!("host-message-{retained}").into_boxed_str());
            assert!(
                cx.debug_bounds(row).is_some(),
                "{action}: host refusal retained"
            );
            assert!(cx.debug_bounds("host-message-warning").is_some());
            let next = Box::leak(format!("host-message-{}", retained + 1).into_boxed_str());
            assert!(
                cx.debug_bounds(next).is_none(),
                "{action}: repeated rendering must not duplicate a completed refusal"
            );
        }
    }
}
