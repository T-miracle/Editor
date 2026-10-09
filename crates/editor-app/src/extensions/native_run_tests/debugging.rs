//! Native buttons and real Rust/PDB sessions, including pause-scoped inspection and leave decisions.
use super::*;
use crate::extensions::composable_tests::{publish_frame, pump_recording_and_debug};
#[path = "../../../../plugin-runtime/tests/support/debugger_packages.rs"]
mod packages;
use std::time::{Duration, Instant};

/// Click the actual painted hit region; a selector is never treated as a synthetic action dispatch.
fn click(cx: &mut gpui_kit::VisualTestContext, selector: &'static str) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let position = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing native control {selector}"))
        .center();
    cx.simulate_event(gpui::MouseMoveEvent {
        position,
        pressed_button: None,
        modifiers: Default::default(),
    });
    cx.run_until_parked();
    cx.simulate_click(position, Modifiers::default());
    cx.run_until_parked();
}

/// A frame drains real queued work, polls source-owned requests and publishes the production shapes.
fn frame(
    manager: &mut plugin_runtime::Manager,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
    renderer: &mut images::VectorRenderer,
    launches: &mut Vec<(u64, String, u64)>,
    debug: &mut BTreeMap<u64, (String, plugin_runtime::DebugRequest)>,
) {
    let (statuses, stops) = pump_recording_and_debug(manager, app, cx, launches, debug);
    manager.poll();
    publish_frame(manager, renderer, app, cx, launches, statuses, stops);
}

