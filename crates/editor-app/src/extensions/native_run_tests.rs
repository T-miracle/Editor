//! The native Run control starts a real program through the public execution contract.
//!
//! This is the vertical acceptance for the run controls: a saved configuration, the real terminal
//! package, the worker's public work item, the host session and the provider's own native panel. No
//! test-only host API is introduced, and the assertions are only about what a user can observe.
#![cfg(windows)]
use super::composable_tests::{publish, publish_with_launches, pump_recording};
use super::*;
use gpui_kit::{TestAppContext, gpui};

// Debug acceptance shares this native workspace fixture, while its deferred driver stays separate.
mod debugging;
mod sharing;

/// Rerun waits for actual old-program exit, then follows the ordinary save and preparation path once.
#[gpui::test]
#[ignore = "build the terminal package with the current public SDK first"]
fn native_rerun_replaces_the_old_program_only_after_it_ends(cx: &mut TestAppContext) {
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
    let mut renderer = images::VectorRenderer::default();
    let mut launches = Vec::new();
    cx.update(|window, cx| app.update(cx, |app, cx| app.start_selected_run(window, cx)));
    for _ in 0..250 {
        pump_recording(&mut manager, &app, cx, &mut launches);
        manager.poll();
        publish_with_launches(&mut manager, &mut renderer, &app, cx, &launches);
        if launches.first().is_some_and(|(id, _, _)| {
            manager.execution(*id).unwrap().snapshot().state
                == plugin_runtime::ExecutionState::Running
        }) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(launches.len(), 1);
    let original = launches[0].0;
    cx.update(|window, cx| app.update(cx, |app, cx| app.rerun_selected(window, cx)));
    // The provider's force-confirmation timeout follows the normal grace period; native painting
    // also consumes time, so the acceptance deadline covers both bounds rather than counting frames.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let mut observations = Vec::new();
    let mut last = None;
    while std::time::Instant::now() < deadline {
        pump_recording(&mut manager, &app, cx, &mut launches);
        if launches.len() > 1 {
            assert_eq!(
                manager.execution(original).unwrap().state(),
                plugin_runtime::ExecutionState::Exited,
                "the replacement must never overlap the original program"
            );
            break;
        }
        manager.poll();
        let current = (
            manager.execution(original).unwrap().state(),
            manager.live["terminal"].process_count(),
        );
        if last != Some(current) {
            let query = manager.query_execution(original).map(|completion| {
                manager.poll_request(&completion);
                completion.status()
            });
            observations.push(format!("{current:?}: {query:?}"));
            last = Some(current);
        }
        publish_with_launches(&mut manager, &mut renderer, &app, cx, &launches);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let visible_status = cx.update(|_, cx| app.read(cx).status.clone());
    assert_eq!(
        launches.len(),
        2,
        "one explicit rerun must eventually create one replacement; status={visible_status}; original={:?}; native={:?}; observations={observations:?}",
        manager.execution(original).unwrap().snapshot(),
        manager.live["terminal"].process_ids()
    );
    manager.shutdown();
}

/// Native session selection restores a hidden tab and displays that program's output, not another tab.
#[gpui::test]
#[ignore = "build the terminal package with the current public SDK first"]
fn native_session_location_selects_the_exact_hidden_program(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (mut manager, app, cx) = fixture(
        cx,
        root.path(),
        "powershell.exe",
        vec![
            "-NoProfile".into(),
            "-Command".into(),
            "[Console]::OutputEncoding=[Text.UTF8Encoding]::new(); Write-Output 'LOCATION_A'; $line=[Console]::ReadLine(); Write-Output ('ECHO_A:'+$line); Start-Sleep -Seconds 60".into(),
        ],
    );
    let first = cx.update(|_, cx| app.read(cx).run_controls.selected().unwrap().id.clone());
    let second = cx.update(|_, cx| {
        app.update(cx, |app, _| {
            let mut config = app.run_controls.selected().unwrap().clone();
            let key = app.workspace_key();
            config.id = app.run_controls.generate_id(&key);
            config.name = "Second location".into();
            config.target = editor_core::RunTarget::Program {
                program: "powershell.exe".into(),
                args: vec![
                    "-NoProfile".into(),
                    "-Command".into(),
                    "[Console]::OutputEncoding=[Text.UTF8Encoding]::new(); Write-Output 'LOCATION_B'; $line=[Console]::ReadLine(); Write-Output ('ECHO_B:'+$line); Start-Sleep -Seconds 60".into(),
                ],
            };
            let id = config.id.clone();
            app.run_controls.upsert(config, &key).unwrap();
            id
        })
    });
    let mut renderer = images::VectorRenderer::default();
    let mut launches = Vec::new();
    for (config, marker) in [(&first, "LOCATION_A"), (&second, "LOCATION_B")] {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.start_configuration_without_environment(config, window, cx)
            })
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while std::time::Instant::now() < deadline {
            pump_recording(&mut manager, &app, cx, &mut launches);
            manager.poll();
            publish_with_launches(&mut manager, &mut renderer, &app, cx, &launches);
            if painted_text(&manager).contains(marker) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(painted_text(&manager).contains(marker));
    }
    let a = launches
        .iter()
        .find(|(_, config, _)| config == &first)
        .unwrap()
        .0;
    let b = launches
        .iter()
        .find(|(_, config, _)| config == &second)
        .unwrap()
        .0;
    type_in_native_program(
        &mut manager,
        &mut renderer,
        &app,
        cx,
        &mut launches,
        "中文 B",
        "ECHO_B:中文B",
    );
    let provider_a = manager
        .execution(a)
        .unwrap()
        .snapshot()
        .provider_session
        .unwrap();
    let revision = manager.live["terminal"].views["terminal"].revision;
    // Close uses the ordinary public native collection event; managed programs hide rather than die.
    manager
        .event(
            "terminal",
            Some("terminal".into()),
            protocol::api::Notification::Ui(protocol::ui::UiEvent {
                revision,
                node: "sessions".into(),
                action: protocol::ui::Action::Close(provider_a.clone()),
            }),
        )
        .unwrap();
    assert!(painted_text(&manager).contains("LOCATION_B"));
    cx.update(|window, cx| app.update(cx, |app, cx| app.reveal_run_session(a, &first, window, cx)));
    for _ in 0..30 {
        pump_recording(&mut manager, &app, cx, &mut launches);
        manager.poll();
        publish_with_launches(&mut manager, &mut renderer, &app, cx, &launches);
        if painted_text(&manager).contains("LOCATION_A") {
            break;
        }
        cx.run_until_parked();
    }
    assert!(
        painted_text(&manager).contains("LOCATION_A"),
        "native selection must locate the actual retained session"
    );
    assert!(!painted_text(&manager).contains("LOCATION_B"));
    assert_eq!(manager.executions().len(), 2);
    assert!(
        manager
            .executions()
            .iter()
            .all(|session| session.state() == plugin_runtime::ExecutionState::Running)
    );
    assert!(cx.debug_bounds("plugin-ui-output").is_some());
    type_in_native_program(
        &mut manager,
        &mut renderer,
        &app,
        cx,
        &mut launches,
        "中文 A",
        "ECHO_A:中文A",
    );
    assert!(
        !painted_text(&manager).contains("ECHO_B:"),
        "input/output stays with each independent session"
    );
    cx.update(|_, cx| app.update(cx, |app, cx| app.stop_selected_run(cx)));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while manager.execution(a).unwrap().state().is_active() && std::time::Instant::now() < deadline
    {
        pump_recording(&mut manager, &app, cx, &mut launches);
        manager.poll();
        publish_with_launches(&mut manager, &mut renderer, &app, cx, &launches);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        manager.execution(a).unwrap().state(),
        plugin_runtime::ExecutionState::Exited
    );
    assert_eq!(
        manager.execution(b).unwrap().state(),
        plugin_runtime::ExecutionState::Running
    );
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.reveal_run_session(b, &second, window, cx))
    });
    pump_recording(&mut manager, &app, cx, &mut launches);
    manager.poll();
    publish_with_launches(&mut manager, &mut renderer, &app, cx, &launches);
    assert!(
        painted_text(&manager).contains("ECHO_B:中文B"),
        "reopened output retains this program's own history"
    );
    manager.shutdown();
}

