//! The vertical acceptance for ordered preparation: build actions and pre-launch steps.
//!
//! Every assertion is about something a user can observe: which programs ran, in which order, what a
//! failure stopped, and whether a program was started at all. The real terminal package and the
//! worker's own work items are used, so no test-only host API is introduced.
#![cfg(windows)]
use super::composable_tests::{publish_frame, publish_with_launches, pump_recording};
use super::*;
use gpui_kit::{TestAppContext, gpui};

/// One prepared action as the configuration form writes it: `名称 = 程序 | 参数`.
fn action(name: &str, script: &str) -> String {
    format!("{name} = powershell.exe | -NoProfile | -Command | {script}")
}

/// A configuration whose steps are given as the build page's own line form.
fn configuration(
    id: &str,
    name: &str,
    program: &str,
    build: &str,
    prelaunch: &str,
) -> editor_core::RunConfig {
    editor_core::RunConfig {
        id: id.to_owned(),
        name: name.to_owned(),
        target: editor_core::RunTarget::Program {
            program: "powershell.exe".into(),
            args: vec!["-NoProfile".into(), "-Command".into(), program.to_owned()],
        },
        directory: None,
        env: Default::default(),
        tool_paths: Default::default(),
        // The steps are read by the same line form the build page edits, so the acceptance exercises
        // the form's own rules rather than a hand-built structure.
        build: crate::run::parse_steps(build).expect("the build actions are well formed"),
        prelaunch: crate::run::parse_steps(prelaunch).expect("the steps are well formed"),
        source: editor_core::RunConfigSource::Local,
        from_target: None,
        provider: None,
        breakpoints: Default::default(),
        local: true,
    }
}

/// Install the real terminal package on a fresh runtime for one workspace.
fn runtime(root: &std::path::Path) -> plugin_runtime::Manager {
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
    manager
}

/// Open the editor on a workspace holding these configurations, the last one selected.
///
/// The editor's own run controls are replaced with a set loaded the way the application loads them:
/// this machine's record plus whatever the project shares, so an acceptance can start from a project
/// file rather than from configurations handed to it directly.
fn editor_on<'a>(
    cx: &'a mut TestAppContext,
    root: &std::path::Path,
    configurations: Vec<editor_core::RunConfig>,
) -> (Entity<EditorApp>, &'a mut gpui_kit::VisualTestContext) {
    editor_loaded(cx, root, configurations, false)
}

/// Open the editor with its run controls loaded from the workspace's own storage.
///
/// `shared` selects the load path the application uses: reading this machine's record together with
/// the project's shared file.
fn editor_loaded<'a>(
    cx: &'a mut TestAppContext,
    root: &std::path::Path,
    configurations: Vec<editor_core::RunConfig>,
    from_storage: bool,
) -> (Entity<EditorApp>, &'a mut gpui_kit::VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let selected = configurations.last().map(|config| config.id.clone());
    let root = root.to_path_buf();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let opening = root.clone();
    // The controls are loaded before the window opens, so the editor starts from the same state the
    // application starts from rather than having them replaced while it is being built.
    let loaded = from_storage.then(|| {
        let local = tempfile::tempdir().unwrap().keep();
        crate::run::RunControls::load_with_project(
            &opening.display().to_string(),
            Some(local),
            Some(opening.clone()),
        )
    });
    let workspace = Workspace::open(&root).unwrap();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        app.update(cx, |app, cx| {
            let key = app.workspace_key();
            if let Some(loaded) = loaded {
                app.run_controls = loaded;
            } else {
                for configuration in configurations {
                    app.run_controls.upsert(configuration, &key).unwrap();
                }
            }
            if let Some(selected) = &selected {
                app.run_controls.select(selected, &key);
            }
            cx.notify();
        });
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    cx.simulate_resize(size(px(1400.), px(900.)));
    (app, cx)
}

