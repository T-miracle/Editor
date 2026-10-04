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