/// Native typing goes through the currently selected canvas and its public event stream.
fn type_in_native_program(
    manager: &mut plugin_runtime::Manager,
    renderer: &mut images::VectorRenderer,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
    launches: &mut Vec<(u64, String, u64)>,
    text: &str,
    expected: &str,
) {
    let bounds = cx
        .debug_bounds("plugin-ui-output")
        .expect("the selected native output is visible");
    cx.simulate_click(bounds.center(), Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_input(text);
    cx.simulate_keystrokes("enter");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        pump_recording(manager, app, cx, launches);
        manager.poll();
        publish_with_launches(manager, renderer, app, cx, launches);
        if painted_text(manager).contains(expected) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!(
        "native input did not reach its selected program: expected {expected:?}; painted={:?}",
        painted_text(manager)
    );
}

/// Install the real terminal package and open the editor on a workspace with one saved configuration.
fn fixture<'a>(
    cx: &'a mut TestAppContext,
    root: &std::path::Path,
    program: &str,
    args: Vec<String>,
) -> (
    plugin_runtime::Manager,
    Entity<EditorApp>,
    &'a mut gpui_kit::VisualTestContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let mut manager = plugin_runtime::Manager::open(
        root.join("runtime"),
        protocol::Environment {
            workspace: root.display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let terminal = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/terminal.zip"),
    )
    .unwrap();
    let grants = terminal.manifest.permissions.clone();
    manager.install(&terminal, grants).unwrap();

    // The configuration is created with the same rules the editor uses to create one.
    let key = root.display().to_string();
    let mut set = editor_core::RunConfigSet::default();
    let id = set.generate_id(&key);
    set.upsert(editor_core::RunConfig {
        id: id.clone(),
        name: "验收配置".into(),
        target: editor_core::RunTarget::Program {
            program: program.into(),
            args,
        },
        directory: None,
        env: Default::default(),
        tool_paths: Default::default(),
        build: Default::default(),
        prelaunch: Default::default(),
        source: editor_core::RunConfigSource::Local,
        from_target: None,
        provider: None,
        breakpoints: Default::default(),
        local: true,
    })
    .unwrap();
    set.select(&id);

    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let workspace = Workspace::open(root).unwrap();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        // The configuration is installed through the same store the title bar reads.
        app.update(cx, |app, cx| {
            let key = app.workspace_key();
            // Keep real acceptance writes under this fixture rather than the user configuration root.
            app.run_controls = crate::run::RunControls::load_with_project(
                &key,
                Some(app.workspace.root().join("private-runs")),
                Some(app.workspace.root().into()),
            );
            app.run_controls
                .upsert(set.configurations[0].clone(), &key)
                .unwrap();
            app.run_controls.select(&id, &key);
            cx.notify();
        });
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    cx.simulate_resize(size(px(1400.), px(900.)));
    let mut renderer = images::VectorRenderer::default();
    publish(&mut manager, &mut renderer, &app, cx);
    (manager, app, cx)
}

