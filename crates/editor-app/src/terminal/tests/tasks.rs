//! Run's real configuration entry feeds owned native PTYs and task tabs without a terminal package.

use super::*;
use crate::extensions::native_configuration_tests::{Driver, click};
use plugin_runtime::Manager;

/// A fast stdio provider can publish output and its completion together; the next header follows both.
#[gpui::test]
fn builtin_task_provider_tail_precedes_the_next_step(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let root = tempfile::tempdir().unwrap();
    let (app, visual) = super::restore::open(cx, Workspace::open(root.path()).unwrap());
    visual.simulate_resize(size(px(1400.), px(900.)));
    visual.update(|window, cx| app.update(cx, |app, cx| {
        let config: editor_core::RunConfig = serde_json::from_value(serde_json::json!({
            "id":"fast-provider", "name":"NEXT_TARGET", "target":{"mode":"provided","provider":"build-provider","binding":"{}","label":"NEXT_TARGET","args":[]},
            "build":[{"name":"BUILD_HEADER", "target":{"kind":"action", "target":{"mode":"provided","provider":"build-provider","binding":"{}","label":"build","args":[]}}}]
        })).unwrap();
        let workspace = app.workspace_key();
        app.run_controls.upsert(config, &workspace).unwrap();
        let request = app.run_controls.begin("fast-provider");
        let plan = app.run_controls.launch_plan("fast-provider", &workspace).unwrap();
        app.run_controls.begin_sequence("fast-provider", plan, request);
        app.terminal.update(cx, |panel, cx| { assert!(panel.begin_task("fast-provider", "NEXT_TARGET", request, cx)); });
        app.reveal_terminal(window, cx);
        app.drive_preparation(cx);
        let request = app.run_controls.active_provider_preparations()[0];
        // Inject only an asynchronous worker publication; production sync owns ordering and layout.
        crate::extensions::native_configuration_tests::completed_preparation_frame(app, cx,
            ("fast-provider".into(), 0, request, Ok("powershell.exe".into())), plugin_runtime::PreparationSnapshot {
            output:"FINAL_BUILD_LINE\n".into(), provider:"build-provider".into(), state:plugin_runtime::ExecutionState::Exited,
        });
        app.sync_run_controls(window, cx);
    }));
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let output = painted(&app, visual);
    assert!(
        output.find("FINAL_BUILD_LINE").unwrap() < output.rfind("NEXT_TARGET").unwrap(),
        "{output}"
    );
}

/// Store a normal built-in Shell form; Run still performs its production validation and save admission.
fn configuration(app: &Entity<EditorApp>, visual: &mut VisualTestContext, id: &str, script: &str) {
    let mut fields = protocol::configurations::command_form::Fields::new(
        id,
        vec!["-NoLogo".into(), "-NoProfile".into(), "-Command".into()],
    );
    fields.script = Some(script.into());
    let values = serde_json::json!({"shell":"PowerShell","fields":fields});
    visual.update(|_, cx| app.update(cx, |app, _| {
        let config = serde_json::from_value(serde_json::json!({"id":id,"name":id,"target":{"mode":"program","program":"powershell.exe","args":[]}})).unwrap();
        let data = editor_core::PluginConfiguration {
            provider:super::super::configurations::PROVIDER.into(), template:"PowerShell".into(),
            values:values.to_string(), pending_events:vec![], name:id.into(), program:"powershell.exe".into(), revision:0,
            validation:editor_core::ConfigurationValidation::Unchecked,
        };
        app.run_controls.accept_configuration_projection(config, data).unwrap();
    }));
}