/// Native Debug enters one target, paints its real stack/locals and keeps a paused target on Cancel.
#[gpui::test]
#[ignore = "build rust-debugger through the current SDK and acquire the pinned CodeLLDB VSIX"]
fn native_rust_debug_controls_inspect_and_confirm_a_real_paused_target(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (source, binary) = packages::program(root.path());
    let (mut manager, app, cx) = builtin_fixture(cx, root.path(), binary.to_str().unwrap(), vec![]);
    let debugger = packages::debugger("native-rust-debug");
    manager
        .install(&debugger, debugger.manifest.permissions.clone())
        .unwrap();
    let config = cx.update(|_, cx| {
        app.update(cx, |app, _| {
            let mut config = app.run_controls.selected().unwrap().clone();
            config
                .breakpoints
                .insert(source.to_str().unwrap(), 8)
                .unwrap();
            let id = config.id.clone();
            let key = app.workspace_key();
            app.run_controls.upsert(config, &key).unwrap();
            id
        })
    });
    let mut driver = crate::extensions::native_configuration_tests::Driver::default();
    driver.frame(&mut manager, &app, cx);
    assert!(cx.debug_bounds("plugin-ui-debug-text").is_none());
    click(cx, "run-debug");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        driver.frame(&mut manager, &app, cx);
        if cx.update(|_, cx| {
            matches!(
                app.read(cx).run_controls.debug_state(),
                editor_core::DebugSessionState::Paused { line: 8, .. }
            )
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native Debug did not reach its real breakpoint: {}",
            cx.update(|_, cx| app.read(cx).status.clone())
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        driver.launches.is_empty(),
        "the final debug target must never also run through ordinary execution"
    );
    assert_eq!(manager.live["native-rust-debug"].process_count(), 2);
    assert!(
        cx.debug_bounds("plugin-ui-debug-text").is_none(),
        "debug output must not create a second plugin panel"
    );
    assert!(cx.debug_bounds("native-terminal-output").is_some());
    let before = manager.debug_observations();
    click(cx, "run-debug");
    driver.frame(&mut manager, &app, cx);
    assert_eq!(
        manager.debug_observations().len(),
        before.len(),
        "repeat Debug locates the same target"
    );

    assert!(
        cx.debug_bounds("debug-panel").is_some(),
        "the unified panel is actually painted"
    );
    click(cx, "debug-panel-step-into");
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        driver.frame(&mut manager, &app, cx);
        if cx.update(|_, cx| {
            app.read(cx)
                .run_controls
                .debug_frames()
                .iter()
                .any(|frame| frame.name.contains("calculate"))
                && app
                    .read(cx)
                    .run_controls
                    .selected_debug_frame()
                    .is_some_and(|id| {
                        app.read(cx)
                            .run_controls
                            .debug_variables(id)
                            .iter()
                            .any(|value| value.name == "value" && value.value == "5")
                    })
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native inspection was not populated: {}; {:?}",
            cx.update(|_, cx| app.read(cx).status.clone()),
            manager.debug_observations()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        cx.update(|_, cx| app
            .read(cx)
            .run_controls
            .debug_session()
            .unwrap()
            .0
            .to_owned()),
        config
    );
    let frame_id = cx.update(|_, cx| app.read(cx).run_controls.selected_debug_frame().unwrap());
    // Selectors in GPUI's native harness require a static borrow; this bounded test owns one string.
    let variable_selector =
        Box::leak(format!("run-debug-variable-{frame_id}-value").into_boxed_str());
    assert!(
        cx.debug_bounds(&*variable_selector).is_some(),
        "the reported local is actually painted in the native panel"
    );
    // The actual native leave card treats a paused debugger as owned work, and Cancel keeps it alive.
    assert!(!cx.update(|_, cx| app.update(cx, |app, cx| app.should_close_window(cx))));
    cx.run_until_parked();
    assert!(cx.debug_bounds("run-leave-confirm").is_some());
    click(cx, "run-leave-cancel");
    assert_eq!(manager.live["native-rust-debug"].process_count(), 2);
    click(cx, "debug-panel-stop");
    let deadline = Instant::now() + Duration::from_secs(20);
    while !cx.update(|_, cx| {
        matches!(
            app.read(cx).run_controls.debug_state(),
            editor_core::DebugSessionState::Exited
        )
    }) {
        driver.frame(&mut manager, &app, cx);
        assert!(
            Instant::now() < deadline,
            "native Stop did not release its target tree"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    driver.frame(&mut manager, &app, cx);
    assert!(cx.update(|_, cx| matches!(
        app.read(cx).run_controls.debug_state(),
        editor_core::DebugSessionState::Exited
    )));
    manager.shutdown();
}

/// Actual Canvas focus, IME input and Enter reach the sole debugger-owned target and its real pause.
#[gpui::test]
#[ignore = "build configuration-example/rust-debugger with the public SDK and prepare CodeLLDB"]
fn native_debug_stdin_reaches_the_target_and_inspection_stays_inside_its_tab(
    cx: &mut TestAppContext,
) {
    let root = tempfile::tempdir().unwrap();
    let (source, binary) = packages::interactive_program(root.path());
    let (mut manager, app, cx) = builtin_fixture(cx, root.path(), binary.to_str().unwrap(), vec![]);
    let debugger = packages::debugger("native-stdin-debugger");
    manager
        .install(&debugger, debugger.manifest.permissions.clone())
        .unwrap();
    cx.update(|_, cx| {
        app.update(cx, |app, _| {
            let mut config = app.run_controls.selected().unwrap().clone();
            config
                .breakpoints
                .insert(source.to_str().unwrap(), 4)
                .unwrap();
            app.run_controls
                .upsert(config, &app.workspace_key())
                .unwrap();
        })
    });
    let mut driver = crate::extensions::native_configuration_tests::Driver::default();
    driver.frame(&mut manager, &app, cx);
    click(cx, "run-debug");
    driver.wait(&mut manager, &app, cx, |cx| {
        crate::terminal::tests::painted(&app, cx).contains("WAITING_FOR_INPUT")
    });
    assert!(cx.debug_bounds("plugin-ui-debug-text").is_none());
    let terminal = cx.debug_bounds("native-terminal-output").unwrap();
    let inspection = cx.debug_bounds("debug-panel").unwrap();
    assert!(
        inspection.top() >= terminal.bottom(),
        "inspection belongs below this tab's grid"
    );
    click(cx, "native-terminal-output");
    cx.simulate_input("7");
    cx.simulate_keystrokes("enter");
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.update(|_, cx| {
            matches!(
                app.read(cx).run_controls.debug_state(),
                editor_core::DebugSessionState::Paused { line: 4, .. }
            )
        })
    });
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.update(|_, cx| {
            app.read(cx)
                .run_controls
                .selected_debug_frame()
                .is_some_and(|id| {
                    app.read(cx)
                        .run_controls
                        .debug_variables(id)
                        .iter()
                        .any(|v| v.name == "value" && v.value == "7")
                })
        })
    });
    assert_eq!(
        std::fs::read_to_string(root.path().join("starts.txt"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert!(
        driver.launches.is_empty(),
        "no ordinary execution copy of a debug target"
    );
    click(cx, "debug-panel-resume");
    driver.wait(&mut manager, &app, cx, |cx| {
        crate::terminal::tests::painted(&app, cx).contains("INPUT_RESULT:15")
    });
    click(cx, "debug-panel-stop");
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.update(|_, cx| {
            matches!(
                app.read(cx).run_controls.debug_state(),
                editor_core::DebugSessionState::Exited
            )
        })
    });
    assert_eq!(manager.live["native-stdin-debugger"].process_count(), 0);
    assert!(crate::terminal::tests::painted(&app, cx).contains("INPUT_RESULT:15"));
    manager.shutdown();
}

/// A real guest budget trap is shown by the native error card while an unrelated program stays live.
#[gpui::test]
#[ignore = "build terminal and capability-example through the current public SDK first"]
fn real_wasm_fault_paints_its_native_error_and_preserves_unrelated_work(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (mut manager, app, cx) = fixture(
        cx,
        root.path(),
        "powershell.exe",
        vec![
            "-NoProfile".into(),
            "-Command".into(),
            "Start-Sleep -Seconds 60".into(),
        ],
    );
    let package = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let mut renderer = images::VectorRenderer::default();
    let mut launches = Vec::new();
    let mut debug = BTreeMap::new();
    publish(&mut manager, &mut renderer, &app, cx);
    click(cx, "run-start");
    let deadline = Instant::now() + Duration::from_secs(30);
    while launches.first().is_none_or(|(id, _, _)| {
        manager.execution(*id).unwrap().state() != plugin_runtime::ExecutionState::Running
    }) {
        frame(
            &mut manager,
            &app,
            cx,
            &mut renderer,
            &mut launches,
            &mut debug,
        );
        assert!(
            Instant::now() < deadline,
            "unrelated real execution must start"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let unrelated = launches[0].0;
    cx.update(|_, cx| {
        app.read(cx)
            .extensions
            .read(cx)
            .invoke_command("capability-example", "fault-spin", serde_json::Value::Null)
            .unwrap()
    });
    frame(
        &mut manager,
        &app,
        cx,
        &mut renderer,
        &mut launches,
        &mut debug,
    );
    let retired = &manager.live["capability-example"];
    assert!(
        retired.resource_count() == 0 && retired.views.is_empty(),
        "the real WASM budget trap revokes the source's executable authority and resources"
    );
    assert!(
        manager
            .published_entries()
            .iter()
            .any(|entry| entry.manifest.id == "capability-example" && entry.error.is_some())
    );
    let records = cx.update(|_, cx| {
        app.read(cx)
            .extensions
            .read(cx)
            .runtime_logs()
            .records("capability-example")
    });
    let failure = records
        .iter()
        .find(|record| record.level == plugin_runtime::logs::LogLevel::Error)
        .expect("the actual error survives source retirement");
    assert!(!failure.message.is_empty());
    assert!(cx.debug_bounds("plugin-error-indicator").is_some());
    click(cx, "plugin-error-indicator");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("plugin-status-popup").is_some());
    let selector = Box::leak(format!("plugin-summary-message-{}", failure.id).into_boxed_str());
    assert!(
        cx.debug_bounds(&*selector).is_some(),
        "the original fault diagnostic is actually painted"
    );
    assert_eq!(
        manager.execution(unrelated).unwrap().state(),
        plugin_runtime::ExecutionState::Running
    );
    assert_eq!(launches.len(), 1, "a fault never replays unrelated starts");
    eprintln!("actual native fault message: {}", failure.message);
    manager.shutdown();
}