/// Stop through the same native work/publication route and wait for observed process cleanup.
/// The terminal's private interactive shell remains independent of this run session.
fn stop_and_observe(
    manager: &mut plugin_runtime::Manager,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
    renderer: &mut images::VectorRenderer,
    launches: &mut Vec<(u64, String, u64)>,
    session: u64,
) {
    cx.update(|_, cx| app.update(cx, |app, cx| app.stop_selected_run(cx)));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let (statuses, stops) = pump_recording(manager, app, cx, launches);
        manager.poll();
        super::composable_tests::publish_frame(
            manager, renderer, app, cx, launches, statuses, stops,
        );
        if manager.execution(session).unwrap().snapshot().state
            == plugin_runtime::ExecutionState::Exited
            && manager.live["terminal"].process_count() == 1
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "native Stop did not finish cleanup"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(manager.live["terminal"].process_count(), 1);
}

/// Run from the title bar produces a real terminal session for the configuration's literal command.
#[gpui::test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn native_run_control_starts_a_real_program_and_shows_its_session(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (mut manager, app, cx) = fixture(
        cx,
        root.path(),
        "powershell.exe",
        vec![
            "-NoProfile".into(),
            "-Command".into(),
            "[Console]::Write('NATIVE_RUN_OK'); Start-Sleep -Seconds 60".into(),
        ],
    );
    let mut renderer = images::VectorRenderer::default();
    // The provider's panel starts hidden; running is what makes it visible.
    assert!(cx.debug_bounds("plugin-ui-output").is_none());

    // Clicking Run is the only entry point used; the request travels the worker's own work item.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.start_selected_run(window, cx));
    });
    let mut launches = Vec::new();
    let mut session = None;
    for _ in 0..200 {
        pump_recording(&mut manager, &app, cx, &mut launches);
        manager.poll();
        publish_with_launches(&mut manager, &mut renderer, &app, cx, &launches);
        if let Some((id, _, _)) = launches.first()
            && manager
                .execution(*id)
                .is_some_and(|session| session.snapshot().provider_session.is_some())
        {
            session = Some(*id);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let session = session.expect("the run control requested one host session");

    // The request was made for the configuration's literal command, not a composed command line.
    let request = manager.execution(session).unwrap().request().clone();
    assert_eq!(request.program, "powershell.exe");
    assert_eq!(
        request.args,
        vec![
            "-NoProfile".to_owned(),
            "-Command".to_owned(),
            "[Console]::Write('NATIVE_RUN_OK'); Start-Sleep -Seconds 60".to_owned()
        ]
    );
    // The launch directory is the workspace root. Windows reports the canonical extended form, so
    // the comparison ignores that platform prefix rather than duplicating it in the expectation.
    let cwd = request
        .cwd
        .clone()
        .expect("a launch always carries a directory");
    assert_eq!(
        cwd.trim_start_matches(r"\\?\"),
        root.path().display().to_string()
    );
    assert_eq!(request.name.as_deref(), Some("验收配置"));

    // The provider owns its presentation and asks the editor for its ordinary terminal panel.
    // Publication already delivers the real provider's presentation request, as production does.
    // Observe its painted result instead of draining the same queue a second time.
    for _ in 0..40 {
        publish(&mut manager, &mut renderer, &app, cx);
        if cx.debug_bounds("plugin-ui-output").is_some() {
            break;
        }
        cx.run_until_parked();
    }
    // The real program is running in a session the user can see.
    assert_eq!(manager.live["terminal"].process_count(), 2);
    assert!(cx.debug_bounds("plugin-ui-output").is_some());

    // The editor adopted that session and reports it as running rather than as a finished request.
    let sessions = cx.update(|_, cx| app.read(cx).run_controls.sessions());
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, session);
    assert!(sessions[0].is_active());
    assert!(sessions[0].failure.is_none());
    assert_eq!(sessions[0].plugin, "terminal");
    assert!(cx.debug_bounds("run-session-state").is_some());

    // A second launch of the same configuration resolves to the running session.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.start_selected_run(window, cx));
    });
    let mut repeats = Vec::new();
    pump_recording(&mut manager, &app, cx, &mut repeats);
    assert!(
        repeats.is_empty(),
        "a repeat launch must not start a second program"
    );
    cx.update(|_, cx| {
        let state = app.read(cx);
        assert_eq!(state.run_controls.sessions().len(), 1);
        assert!(
            state.status.contains("定位会话")
                || state.status.contains("正在定位")
                || state.status.contains("Located session")
                || state.status.contains("Locating session"),
            "{}",
            state.status
        );
    });

    // Drive normal Stop through the native controller and await the actual owned-program barrier.
    stop_and_observe(
        &mut manager,
        &app,
        cx,
        &mut renderer,
        &mut launches,
        session,
    );
    // Replacing a running instance is its own action rather than an effect of clicking Run again.
    assert!(cx.debug_bounds("run-rerun").is_some());

    // Owned termination follows the provider's lifetime; no session outlives it as running.
    manager.disable("terminal").unwrap();
    for _ in 0..60 {
        manager.poll();
        publish(&mut manager, &mut renderer, &app, cx);
        if cx.update(|_, cx| app.read(cx).run_controls.active_sessions().is_empty()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(cx.update(|_, cx| app.read(cx).run_controls.active_sessions().is_empty()));
    // Retiring the provider removes its surface; the plugin runtime suite asserts that the owned
    // program itself is terminated with it.
    for _ in 0..20 {
        publish(&mut manager, &mut renderer, &app, cx);
        if cx.debug_bounds("plugin-ui-output").is_none() {
            break;
        }
        cx.run_until_parked();
    }
    assert!(cx.debug_bounds("plugin-ui-output").is_none());
}

/// Leaving with a running program stops it through the provider before the host shuts down.
#[gpui::test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn leaving_stops_every_run_session_before_shutdown(cx: &mut TestAppContext) {
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
    let mut renderer = images::VectorRenderer::default();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.start_selected_run(window, cx));
    });
    let mut launches = Vec::new();
    let mut session = None;
    for _ in 0..200 {
        pump_recording(&mut manager, &app, cx, &mut launches);
        manager.poll();
        publish_with_launches(&mut manager, &mut renderer, &app, cx, &launches);
        if let Some((id, _, _)) = launches.first()
            && manager
                .execution(*id)
                .is_some_and(|session| session.snapshot().provider_session.is_some())
        {
            session = Some(*id);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let session = session.expect("the run control started one session");
    assert_eq!(manager.live["terminal"].process_count(), 2);

    // The close is refused while a decision is pending, then confirmed by the user.
    let refused = cx.update(|_, cx| app.update(cx, |app, cx| app.should_close_window(cx)));
    assert!(!refused);
    cx.update(|_, cx| app.update(cx, |app, cx| app.confirm_leave(cx)));

    // Every active session was asked to stop through its own provider, not terminated by the host.
    let stops = cx.update(|_, cx| {
        app.read(cx)
            .extensions
            .read(cx)
            .worker
            .recorded
            .lock()
            .unwrap()
            .try_iter()
            .filter_map(|work| match work {
                Work::StopRun { session, .. } => Some(session),
                _ => None,
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(stops, vec![session]);
    // The configuration this test stored host-locally is removed with it.
    let stored = cx.update(|_, cx| {
        editor_core::storage_path(&app.read(cx).workspace_key()).expect("host-local path")
    });
    let _ = std::fs::remove_file(stored);
}

/// Hiding a session's output only hides it: the program keeps running and the session stays active.
#[gpui::test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn hiding_a_session_output_does_not_stop_its_program(cx: &mut TestAppContext) {
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
    let mut renderer = images::VectorRenderer::default();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.start_selected_run(window, cx));
    });
    let mut launches = Vec::new();
    let mut session = None;
    for _ in 0..200 {
        pump_recording(&mut manager, &app, cx, &mut launches);
        manager.poll();
        publish_with_launches(&mut manager, &mut renderer, &app, cx, &launches);
        if let Some((id, _, _)) = launches.first()
            && manager
                .execution(*id)
                .is_some_and(|session| session.snapshot().provider_session.is_some())
        {
            session = Some(*id);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let session = session.expect("one session was started");
    // The provider asked the editor for its ordinary panel; answering is what makes it appear.
    // Publication already delivers the real provider's presentation request, as production does.
    // Observe its painted result instead of draining the same queue a second time.
    for _ in 0..20 {
        publish_with_launches(&mut manager, &mut renderer, &app, cx, &launches);
        if cx.debug_bounds("plugin-ui-output").is_some() {
            break;
        }
        cx.run_until_parked();
    }
    let running = manager.live["terminal"].process_count();
    assert_eq!(running, 2, "the program and the provider's own shell");
    assert!(cx.debug_bounds("plugin-ui-output").is_some());

    // Hiding the output is a presentation choice, made through the host's own panel path.
    cx.update(|_, cx| {
        app.update(cx, |app, cx| {
            app.hide_plugin_panel("terminal", "terminal", cx)
        });
    });
    for _ in 0..20 {
        manager.poll();
        publish_with_launches(&mut manager, &mut renderer, &app, cx, &launches);
        if cx.debug_bounds("plugin-ui-output").is_none() {
            break;
        }
        cx.run_until_parked();
    }
    assert!(cx.debug_bounds("plugin-ui-output").is_none());
    // The program is untouched by hiding: nothing was terminated and the session is still active.
    assert_eq!(manager.live["terminal"].process_count(), running);
    assert!(
        manager.execution(session).unwrap().stoppable(),
        "a hidden session is still a running program"
    );
    let sessions = cx.update(|_, cx| app.read(cx).run_controls.sessions());
    assert_eq!(sessions.len(), 1);
    assert!(sessions[0].is_active());

    // The dropdown offers that session again, and selecting it brings its output back.
    assert!(
        cx.update(|_, cx| app.read(cx).run_controls.menu_entries().iter().any(
            |entry| matches!(entry, crate::run::RunMenuEntry::Session { id, .. } if *id == session)
        ))
    );
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.reveal_run_session(session, &sessions[0].config, window, cx);
        });
    });
    // Locate is now a public provider operation; deliver it through the worker before expecting
    // the hidden native panel to reappear. Polling state alone never executes that queued operation.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let (statuses, stops) = pump_recording(&mut manager, &app, cx, &mut launches);
        manager.poll();
        super::composable_tests::publish_frame(
            &mut manager,
            &mut renderer,
            &app,
            cx,
            &launches,
            statuses,
            stops,
        );
        if cx.debug_bounds("plugin-ui-output").is_some() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the session's output is recoverable from the dropdown"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    // Restoring presentation does not change which owned program the native Stop addresses.
    stop_and_observe(
        &mut manager,
        &app,
        cx,
        &mut renderer,
        &mut launches,
        session,
    );
    let stored = cx.update(|_, cx| {
        editor_core::storage_path(&app.read(cx).workspace_key()).expect("host-local path")
    });
    let _ = std::fs::remove_file(stored);
}