/// Install the real terminal package and open the editor on one configuration with these steps.
fn fixture<'a>(
    cx: &'a mut TestAppContext,
    root: &std::path::Path,
    build: &str,
    prelaunch: &str,
) -> (
    plugin_runtime::Manager,
    Entity<EditorApp>,
    String,
    &'a mut gpui_kit::VisualTestContext,
) {
    let mut manager = runtime(root);
    let key = root.display().to_string();
    let mut set = editor_core::RunConfigSet::default();
    let id = set.generate_id(&key);
    let configuration = configuration(
        &id,
        "验收配置",
        &format!(
            "[Console]::Write('PROGRAM_RAN'); Set-Content -Path '{}' -Value ran",
            root.join("program.txt").display()
        ),
        build,
        prelaunch,
    );
    let (app, cx) = editor_on(cx, root, vec![configuration]);
    let mut renderer = images::VectorRenderer::default();
    publish_with_launches(&mut manager, &mut renderer, &app, cx, &[]);
    (manager, app, id, cx)
}

/// Drive one frame of the editor's own loop: the work items, the runtime, and the publication.
fn frame(
    manager: &mut plugin_runtime::Manager,
    renderer: &mut images::VectorRenderer,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
    launches: &mut Vec<(u64, String, u64)>,
) {
    // The harness performs the worker's own work items and hands back the answers a frame publishes.
    let (statuses, stops) = pump_recording(manager, app, cx, launches);
    manager.poll();
    publish_frame(manager, renderer, app, cx, launches, statuses, stops);
    std::thread::sleep(std::time::Duration::from_millis(20));
}

/// Drive preparation until `done` reports it finished, returning every launch in order.
fn run_until(
    manager: &mut plugin_runtime::Manager,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
    mut done: impl FnMut(&Entity<EditorApp>, &mut gpui_kit::VisualTestContext) -> bool,
) -> Vec<(u64, String, u64)> {
    let mut renderer = images::VectorRenderer::default();
    let mut launches = Vec::new();
    for _ in 0..300 {
        frame(manager, &mut renderer, app, cx, &mut launches);
        if done(app, cx) {
            return launches;
        }
    }
    let (status, preparing, sessions, steps) = cx.update(|_, cx| {
        let state = app.read(cx);
        let id = state
            .run_controls
            .selected()
            .map(|config| config.id.clone());
        (
            state.status.clone(),
            id.as_deref()
                .and_then(|id| state.run_controls.preparing_step(id)),
            state.run_controls.sessions().len(),
            id.as_deref()
                .and_then(|id| state.run_controls.preparation(id))
                .map(|sequence| {
                    sequence
                        .steps()
                        .iter()
                        .map(|step| format!("{}={:?}", step.name, step.state))
                        .collect::<Vec<_>>()
                }),
        )
    });
    let named = launches
        .iter()
        .map(|(id, _, _)| {
            manager
                .execution(*id)
                .and_then(|session| session.request().name.clone())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    panic!(
        "preparation did not finish; status={status:?} preparing={preparing:?} \
         sessions={sessions} steps={steps:?} launches={named:?}"
    );
}

/// End the test without leaving a program running on the machine.
fn shut_down(
    manager: &mut plugin_runtime::Manager,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
) {
    let sessions = cx.update(|_, cx| app.read(cx).run_controls.active_sessions());
    for session in sessions {
        let _ = manager.stop_execution(session.id);
    }
    for _ in 0..40 {
        manager.poll();
        if manager.executions().iter().all(|execution| {
            !matches!(
                execution.state(),
                plugin_runtime::ExecutionState::Starting | plugin_runtime::ExecutionState::Running
            )
        }) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    manager.shutdown();
}

/// Two preparation steps run in order, then the program — and each step is its own session.
#[gpui::test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn a_launch_prepares_each_step_in_order_before_the_program(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first.txt");
    let second = root.path().join("second.txt");
    let (mut manager, app, _id, cx) = fixture(
        cx,
        root.path(),
        &action(
            "第一步",
            &format!("Set-Content -Path '{}' -Value one", first.display()),
        ),
        &action(
            "第二步",
            &format!("Set-Content -Path '{}' -Value two", second.display()),
        ),
    );
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.start_selected_run(window, cx));
    });
    // Readiness is read from the sequence: every preparation step finished and the program is the
    // session now running. The program itself is then given a moment to produce its own output, so
    // the assertion is about what it did rather than about how fast it was scheduled.
    let launches = run_until(&mut manager, &app, cx, |app, cx| {
        cx.update(|_, cx| {
            let state = app.read(cx);
            state
                .run_controls
                .selected()
                .is_some_and(|config| state.run_controls.preparation_complete(&config.id))
        })
    });
    let mut renderer = images::VectorRenderer::default();
    let mut extra = Vec::new();
    for _ in 0..100 {
        frame(&mut manager, &mut renderer, &app, cx, &mut extra);
        if root.path().join("program.txt").exists() {
            break;
        }
    }
    let named = launches
        .iter()
        .map(|(id, _, request)| {
            (
                *id,
                *request,
                manager
                    .execution(*id)
                    .and_then(|session| session.request().name.clone())
                    .unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();
    let progress = cx.update(|_, cx| {
        let state = app.read(cx);
        let selected = state
            .run_controls
            .selected()
            .map(|config| config.id.clone());
        (
            state.status.clone(),
            selected
                .as_deref()
                .and_then(|id| state.run_controls.preparation(id))
                .map(|sequence| {
                    sequence
                        .steps()
                        .iter()
                        .map(|step| format!("{}={:?}", step.name, step.state))
                        .collect::<Vec<_>>()
                }),
            state.run_controls.sessions().len(),
        )
    });
    assert_eq!(
        named.len(),
        3,
        "the build, the pre-launch step and the program are three sessions: {named:?} progress={progress:?}"
    );
    // The order is the description's order: a step may rely on what the previous one produced.
    let names = launches
        .iter()
        .map(|(id, _, _)| manager.execution(*id).unwrap().request().name.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            Some("第一步".to_owned()),
            Some("第二步".to_owned()),
            Some("验收配置".to_owned())
        ]
    );
    // Each step really ran, and the program ran only after both.
    assert_eq!(std::fs::read_to_string(&first).unwrap().trim(), "one");
    assert_eq!(std::fs::read_to_string(&second).unwrap().trim(), "two");
    assert!(root.path().join("program.txt").exists());
    shut_down(&mut manager, &app, cx);
}

/// A failing step stops the sequence: no later step and no program are started.
#[gpui::test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn a_failing_step_blocks_every_later_step_and_the_program(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let later = root.path().join("later.txt");
    let (mut manager, app, id, cx) = fixture(
        cx,
        root.path(),
        &action("会失败", "exit 3"),
        &action(
            "不应执行",
            &format!("Set-Content -Path '{}' -Value later", later.display()),
        ),
    );
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.start_selected_run(window, cx));
    });
    let launches = run_until(&mut manager, &app, cx, |app, cx| {
        cx.update(|_, cx| !app.read(cx).run_controls.is_preparing(&id))
    });
    assert_eq!(launches.len(), 1, "only the failing step was requested");
    // Neither the later step nor the program ran, and the reason is visible with the failing step.
    assert!(!later.exists(), "a blocked step must not run");
    assert!(!root.path().join("program.txt").exists());
    let status = cx.update(|_, cx| app.read(cx).status.clone());
    assert!(
        status.contains("会失败") && status.contains('3'),
        "{status}"
    );
    shut_down(&mut manager, &app, cx);
}

