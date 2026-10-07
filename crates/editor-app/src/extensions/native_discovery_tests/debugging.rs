//! A native combined acceptance flow discovers/builds exact Cargo artifacts and manages two real pauses.
use super::*;
#[path = "../../../../plugin-runtime/tests/support/debugger_packages.rs"]
mod packages;

/// Native controls must route to one selected configuration and preserve unrelated target lifetimes.
#[gpui::test]
#[ignore = "build terminal/rust/rust-debugger and verify the pinned CodeLLDB dependency first"]
fn cargo_artifacts_debug_in_parallel_and_retire_through_native_management(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    rust_project(root.path(), "parallel-debug");
    let source = root.path().join("src/main.rs");
    std::fs::write(&source, PROBE).unwrap();
    let (mut manager, app, cx) = editor_for(cx, root.path());
    let debugger = packages::debugger("parallel-debug-provider");
    manager
        .install(&debugger, debugger.manifest.permissions.clone())
        .unwrap();
    let mut driver = Driver::default();
    driver.frame(&mut manager, &app, cx);
    cx.update(|_, cx| app.update(cx, |state, cx| state.discover_run_targets(cx)));
    driver.wait(&mut manager, &app, cx, |state| {
        state.run_controls.discovered_targets().len() == 2
    });
    let (a, b) = cx.update(|_, cx| {
        app.update(cx, |state, _| {
            let target = state
                .run_controls
                .discovered_targets()
                .iter()
                .find(|target| target.label.ends_with("Debug"))
                .unwrap()
                .id
                .clone();
            let key = state.workspace_key();
            let a = state.run_controls.confirm_target(&target, &key).unwrap();
            let mut first = state.run_controls.configuration(&a).unwrap().clone();
            first.name = "Cargo debug A".into();
            first
                .breakpoints
                .insert(source.to_str().unwrap(), 12)
                .unwrap();
            state.run_controls.upsert(first.clone(), &key).unwrap();
            let b = state.run_controls.generate_id(&key);
            first.id = b.clone();
            first.name = "Cargo debug B".into();
            state.run_controls.upsert(first, &key).unwrap();
            state.run_controls.select(&a, &key);
            (a, b)
        })
    });
    click(cx, "run-debug");
    driver.wait(&mut manager, &app, cx, |state| {
        matches!(
            state.run_controls.debug_state(),
            editor_core::DebugSessionState::Paused { line: 12, .. }
        )
    });
    assert!(
        driver.launches.is_empty(),
        "Cargo preparation must launch only the debugger-owned final target"
    );
    assert!(root.path().join("target/debug/parallel-debug.exe").exists());
    let first_session =
        cx.update(|_, cx| app.read(cx).run_controls.debug_provider_session().unwrap());
    assert_eq!(manager.live["parallel-debug-provider"].process_count(), 1);
    // Starting B then selecting A while B connects must not steal the inspected location on B's stop.
    cx.update(|_, cx| {
        app.update(cx, |state, cx| {
            let key = state.workspace_key();
            state.run_controls.select(&b, &key);
            cx.notify();
        })
    });
    click(cx, "run-debug");
    driver.frame(&mut manager, &app, cx);
    cx.update(|_, cx| {
        app.update(cx, |state, cx| {
            assert!(state.run_controls.select_debug_session(&a));
            cx.notify();
        })
    });
    let inspected = cx.update(|_, cx| app.read(cx).active_path.clone());
    driver.wait(&mut manager, &app, cx, |state| {
        state
            .run_controls
            .debug_session_of(&b)
            .is_some_and(|session| {
                matches!(
                    session.state(),
                    editor_core::DebugSessionState::Paused { line: 12, .. }
                )
            })
    });
    assert_eq!(manager.live["parallel-debug-provider"].process_count(), 2);
    assert_eq!(
        cx.update(|_, cx| app
            .read(cx)
            .run_controls
            .debug_session()
            .unwrap()
            .0
            .to_owned()),
        a
    );
    assert_eq!(
        cx.update(|_, cx| app.read(cx).active_path.clone()),
        inspected
    );
    assert!(cx.update(|_, cx| app.read(cx).run_controls.debug_panel_rows().another_paused));
    let second_session = cx.update(|_, cx| {
        app.read(cx)
            .run_controls
            .debug_session_of(&b)
            .unwrap()
            .provider_session()
            .unwrap()
            .to_owned()
    });
    assert_ne!(first_session, second_session);
    // Into/Over/Out travel the actual native panel keyboard path to real CodeLLDB/PDB pauses.
    // A source line can contain more than one instruction. Each keyboard step must produce
    // a fresh real pause; keep stepping until the call enters calculate rather than guessing.
    for _ in 0..6 {
        let epoch = cx.update(|_, cx| app.read(cx).run_controls.debug_pause_epoch().unwrap());
        click(cx, "debug-panel-state");
        cx.simulate_keystrokes("f11");
        assert!(
            cx.update(|_, cx| app.read(cx).run_controls.debug_action_pending()),
            "the native F11 key must reach the selected debug target; bounds={:?}; status={}",
            cx.debug_bounds("debug-panel-state"),
            cx.update(|_, cx| app.read(cx).status.clone())
        );
        driver.wait(&mut manager, &app, cx, |state| {
            state
                .run_controls
                .debug_pause_epoch()
                .is_some_and(|new| new > epoch)
                && !state.run_controls.debug_action_pending()
                && !state.run_controls.debug_frames().is_empty()
        });
        if cx.update(|_, cx| {
            app.read(cx)
                .run_controls
                .debug_frames()
                .iter()
                .any(|frame| frame.name.contains("calculate"))
        }) {
            break;
        }
    }
    driver.wait(&mut manager, &app, cx, |state| {
        state
            .run_controls
            .debug_frames()
            .iter()
            .any(|frame| frame.name.contains("calculate"))
            && state.run_controls.selected_debug_frame().is_some_and(|id| {
                state
                    .run_controls
                    .debug_variables(id)
                    .iter()
                    .any(|value| value.name == "value" && value.value == "5")
            })
    });
    assert!(
        manager
            .debug_observations()
            .iter()
            .any(|state| state.session == second_session && state.line == Some(12))
    );
    let frames = cx.update(|_, cx| app.read(cx).run_controls.debug_frames().to_vec());
    assert!(frames.len() > 20);
    let selector = Box::leak(format!("debug-panel-frame-{}", frames[0].id).into_boxed_str());
    let before = cx.debug_bounds(&*selector).unwrap().origin.y;
    let pane = cx.debug_bounds("debug-panel-frames").unwrap();
    // The native wheel is sent at the pointer position; hover is observed before scrolling.
    cx.simulate_event(gpui::MouseMoveEvent {
        position: pane.center(),
        pressed_button: None,
        modifiers: Default::default(),
    });
    cx.run_until_parked();
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: pane.center(),
        delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-350.))),
        modifiers: Default::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds(&*selector).unwrap().origin.y < before,
        "the actual wheel scrolls the stack pane"
    );
    click(cx, "debug-panel-state");
    cx.simulate_keystrokes("down");
    let frame = cx.update(|_, cx| app.read(cx).run_controls.selected_debug_frame().unwrap());
    assert_ne!(frame, frames[0].id);
    cx.simulate_keystrokes("up");
    assert_eq!(
        cx.update(|_, cx| app.read(cx).run_controls.selected_debug_frame().unwrap()),
        frames[0].id
    );
    for key in ["f10", "shift-f11"] {
        let epoch = cx.update(|_, cx| app.read(cx).run_controls.debug_pause_epoch().unwrap());
        cx.simulate_keystrokes(key);
        driver.wait(&mut manager, &app, cx, |state| {
            state
                .run_controls
                .debug_pause_epoch()
                .is_some_and(|new| new > epoch)
                && !state.run_controls.debug_action_pending()
        });
    }
    cx.simulate_keystrokes("f5");
    driver.wait(&mut manager, &app, cx, |state| {
        matches!(
            state.run_controls.debug_state(),
            editor_core::DebugSessionState::Running
        )
    });
    assert!(
        manager
            .debug_observations()
            .iter()
            .any(|state| state.session == second_session
                && state.state == plugin_runtime::DebugState::Paused)
    );
    cx.simulate_keystrokes("f6");
    driver.wait(&mut manager, &app, cx, |state| {
        matches!(
            state.run_controls.debug_state(),
            editor_core::DebugSessionState::Paused { .. }
        )
    });
    // Scaling/theme/locale use the same settings contracts, and retain the two live target owners.
    let previous_locale = LocaleRestore(rust_i18n::locale().to_string());
    for (dark, locale, font) in [(true, "en", 24.), (false, "zh-CN", 14.)] {
        rust_i18n::set_locale(locale);
        cx.update(|window, cx| {
            apply_theme(builtin_theme(dark), cx);
            typography::set_font_size(cx, font);
            crate::ui::theme::sync_font_sizes(cx);
            window.refresh();
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("debug-panel-step-into").is_some());
        assert_eq!(manager.live["parallel-debug-provider"].process_count(), 2);
    }
    drop(previous_locale);
    // An unrelated ordinary terminal run must survive the debug provider's confirmed retirement.
    let c = cx.update(|_, cx| {
        app.update(cx, |state, cx| {
            let key = state.workspace_key();
            let mut config = state.run_controls.configuration(&a).unwrap().clone();
            config.id = state.run_controls.generate_id(&key);
            config.name = "Unrelated ordinary run".into();
            config.target = editor_core::RunTarget::Program {
                program: "powershell.exe".into(),
                args: vec![
                    "-NoProfile".into(),
                    "-Command".into(),
                    "Start-Sleep -Seconds 60".into(),
                ],
            };
            config.from_target = None;
            config.build.clear();
            config.breakpoints = Default::default();
            let id = config.id.clone();
            state.run_controls.upsert(config, &key).unwrap();
            state.run_controls.select(&id, &key);
            cx.notify();
            id
        })
    });
    click(cx, "run-start");
    driver.wait(&mut manager, &app, cx, |state| {
        state.run_controls.running_for(&c).is_some()
    });
    let unrelated = driver.launches.last().unwrap().0;
    let open = |app: &Entity<EditorApp>, cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|window, cx| {
            let impact = app
                .read(cx)
                .run_controls
                .plugin_session_impact("parallel-debug-provider", None)
                .summary()
                .unwrap();
            assert!(
                impact.contains("Cargo debug A") && impact.contains("Cargo debug B"),
                "{impact}"
            );
            app.read(cx).extensions.clone().update(cx, |owner, cx| {
                owner.open_remove_dialog(
                    "parallel-debug-provider".into(),
                    false,
                    Some(impact),
                    window,
                    cx,
                )
            });
        })
    };
    open(&app, cx);
    cx.run_until_parked();
    assert!(cx.debug_bounds("plugin-remove-consent").is_some());
    click(cx, "plugin-remove-cancel");
    driver.frame(&mut manager, &app, cx);
    assert_eq!(manager.live["parallel-debug-provider"].process_count(), 2);
    open(&app, cx);
    cx.run_until_parked();
    click(cx, "plugin-remove-preserve");
    driver.frame(&mut manager, &app, cx);
    assert!(!manager.live.contains_key("parallel-debug-provider"));
    // Source revocation queues native retirement; only its observed exit receipt is a terminal state.
    driver.wait(&mut manager, &app, cx, |state| {
        [&a, &b].into_iter().all(|id| {
            state
                .run_controls
                .debug_session_of(id)
                .is_some_and(|session| {
                    matches!(
                        session.state(),
                        editor_core::DebugSessionState::Failed { .. }
                    )
                })
        })
    });
    assert!(
        manager
            .debug_observations()
            .iter()
            .filter(|state| [&first_session, &second_session].contains(&&state.session))
            .all(|state| state.state == plugin_runtime::DebugState::Failed)
    );
    assert_eq!(
        manager.execution(unrelated).unwrap().snapshot().state,
        plugin_runtime::ExecutionState::Running
    );
    assert!(cx.update(|_, cx| {
        matches!(
            app.read(cx)
                .run_controls
                .debug_session_of(&a)
                .unwrap()
                .state(),
            editor_core::DebugSessionState::Failed { .. }
        )
    }));
    assert_eq!(
        driver.launches.len(),
        1,
        "retirement never replays a debug or ordinary target"
    );
    manager.shutdown();
}

/// Failed assertions must not leak the selected locale into another native UI test.
struct LocaleRestore(String);
impl Drop for LocaleRestore {
    fn drop(&mut self) {
        rust_i18n::set_locale(&self.0);
    }
}

/// Recursion supplies a real long stack for wheel verification, with stable unoptimized source lines.
const PROBE: &str = "//! Cargo/PDB parallel acceptance target.\n#[inline(never)]\nfn calculate(value:i32)->i32 {\n    let doubled=value*2;\n    doubled+1\n}\n#[inline(never)]\nfn descend(depth:u32)->i32 {\n    if depth>0 {\n        let value=descend(depth-1);std::hint::black_box(value)\n    } else {\n        calculate(5)\n    }\n}\nfn main() {\n    let result=descend(24);\n    println!(\"RUST_RESULT:{result}\");\n    for _ in 0..6000 {std::thread::sleep(std::time::Duration::from_millis(10));}\n}\n";