/// Two independent configurations run side by side, each keeping its own session.
#[gpui::test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn two_configurations_run_concurrently_with_their_own_sessions(cx: &mut TestAppContext) {
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
    // A second configuration differs only in which program it starts.
    let second = cx.update(|_, cx| {
        app.update(cx, |app, cx| {
            let key = app.workspace_key();
            let id = app.run_controls.generate_id(&key);
            let configuration = editor_core::RunConfig {
                id: id.clone(),
                name: "第二个程序".into(),
                target: editor_core::RunTarget::Program {
                    program: "cmd.exe".into(),
                    args: vec!["/c".into(), "ping -n 60 127.0.0.1 > NUL".into()],
                },
                directory: None,
                env: Default::default(),
                tool_paths: Default::default(),
                build: Default::default(),
                prelaunch: Default::default(),
                source: editor_core::RunConfigSource::Local,
                from_target: None,
                provider: None,
                breakpoints: Default::default(),
                local: true,
            };
            app.run_controls.upsert(configuration, &key).unwrap();
            cx.notify();
            id
        })
    });
    let mut renderer = images::VectorRenderer::default();
    let mut launches = Vec::new();
    // Both configurations are started from the same control, one after the other.
    let first_id = cx.update(|_, cx| app.read(cx).run_controls.selected().unwrap().id.clone());
    for config in [first_id.clone(), second.clone()] {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.start_configuration_without_environment(&config, window, cx)
            });
        });
        for _ in 0..200 {
            pump_recording(&mut manager, &app, cx, &mut launches);
            manager.poll();
            publish_with_launches(&mut manager, &mut renderer, &app, cx, &launches);
            if launches.iter().any(|(id, _, _)| {
                manager
                    .execution(*id)
                    .is_some_and(|session| session.snapshot().provider_session.is_some())
            }) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    // Two host sessions exist for two different literal commands.
    assert_eq!(
        launches.len(),
        2,
        "each configuration starts its own program"
    );
    let commands = launches
        .iter()
        .map(|(id, _, _)| manager.execution(*id).unwrap().request().program.clone())
        .collect::<Vec<_>>();
    assert!(commands.contains(&"powershell.exe".to_owned()));
    assert!(commands.contains(&"cmd.exe".to_owned()));
    // Both are active in the editor, each joined to its own configuration.
    let sessions = cx.update(|_, cx| app.read(cx).run_controls.sessions());
    assert_eq!(sessions.len(), 2);
    assert!(sessions.iter().all(|session| session.is_active()));
    let configs = sessions
        .iter()
        .map(|session| session.config.clone())
        .collect::<Vec<_>>();
    assert!(configs.contains(&first_id));
    assert!(configs.contains(&second));
    // Starting the same configuration again only locates its session.
    let before = launches.len();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.start_configuration_without_environment(&second, window, cx);
        });
    });
    let mut repeats = Vec::new();
    pump_recording(&mut manager, &app, cx, &mut repeats);
    assert_eq!(launches.len(), before, "a repeat launch starts nothing new");
    assert!(repeats.is_empty());
    let stored = cx.update(|_, cx| {
        editor_core::storage_path(&app.read(cx).workspace_key()).expect("host-local path")
    });
    let _ = std::fs::remove_file(stored);
}