/// Parallel input, duplicate location, close cancel/confirm, reuse and new-tab-after-close share one fixture.
#[gpui::test]
fn builtin_tasks_parallel_input_reuse_and_confirm_close(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        root.path().join("plugins"),
        protocol::Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let (app, visual) = super::restore::open(cx, Workspace::open(root.path()).unwrap());
    visual.simulate_resize(size(px(1400.), px(900.)));
    let mut driver = Driver::default();
    driver.frame(&mut manager, &app, visual);
    visual
        .update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("authorized-copy".into())));
    for id in ["task-a", "task-b"] {
        configuration(
            &app,
            visual,
            id,
            &format!(
                "[Console]::Write('\x1b]52;c;aGlqYWNr\x07'); Write-Output 'READY-{id}'; $line=[Console]::ReadLine(); Write-Output ('ANSWER-{id}:'+$line); Start-Sleep -Seconds 60"
            ),
        );
        visual.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.start_configuration_without_environment(id, window, cx)
            })
        });
        driver.wait(&mut manager, &app, visual, |visual| {
            painted(&app, visual).contains(&format!("READY-{id}"))
        });
    }
    assert!(manager.installed.is_empty());
    // Public process output cannot use OSC52 to silently gain clipboard authority.
    assert_eq!(
        visual.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text())),
        Some("authorized-copy".into())
    );
    assert_eq!(
        visual.update(|_, cx| app.read(cx).terminal.read(cx).sessions.len()),
        2,
        "task entry must not create an extra Shell"
    );
    let first_id = visual.update(|_, cx| app.read(cx).terminal.read(cx).sessions[0].id);
    // GPUI's debug selector API accepts static strings; this one fixture owns one selector.
    let first_selector: &'static str = Box::leak(format!("side-tab-{first_id}").into_boxed_str());
    let first = visual.debug_bounds(first_selector).unwrap();
    visual.simulate_click(first.center(), Default::default());
    click(visual, "native-terminal-output");
    visual.simulate_input("中文-one");
    visual.simulate_keystrokes("enter");
    driver.wait(&mut manager, &app, visual, |visual| {
        painted(&app, visual).contains("ANSWER-task-a:中文-one")
    });
    assert!(!painted(&app, visual).contains("ANSWER-task-b"));
    // Repeated active Run locates this same process and tab, even when another config is selected.
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.start_configuration_without_environment("task-a", window, cx)
        })
    });
    driver.frame(&mut manager, &app, visual);
    assert_eq!(
        manager
            .executions()
            .iter()
            .filter(|session| session.snapshot().state.is_active())
            .count(),
        2
    );
    let old_execution = visual.update(|_, cx| {
        app.read(cx).terminal.read(cx).sessions[0]
            .task
            .as_ref()
            .unwrap()
            .execution
            .unwrap()
    });
    // The real Rerun button waits for old native exit, then reuses this tab with one new child.
    click(visual, "terminal-task-rerun");
    driver.wait(&mut manager, &app, visual, |visual| {
        visual.update(|_, cx| {
            app.read(cx).terminal.read(cx).sessions[0]
                .task
                .as_ref()
                .unwrap()
                .execution
                .is_some_and(|id| id != old_execution)
        }) && painted(&app, visual).contains("READY-task-a")
    });
    assert!(
        !manager
            .execution(old_execution)
            .unwrap()
            .snapshot()
            .state
            .is_active()
    );
    assert_eq!(
        manager
            .executions()
            .iter()
            .filter(|session| session.snapshot().state.is_active())
            .count(),
        2
    );
    // Hiding has no stop effect; clicking the active configuration restores this same input target.
    visual.update(|_, cx| {
        app.read(cx)
            .terminal
            .clone()
            .update(cx, |panel, cx| panel.set_visible(false, cx))
    });
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.start_configuration_without_environment("task-a", window, cx)
        })
    });
    driver.frame(&mut manager, &app, visual);
    assert!(visual.update(|_, cx| app.read(cx).terminal.read(cx).visible()));
    click(visual, "native-terminal-output");
    visual.simulate_keystrokes("ctrl-shift-w");
    driver.frame(&mut manager, &app, visual);
    assert!(visual.debug_bounds("terminal-close-confirmation").is_some());
    click(visual, "terminal-close-cancel");
    driver.frame(&mut manager, &app, visual);
    assert_eq!(
        manager
            .executions()
            .iter()
            .filter(|session| session.snapshot().state.is_active())
            .count(),
        2
    );
    visual.simulate_keystrokes("ctrl-shift-w");
    driver.frame(&mut manager, &app, visual);
    click(visual, "terminal-close-stop");
    driver.wait(&mut manager, &app, visual, |visual| {
        visual.debug_bounds(first_selector).is_none()
    });
    assert_eq!(
        manager
            .executions()
            .iter()
            .filter(|session| session.snapshot().state.is_active())
            .count(),
        1
    );
    configuration(&app, visual, "task-a", "Write-Output 'NEXT-ROUND'");
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.start_configuration_without_environment("task-a", window, cx)
        })
    });
    driver.wait(&mut manager, &app, visual, |visual| {
        painted(&app, visual).contains("NEXT-ROUND")
    });
    let new_id = visual.update(|_, cx| app.read(cx).terminal.read(cx).active.unwrap());
    assert_ne!(new_id, first_id);
    driver.wait(&mut manager, &app, visual, |visual| {
        visual.update(|_, cx| {
            app.read(cx)
                .terminal
                .read(cx)
                .sessions
                .iter()
                .find(|tab| tab.id == new_id)
                .unwrap()
                .exited
        })
    });
    configuration(&app, visual, "task-a", "Write-Output 'REUSED-ROUND'");
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.start_configuration_without_environment("task-a", window, cx)
        })
    });
    driver.wait(&mut manager, &app, visual, |visual| {
        painted(&app, visual).contains("REUSED-ROUND")
    });
    assert_eq!(
        visual.update(|_, cx| app.read(cx).terminal.read(cx).active.unwrap()),
        new_id
    );
    assert!(!painted(&app, visual).contains("NEXT-ROUND"));
    let retained = painted(&app, visual);
    configuration(&app, visual, "task-a", " ");
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.start_configuration_without_environment("task-a", window, cx)
        })
    });
    driver.frame(&mut manager, &app, visual);
    assert!(
        painted(&app, visual).starts_with(&retained),
        "validation rejection keeps the prior round visible"
    );
    manager.shutdown();
}

