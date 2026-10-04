//! The native Run control starts a real program through the public execution contract.
//!
//! This is the vertical acceptance for the run controls: a saved configuration, the real terminal
//! package, the worker's public work item, the host session and the provider's own native panel. No
//! test-only host API is introduced, and the assertions are only about what a user can observe.
#![cfg(windows)]
use super::composable_tests::{publish, publish_with_launches, pump_recording};
use super::*;
use gpui_kit::{TestAppContext, gpui};

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
    let requests = manager
        .live
        .get_mut("terminal")
        .unwrap()
        .take_editor_requests();
    assert!(!requests.is_empty());
    cx.update(|_, cx| {
        app.read(cx).extensions.clone().update(cx, |owner, cx| {
            let mut state = owner.worker.state.lock().unwrap();
            state.editor_requests.extend(
                requests
                    .into_iter()
                    .map(|request| ("terminal".into(), request)),
            );
            drop(state);
            owner.poll(cx);
        });
    });
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
        assert!(state.status.contains("定位会话"), "{}", state.status);
    });

    // Stop asks the session's own provider; the host never terminates a program by itself.
    cx.update(|window, cx| {
        let _ = window;
        app.update(cx, |app, cx| app.stop_selected_run(cx));
    });
    let stop = cx.update(|_, cx| {
        let state = app.read(cx);
        assert!(
            state.run_controls.is_stopping(&sessions[0].config),
            "a stop stays pending until its provider answers"
        );
        state
            .extensions
            .read(cx)
            .worker
            .recorded
            .lock()
            .unwrap()
            .try_iter()
            .find_map(|work| match work {
                Work::StopRun { session, .. } => Some(session),
                _ => None,
            })
    });
    let stopped_session = stop.expect("the stop control asked the worker to stop this session");
    assert_eq!(stopped_session, session);
    // The provider ends the owned program; its own private shell is not this session's program.
    manager.stop_execution(stopped_session).unwrap();
    for _ in 0..60 {
        manager.poll();
        if manager.live["terminal"].process_count() <= 1 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(manager.live["terminal"].process_count(), 1);
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
    let requests = manager
        .live
        .get_mut("terminal")
        .unwrap()
        .take_editor_requests();
    assert!(!requests.is_empty());
    cx.update(|_, cx| {
        app.read(cx).extensions.clone().update(cx, |owner, cx| {
            let mut state = owner.worker.state.lock().unwrap();
            state.editor_requests.extend(
                requests
                    .into_iter()
                    .map(|request| ("terminal".into(), request)),
            );
            drop(state);
            owner.poll(cx);
        });
    });
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
    for _ in 0..20 {
        manager.poll();
        publish_with_launches(&mut manager, &mut renderer, &app, cx, &launches);
        if cx.debug_bounds("plugin-ui-output").is_some() {
            break;
        }
        cx.run_until_parked();
    }
    assert!(
        cx.debug_bounds("plugin-ui-output").is_some(),
        "the session's output is recoverable from the dropdown"
    );
    // Stopping from here still affects only this session, and only through its provider.
    manager.stop_execution(session).unwrap();
    for _ in 0..60 {
        manager.poll();
        if manager.live["terminal"].process_count() <= 1 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(manager.live["terminal"].process_count(), 1);
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
    // The evaluation happened: the same marker carries the interpreter's own arithmetic result.\n    let painted = painted_text(&manager);\n    assert!(painted.contains("SCRIPT_MODE_OK2"), "{painted:?}");
    let stored = cx.update(|_, cx| {
        editor_core::storage_path(&app.read(cx).workspace_key()).expect("host-local path")
    });
    let _ = std::fs::remove_file(stored);
    let _ = config;
}

/// A restricted workspace never starts a program, whatever the stored configuration says.
#[gpui::test]
fn a_restricted_workspace_refuses_to_launch_from_the_run_control(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (mut manager, app, cx) =
        fixture(cx, root.path(), "powershell.exe", vec!["-NoProfile".into()]);
    // Host-local authority is withdrawn before the launch is attempted.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            let extensions = app.extensions.clone();
            extensions.update(cx, |panel, cx| panel.set_workspace_trusted(false, cx));
            app.start_selected_run(window, cx);
        });
    });
    let mut launches = Vec::new();
    pump_recording(&mut manager, &app, cx, &mut launches);
    assert!(launches.is_empty(), "a restricted workspace starts nothing");
    cx.update(|_, cx| {
        let state = app.read(cx);
        assert!(state.status.contains("受限工作区"), "{}", state.status);
        assert!(state.run_controls.sessions().is_empty());
    });
}