/// Collect the glyphs a provider painted, which is how a program's output becomes observable.
///
/// A provider that is not running yet has painted nothing, so absence is empty text rather than a
/// failure: the caller is waiting for output, not for the provider's existence.
fn painted_text(manager: &plugin_runtime::Manager) -> String {
    use plugin_runtime::plugin_protocol::{Paint, ui::Kind};
    let mut text = String::new();
    let Some(scene) = manager
        .live
        .get("terminal")
        .and_then(|instance| instance.views.get("terminal"))
    else {
        return text;
    };
    scene.root.visit(&mut |node| {
        if let Kind::Canvas(canvas) = &node.kind {
            for paint in &canvas.paint {
                if let Paint::Text { text: glyphs, .. } = paint {
                    text.push_str(glyphs);
                }
            }
        }
    });
    text
}

/// A configuration's environment reaches the program it starts, through the public contract.
#[gpui::test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn a_configuration_environment_reaches_the_program_it_starts(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (mut manager, app, cx) = fixture(
        cx,
        root.path(),
        "powershell.exe",
        vec![
            "-NoProfile".into(),
            "-Command".into(),
            "[Console]::Write($env:RDB_PROBE); Start-Sleep -Seconds 60".into(),
        ],
    );
    let mut renderer = images::VectorRenderer::default();
    // The entry names what the program should see; no shell quoting is involved.
    let config = cx.update(|_, cx| app.read(cx).run_controls.selected().unwrap().id.clone());
    let mut launches = Vec::new();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.start_configuration(
                &config,
                vec![plugin_runtime::RunEnvEntry {
                    name: "RDB_PROBE".into(),
                    value: "ENV_REACHED_CHILD".into(),
                }],
                window,
                cx,
            );
        });
    });
    let mut session = None;
    for _ in 0..200 {
        pump_recording(&mut manager, &app, cx, &mut launches);
        manager.poll();
        publish_with_launches(&mut manager, &mut renderer, &app, cx, &launches);
        if let Some((id, _, _)) = launches.first()
            && painted_text(&manager).contains("ENV_REACHED_CHILD")
        {
            session = Some(*id);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let session = session.expect("the program printed the environment value it was given");
    // The request carried exactly that entry, and nothing else was added for it.
    let request = manager.execution(session).unwrap().request().clone();
    assert_eq!(request.env.len(), 1);
    assert_eq!(request.env[0].name, "RDB_PROBE");
    assert_eq!(request.env[0].value, "ENV_REACHED_CHILD");
    assert!(painted_text(&manager).contains("ENV_REACHED_CHILD"));

    // The configuration itself gains tool directories; its own entries stay authoritative for the
    // program, and this launch adds one entry of its own beside them.
    let tool_root = root.path().join("tools");
    std::fs::create_dir_all(&tool_root).unwrap();
    let tool_config = cx.update(|_, cx| {
        app.update(cx, |app, cx| {
            let key = app.workspace_key();
            let id = app.run_controls.generate_id(&key);
            let mut configuration = app
                .run_controls
                .configurations()
                .first()
                .cloned()
                .expect("one configuration exists");
            configuration.id = id.clone();
            configuration.name = "工具路径".into();
            configuration.tool_paths = vec![tool_root.display().to_string()];
            app.run_controls.upsert(configuration, &key).unwrap();
            cx.notify();
            id
        })
    });
    let before_tools = launches.len();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.start_configuration(
                &tool_config,
                vec![plugin_runtime::RunEnvEntry {
                    name: "RDB_SECOND".into(),
                    value: "ALSO_PRESENT".into(),
                }],
                window,
                cx,
            );
        });
    });
    for _ in 0..200 {
        pump_recording(&mut manager, &app, cx, &mut launches);
        manager.poll();
        publish_with_launches(&mut manager, &mut renderer, &app, cx, &launches);
        if launches.len() > before_tools
            && manager
                .execution(launches.last().unwrap().0)
                .is_some_and(|session| {
                    session.request().env.iter().any(|entry| {
                        entry.name.eq_ignore_ascii_case("PATH")
                            && entry.value.starts_with(&tool_root.display().to_string())
                    })
                })
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        launches.len() > before_tools,
        "the tool-path launch started"
    );
    let tool_request = manager
        .execution(launches.last().unwrap().0)
        .unwrap()
        .request()
        .clone();
    let path = tool_request
        .env
        .iter()
        .find(|entry| entry.name.eq_ignore_ascii_case("PATH"))
        .map(|entry| entry.value.clone())
        .expect("a tool directory becomes the program's search order");
    assert!(
        path.starts_with(&tool_root.display().to_string()),
        "the configuration's own tools are searched first: {path}"
    );
    // The configuration's own entries are not replaced by a launch that adds one of its own.
    assert!(
        tool_request
            .env
            .iter()
            .any(|entry| entry.name == "RDB_SECOND" && entry.value == "ALSO_PRESENT")
    );
    assert_eq!(
        tool_request
            .env
            .iter()
            .filter(|entry| entry.name.eq_ignore_ascii_case("PATH"))
            .count(),
        1,
        "one PATH reaches the program"
    );

    // A launch without the entry does not inherit it from a previous session of the same program.
    let second = cx.update(|_, cx| {
        app.update(cx, |app, cx| {
            let key = app.workspace_key();
            let id = app.run_controls.generate_id(&key);
            app.run_controls
                .upsert(
                    editor_core::RunConfig {
                        id: id.clone(),
                        name: "无环境变量".into(),
                        target: editor_core::RunTarget::Program {
                            program: "powershell.exe".into(),
                            args: vec![
                                "-NoProfile".into(),
                                "-Command".into(),
                                "[Console]::Write('NO_ENV_HERE'); Start-Sleep -Seconds 60".into(),
                            ],
                        },
                        directory: None,
                        env: Default::default(),
                        tool_paths: Default::default(),
                        build: Default::default(),
                        prelaunch: Default::default(),
                        source: editor_core::RunConfigSource::Local,
                        from_target: None,
                        provider: None,
                        breakpoints: Default::default(),
                        local: true,
                    },
                    &key,
                )
                .unwrap();
            cx.notify();
            id
        })
    });
    let before = launches.len();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.start_configuration_without_environment(&second, window, cx);
        });
    });
    for _ in 0..200 {
        pump_recording(&mut manager, &app, cx, &mut launches);
        manager.poll();
        publish_with_launches(&mut manager, &mut renderer, &app, cx, &launches);
        if launches.len() > before && painted_text(&manager).contains("NO_ENV_HERE") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(launches.len() > before, "the second configuration started");
    let second_session = launches.last().unwrap().0;
    assert!(
        manager
            .execution(second_session)
            .unwrap()
            .request()
            .env
            .is_empty(),
        "a configuration without entries starts a program without them"
    );
    let stored = cx.update(|_, cx| {
        editor_core::storage_path(&app.read(cx).workspace_key()).expect("host-local path")
    });
    let _ = std::fs::remove_file(stored);
}