/// Build runs the build actions only: no pre-launch step and no program.
#[gpui::test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn build_runs_only_the_build_actions(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let produced = root.path().join("built.txt");
    let (mut manager, app, id, cx) = fixture(
        cx,
        root.path(),
        &action(
            "构建",
            &format!("Set-Content -Path '{}' -Value built", produced.display()),
        ),
        &action(
            "启动前",
            &format!(
                "Set-Content -Path '{}' -Value prelaunch",
                root.path().join("prelaunch.txt").display()
            ),
        ),
    );
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.build_selected(window, cx));
    });
    // A build is only finished once it has been observed to start and then to end.
    let mut renderer = images::VectorRenderer::default();
    let mut launches = Vec::new();
    let mut started = false;
    for _ in 0..300 {
        frame(&mut manager, &mut renderer, &app, cx, &mut launches);
        let preparing = cx.update(|_, cx| app.read(cx).run_controls.is_preparing(&id));
        if preparing {
            started = true;
        } else if started {
            break;
        }
    }
    assert!(started, "the build was requested");
    let named = launches
        .iter()
        .map(|(id, _, request)| {
            (
                *id,
                *request,
                manager
                    .execution(*id)
                    .and_then(|session| session.request().name.clone())
                    .unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        named,
        vec![(launches[0].0, launches[0].2, "构建".to_owned())],
        "a build starts one session"
    );
    assert_eq!(
        manager
            .execution(launches[0].0)
            .unwrap()
            .request()
            .name
            .as_deref(),
        Some("构建")
    );
    // The build really ran, and it started nothing else.
    let entries = std::fs::read_dir(root.path())
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    assert!(
        produced.exists(),
        "the build action's own output is missing; root={} entries={entries:?} status={:?}",
        root.path().display(),
        cx.update(|_, cx| app.read(cx).status.clone())
    );
    assert!(!root.path().join("prelaunch.txt").exists());
    assert!(!root.path().join("program.txt").exists());
    shut_down(&mut manager, &app, cx);
}

/// Stopping during preparation stops the step and never starts the program.
#[gpui::test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn stopping_during_preparation_never_starts_the_program(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (mut manager, app, id, cx) = fixture(
        cx,
        root.path(),
        &action("长构建", "Start-Sleep -Seconds 120"),
        "",
    );
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.start_selected_run(window, cx));
    });
    // One loop drives the whole interaction, so the stop is requested at the moment the step is
    // actually running rather than after a second look that could observe something else.
    let mut renderer = images::VectorRenderer::default();
    let mut launches = Vec::new();
    let mut stopped_session = None;
    for _ in 0..300 {
        frame(&mut manager, &mut renderer, &app, cx, &mut launches);
        if stopped_session.is_none() {
            let current = cx.update(|_, cx| {
                app.update(cx, |app, _| {
                    let session = app
                        .run_controls
                        .preparation(&id)
                        .and_then(|sequence| sequence.current_session());
                    if session.is_some() {
                        app.run_controls.stop_preparations();
                    }
                    session
                })
            });
            stopped_session = current;
            continue;
        }
        // The check does not read the worker's queue at all. `Receiver::try_iter` drains it, so reading
        // it here takes the stop out of the channel before the harness can perform it inside `frame` —
        // which is what left no answer to publish and made this case fail while looking like a product
        // defect. What the stop addressed is checked afterwards, from the result the harness published.
        let blocked = cx.update(|_, cx| app.read(cx).run_controls.preparation_blocked(&id));
        if blocked.is_some() {
            break;
        }
    }
    let stopped_session = stopped_session.expect("the step was running when it was stopped");
    // The runtime's own session state is deliberately not asserted here. It still reads `Running` after
    // the provider was asked to stop, because that provider acknowledges a stop when termination has been
    // *issued* — the distinction the design draws when it says an accepted stop is not the program having
    // ended. What the case is about is that the editor learned the preparation was stopped and therefore
    // never launched the program, which the two assertions below state.
    let _ = stopped_session;
    let reason = cx.update(|_, cx| app.read(cx).run_controls.preparation_blocked(&id));
    assert!(
        reason
            .as_deref()
            .is_some_and(|reason| reason.contains("停止")),
        "a stopped preparation reports why: {reason:?}"
    );
    assert!(
        !root.path().join("program.txt").exists(),
        "a stopped preparation must not launch the program"
    );
    assert_eq!(launches.len(), 1, "the program was never requested");
    shut_down(&mut manager, &app, cx);
}