/// Batch sequence integration at its admitted-plan seam: real builds, failure and start cancellation.
#[gpui::test]
fn builtin_task_preparation_order_failure_build_and_start_cancel(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        root.path().join("plugins"),
        protocol::Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let (app, visual) = super::restore::open(cx, Workspace::open(root.path()).unwrap());
    let mut driver = Driver::default();
    driver.frame(&mut manager, &app, visual);
    for (key, fail, cancel, build_only) in [
        ("ordered", false, false, false),
        ("failed", true, false, false),
        ("cancelled", false, true, false),
        ("build-only", false, false, true),
    ] {
        let cfg: editor_core::RunConfig = serde_json::from_value(serde_json::json!({
            "id":key, "name":key, "target":{"mode":"program","program":"powershell.exe","args":["-NoProfile","-Command","Write-Output 'TARGET_ONLY'"]},
            "build":[{"name":"BUILD_STEP","target":{"kind":"action","target":{"mode":"program","program":"powershell.exe","args":["-NoProfile","-Command",if cancel { "Write-Output 'BUILD_ONLY'; Start-Sleep -Seconds 60" } else if fail { "exit 9" } else { "Write-Output 'BUILD_ONLY'" }]}}}],
            "prelaunch":[{"name":"PRELAUNCH_STEP","target":{"kind":"action","target":{"mode":"program","program":"powershell.exe","args":["-NoProfile","-Command","Write-Output 'PRELAUNCH_ONLY'"]}}}]
        })).unwrap();
        visual.update(|_, cx| {
            app.update(cx, |app, cx| {
                let workspace = app.workspace_key();
                app.run_controls.upsert(cfg.clone(), &workspace).unwrap();
                let request = app.run_controls.begin(key);
                app.terminal.update(cx, |panel, cx| {
                    assert!(panel.begin_task(key, key, request, cx));
                });
                // Admission and fresh form validation are covered by the neighboring production-entry test.
                // This seam exercises the unchanged coordinator with actual PTY children for every step.
                if build_only {
                    let plan = app.run_controls.prepare_build(key, &workspace, 32).unwrap();
                    app.run_controls.begin_build(key, &plan, request);
                } else {
                    let plan = app.run_controls.launch_plan(key, &workspace).unwrap();
                    app.run_controls.begin_sequence(key, plan, request);
                }
                app.drive_preparation(cx);
            })
        });
        if cancel {
            // Close while its creation receipt is still queued, before the first supervisor paint.
            visual.update(|_, cx| {
                app.read(cx).terminal.clone().update(cx, |panel, cx| {
                    let id = panel.active.unwrap();
                    panel.close(id, cx);
                    panel.confirm_close(cx);
                })
            });
        }
        driver.wait(&mut manager, &app, visual, |visual| {
            visual.update(|_, cx| !app.read(cx).run_controls.is_preparing(key))
        });
        let output = painted(&app, visual);
        if !cancel {
            driver.wait(&mut manager, &app, visual, |visual| {
                visual.update(|_, cx| {
                    app.read(cx)
                        .terminal
                        .read(cx)
                        .sessions
                        .iter()
                        .find(|session| session.task.as_ref().is_some_and(|task| task.key == key))
                        .unwrap()
                        .exited
                })
            });
            // Step results are retained in scrollback too. Read both visible pages through the
            // native input/Canvas seam rather than assuming the complete transcript fits one grid.
            let last_page = painted(&app, visual);
            visual.simulate_keystrokes("shift-pageup");
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let output = format!("{}{}", painted(&app, visual), last_page);
            let details = visual.update(|_, cx| {
                format!(
                    "{}; steps={:?}",
                    app.read(cx).status,
                    app.read(cx).run_controls.preparation(key).unwrap().steps()
                )
            });
            let executions = manager
                .executions()
                .iter()
                .map(|session| (session.request().name.clone(), session.snapshot()))
                .collect::<Vec<_>>();
            assert_eq!(output.contains("BUILD_ONLY"), !fail, "{output}");
            if fail {
                // A silent failure is still visible after selection changes; the shared status bar
                // is not the retained record of this task's step outcome.
                assert!(
                    output.contains("9"),
                    "the task must retain its failing exit code: {output}"
                );
            }
            assert_eq!(
                output.contains("TARGET_ONLY"),
                !fail && !build_only,
                "{output}; {details}; {executions:?}"
            );
            assert_eq!(
                output.contains("PRELAUNCH_ONLY"),
                !fail && !build_only,
                "{output}"
            );
            if !fail && !build_only {
                assert!(
                    output.find("BUILD_ONLY").unwrap() < output.find("PRELAUNCH_ONLY").unwrap()
                );
            }
        } else {
            assert!(!output.contains("TARGET_ONLY"));
        }
    }
    assert!(manager.installed.is_empty());
    manager.shutdown();
    assert_eq!(manager.resource_count(), 0);
}