/// A shell-mode configuration is interpreted by the interpreter it names, script text intact.
#[gpui::test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn a_shell_configuration_runs_its_script_through_the_named_interpreter(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (mut manager, app, cx) = fixture(
        cx,
        root.path(),
        "powershell.exe",
        vec![
            "-NoProfile".into(),
            "-Command".into(),
            "Write-Output program".into(),
        ],
    );
    let mut renderer = images::VectorRenderer::default();
    // The draft the environment page and the basic page produce, saved through the normal store.
    let config = cx.update(|_, cx| {
        app.update(cx, |app, cx| {
            let key = app.workspace_key();
            let id = app.run_controls.generate_id(&key);
            let draft = crate::run::RunConfigDraft {
                id: id.clone(),
                name: "脚本模式".into(),
                shell: true,
                program: "powershell.exe".into(),
                arguments: "-NoProfile\n-Command".into(),
                // A pipeline with quoting stays one script body rather than several arguments.
                script: "[Console]::Write('SCRIPT_MODE_OK ' + (1 + 1)); Start-Sleep -Seconds 60"
                    .into(),
                directory: String::new(),
                environment: String::new(),
                tool_paths: String::new(),
                source: editor_core::RunConfigSource::Local,
                from_target: None,
                provided: None,
                provider_build: vec![],
                provider_prelaunch: vec![],
                original_arguments: None,
                provider: None,
                breakpoints: String::new(),
                share: false,
                build: String::new(),
                prelaunch: String::new(),
            };
            app.run_controls
                .upsert(draft.to_config().expect("the draft is valid"), &key)
                .expect("the configuration is stored");
            app.run_controls.select(&id, &key);
            cx.notify();
            id
        })
    });
    let mut launches = Vec::new();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.start_stored_configuration(window, cx));
    });
    let mut session = None;
    for _ in 0..200 {
        pump_recording(&mut manager, &app, cx, &mut launches);
        manager.poll();
        publish_with_launches(&mut manager, &mut renderer, &app, cx, &launches);
        // The paint concatenates the grid's cells, so a space is its own cell rather than part of a
        // run; the marker is what proves the interpreter evaluated the script body.
        if let Some((id, _, _)) = launches.first()
            && painted_text(&manager).contains("SCRIPT_MODE_OK")
        {
            session = Some(*id);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let session = session.unwrap_or_else(|| {
        let status = cx.update(|_, cx| app.read(cx).status.clone());
        let painted = painted_text(&manager);
        panic!("no session; status={status:?} painted={painted:?}")
    });
    // The request names the interpreter and carries the script as one final argument.
    let request = manager.execution(session).unwrap().request().clone();
    assert_eq!(request.program, "powershell.exe");
    assert_eq!(
        request.args,
        vec![
            "-NoProfile".to_owned(),
            "-Command".to_owned(),
            "[Console]::Write('SCRIPT_MODE_OK ' + (1 + 1)); Start-Sleep -Seconds 60".to_owned()
        ]
    );
    // The evaluation happened: the same marker carries the interpreter's own arithmetic result.
    let painted = painted_text(&manager);
    assert!(painted.contains("SCRIPT_MODE_OK2"), "{painted:?}");
    let stored = cx.update(|_, cx| {
        editor_core::storage_path(&app.read(cx).workspace_key()).expect("host-local path")
    });
    let _ = std::fs::remove_file(stored);
    let _ = config;
}

/// A restricted workspace never starts a program, whatever the stored configuration says.
#[gpui::test]
#[ignore = "build configuration examples and terminal package with the public SDK first"]
fn a_restricted_workspace_refuses_to_launch_from_the_run_control(cx: &mut TestAppContext) {
    use super::native_configuration_tests::{Driver, click, edit, open_form};

    let root = tempfile::tempdir().unwrap();
    let (mut manager, app, cx) = super::native_configuration_tests::fixture(cx, root.path());
    let mut driver = Driver::default();
    driver.frame(&mut manager, &app, cx);
    let parent = open_form(&app, cx);
    click(cx, "run-config-add");
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.debug_bounds("run-template-configuration-alpha-program")
            .is_some()
    });
    click(cx, "run-template-configuration-alpha-program");
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.debug_bounds("plugin-ui-name").is_some()
    });
    edit(cx, "plugin-ui-name", "Restricted launch");
    click(cx, "run-config-save");
    *cx = gpui_kit::VisualTestContext::from_window(parent, &cx.cx);
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.update(|_, cx| app.read(cx).run_form.is_none())
    });
    // A real plugin-created, validated configuration reaches the trust guard rather than the
    // retired-format guard. Withdrawing trust must still invalidate its permission to execute.
    assert!(cx.update(|_, cx| {
        let controls = &app.read(cx).run_controls;
        let selected = controls.selected().expect("Save selects the configuration");
        controls
            .plugin_configuration_blocker(&selected.id)
            .is_none()
    }));
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            let extensions = app.extensions.clone();
            extensions.update(cx, |panel, cx| panel.set_workspace_trusted(false, cx));
            app.start_selected_run(window, cx);
        });
    });
    driver.frame(&mut manager, &app, cx);
    assert!(
        driver.launches.is_empty(),
        "a restricted workspace starts nothing"
    );
    cx.update(|_, cx| {
        let state = app.read(cx);
        assert!(
            state.status.contains("受限工作区") || state.status.contains("restricted workspace"),
            "{}",
            state.status
        );
        assert!(state.run_controls.sessions().is_empty());
    });
    manager.shutdown();
}