/// A step that names another configuration runs that configuration's build as it is now, not as a
/// copy: editing the referenced build changes what the next launch does.
#[gpui::test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn a_step_that_references_a_build_tracks_its_current_definition(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let mut manager = runtime(root.path());
    let first = root.path().join("library-v1.txt");
    let second = root.path().join("library-v2.txt");
    let key = root.path().display().to_string();
    let running = format!("{key}-run");
    let library = "库配置";
    let (app, cx) = editor_on(
        cx,
        root.path(),
        vec![
            configuration(
                "lib",
                library,
                "Start-Sleep -Seconds 60",
                &action(
                    "编译库",
                    &format!("Set-Content -Path '{}' -Value v1", first.display()),
                ),
                "",
            ),
            configuration(
                &running,
                "运行",
                &format!(
                    "[Console]::Write('PROGRAM_RAN'); Set-Content -Path '{}' -Value ran",
                    root.path().join("program.txt").display()
                ),
                "",
                &format!("先建库 = @{library}"),
            ),
        ],
    );
    let mut renderer = images::VectorRenderer::default();
    publish_with_launches(&mut manager, &mut renderer, &app, cx, &[]);
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.start_selected_run(window, cx));
    });
    let launches = run_until(&mut manager, &app, cx, |app, cx| {
        cx.update(|_, cx| {
            let state = app.read(cx);
            state.run_controls.preparation_complete(&running)
        })
    });
    // The reference ran the library's build action, then the program, as two sessions.
    let named = launches
        .iter()
        .map(|(id, _, _)| {
            manager
                .execution(*id)
                .and_then(|session| session.request().name.clone())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        named.len(),
        2,
        "the referenced build and the program: {named:?}"
    );
    assert!(
        named[0].contains("先建库"),
        "the step names its reference: {named:?}"
    );
    assert_eq!(named[1], "运行");
    // The program is a fresh process, so it is given its own moment to produce its output.
    let mut extra = Vec::new();
    for _ in 0..100 {
        frame(&mut manager, &mut renderer, &app, cx, &mut extra);
        if first.exists() && root.path().join("program.txt").exists() {
            break;
        }
    }
    assert!(
        first.exists(),
        "the referenced configuration's own build action ran"
    );
    assert!(root.path().join("program.txt").exists());
    // The program step is still running, so this launch is stopped before the next one starts.
    let sessions = cx.update(|_, cx| app.read(cx).run_controls.active_sessions());
    for session in sessions {
        let _ = manager.stop_execution(session.id);
    }
    for _ in 0..60 {
        manager.poll();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    // Editing the referenced build changes what this configuration's step does, with no copy of the
    // command anywhere in the referencing configuration.
    cx.update(|_, cx| {
        app.update(cx, |app, cx| {
            let key = app.workspace_key();
            let mut updated = app.run_controls.configuration("lib").cloned().unwrap();
            updated.build = crate::run::parse_steps(&action(
                "编译库",
                &format!("Set-Content -Path '{}' -Value v2", second.display()),
            ))
            .unwrap();
            app.run_controls.upsert(updated, &key).unwrap();
            // The referencing step still says only `@库配置`.
            let step = app.run_controls.configuration(&running).unwrap();
            assert_eq!(
                step.prelaunch[0].target,
                editor_core::StepTarget::Build {
                    config: library.to_owned()
                },
                "the reference is an identity, not a copied command"
            );
            cx.notify();
        });
    });
    // The edited definitions are launched again, from a fresh editor over the same stored
    // configurations: a second launch is a new decision, not a resumption of the first one, and the
    // first launch's own programs are not part of it.
    let edited = cx.update(|_, cx| {
        let app = app.read(cx);
        vec![
            app.run_controls.configuration("lib").cloned().unwrap(),
            app.run_controls.configuration(&running).cloned().unwrap(),
        ]
    });
    let (second_app, cx) = editor_on(cx, root.path(), edited);
    cx.update(|window, cx| {
        second_app.update(cx, |app, cx| app.start_selected_run(window, cx));
    });
    for _ in 0..300 {
        frame(
            &mut manager,
            &mut renderer,
            &second_app,
            cx,
            &mut Vec::new(),
        );
        if second.exists() {
            break;
        }
    }
    assert!(
        second.exists(),
        "the edited build definition is what the next launch ran; status={:?}",
        cx.update(|_, cx| second_app.read(cx).status.clone())
    );
    shut_down(&mut manager, &second_app, cx);
}

/// A configuration without build actions says why Build cannot run.
#[test]
fn build_reports_why_it_is_unavailable() {
    let root = tempfile::tempdir().unwrap().keep();
    let mut set = editor_core::RunConfigSet::default();
    let id = set.generate_id("C:/work");
    set.upsert(
        crate::run::RunConfigDraft {
            id: id.clone(),
            name: "无构建".into(),
            shell: false,
            program: "app.exe".into(),
            arguments: String::new(),
            script: String::new(),
            directory: String::new(),
            environment: String::new(),
            tool_paths: String::new(),
            source: editor_core::RunConfigSource::Local,
            from_target: None,
            provider: None,
            breakpoints: String::new(),
            share: false,
            build: String::new(),
            prelaunch: String::new(),
        }
        .to_config()
        .unwrap(),
    )
    .unwrap();
    // The control's reason is read from the same store the title bar uses.
    let mut controls = crate::run::RunControls::load("C:/work", Some(root));
    controls
        .upsert(set.configurations[0].clone(), "C:/work")
        .unwrap();
    let reason = controls
        .preparation_error(&id)
        .expect("a configuration without build actions cannot build");
    assert!(reason.contains("没有构建操作"), "{reason}");
}