/// A shared configuration the project carries starts a real program with this machine's overrides.
///
/// Ticket 06's remaining acceptance: the earlier gap was that the editor control had never been shown
/// starting a real program from a *shared* entry. The shared definition is read from the project's own
/// file and the local values from this machine's store, which is what makes the two halves meaningful:
/// the program and its arguments come from the project, while the environment and the tool directory
/// come from here and are not written into the project file.
#[gpui::test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn a_shared_configuration_starts_a_real_program_with_local_values(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let project = tempfile::tempdir().unwrap().keep();
    let workspace = project.display().to_string();
    let mut manager = plugin_runtime::Manager::open(
        project.join("runtime"),
        protocol::Environment {
            workspace: workspace.clone(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let terminal = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/terminal.zip"),
    )
    .unwrap();
    let grants = terminal.manifest.permissions.clone();
    manager.install(&terminal, grants).unwrap();

    // The project's half: what to run, written where a team shares it.
    const MARKER: &str = "SHARED_CONFIG_RAN";
    const ENV_MARKER: &str = "SHARED_CONFIG_ENV";
    let mut shared = editor_core::SharedSet::default();
    shared.upsert(editor_core::SharedConfig {
        id: "shared-run".into(),
        name: "共享运行".into(),
        target: editor_core::RunTarget::Program {
            program: "powershell.exe".into(),
            args: vec![
                "-NoProfile".into(),
                "-Command".into(),
                format!(
                    "[Console]::Write('{MARKER}=' + $env:SHARED_CONFIG_ENV); Start-Sleep -Seconds 60"
                ),
            ],
        },
        directory: Some(editor_core::WORKSPACE_TOKEN.to_owned()),
        build: Default::default(),
        prelaunch: Default::default(),
        breakpoints: Default::default(),
    });
    editor_core::save_shared(&project, &shared).unwrap();

    // This machine's half: the value the program will print, kept out of the project file.
    let local_root = project.join("local");
    let mut mine = editor_core::RunConfigSet::default();
    mine.upsert(editor_core::RunConfig {
        id: "shared-run".into(),
        name: "本机名字".into(),
        target: editor_core::RunTarget::Program {
            program: "powershell.exe".into(),
            args: Vec::new(),
        },
        directory: None,
        env: [(ENV_MARKER.to_owned(), "local-value".to_owned())].into(),
        tool_paths: Default::default(),
        build: Default::default(),
        prelaunch: Default::default(),
        source: editor_core::RunConfigSource::Project,
        from_target: None,
        provider: None,
        breakpoints: Default::default(),
        local: false,
    })
    .unwrap();
    mine.select("shared-run");
    editor_core::save(&local_root, &workspace, &mine).unwrap();
    // The local values must not have travelled into the shared file.
    let written = std::fs::read_to_string(editor_core::project_path(&project)).unwrap();
    assert!(
        !written.contains("local-value"),
        "this machine's values stay out of the project file: {written}"
    );

    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let local = local_root.clone();
    let key = workspace.clone();
    let workspace_handle = Workspace::open(&project).unwrap();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace_handle, None, window, cx));
        // Load exactly as the editor loads a workspace: the project's definitions merged with this
        // machine's overrides, through the same entry point.
        app.update(cx, |app, cx| {
            app.run_controls = crate::run::RunControls::load_with_project(
                &key,
                Some(local),
                Some(project.clone()),
            );
            assert!(app.run_controls.error.is_none());
            // The shared definition is the project's: its name and its program are what the team
            // shared. This machine's file contributes only what cannot be shared, so the name stays
            // the project's even though a different one exists locally.
            assert_eq!(
                app.run_controls
                    .configuration("shared-run")
                    .map(|c| c.name.as_str()),
                Some("共享运行"),
                "the shared name is the project's"
            );
            assert_eq!(
                app.run_controls
                    .configuration("shared-run")
                    .map(|c| c.env.get(ENV_MARKER).map(String::to_owned)),
                Some(Some("local-value".to_owned())),
                "this machine's environment is the local half"
            );
            cx.notify();
        });
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    cx.simulate_resize(size(px(1400.), px(900.)));
    let mut renderer = images::VectorRenderer::default();
    publish(&mut manager, &mut renderer, &app, cx);

    // Running it is the user's action, through the same control as any other configuration.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.start_selected_run(window, cx));
    });
    let mut launches = Vec::new();
    for _ in 0..300 {
        pump_recording(&mut manager, &app, cx, &mut launches);
        manager.poll();
        publish_with_launches(&mut manager, &mut renderer, &app, cx, &launches);
        if !launches.is_empty() {
            break;
        }
    }
    assert_eq!(
        launches.len(),
        1,
        "the shared configuration started one program"
    );
    let id = launches[0].0;
    // The program's own output is the evidence that both halves arrived: the project's program and
    // arguments, and this machine's environment value.
    let shown = {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut text = String::new();
        while std::time::Instant::now() < deadline {
            manager.poll();
            publish_with_launches(&mut manager, &mut renderer, &app, cx, &launches);
            text = manager
                .live
                .get("terminal")
                .and_then(|instance| instance.views.get("terminal"))
                .map(|view| {
                    // The terminal publishes a native grid, so the visible glyphs are what is read:
                    // searching serialized JSON would match text that was never painted.
                    let mut text = String::new();
                    view.as_ref().root.visit(&mut |node| {
                        // The terminal publishes paint commands rather than a document, so this reads
                        // the glyphs that were painted; searching serialized JSON would match text
                        // that never reached the screen.
                        if let protocol::ui::Kind::Canvas(canvas) = &node.kind {
                            for paint in &canvas.paint {
                                if let protocol::Paint::Text { text: painted, .. } = paint {
                                    text.push_str(painted);
                                }
                            }
                        }
                    });
                    text
                })
                .unwrap_or_default();
            if text.contains(MARKER) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        text
    };
    assert!(
        shown.contains(&format!("{MARKER}=local-value")),
        "the project's program ran with this machine's environment: {shown}"
    );
    assert!(manager.execution(id).is_some_and(|s| s.stoppable()));
    let _ = manager.stop_execution(id);
    manager.shutdown();
}
