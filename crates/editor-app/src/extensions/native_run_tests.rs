//! The native Run control starts a real program through the public execution contract.
//!
//! These checks exercise saved configuration values through native execution and its visible grid. No
//! test-only host API is introduced, and the assertions are only about what a user can observe.
#![cfg(windows)]
use super::composable_tests::{publish, publish_with_launches, pump_recording};
use super::*;
use gpui_kit::{TestAppContext, gpui};

// Debug acceptance shares this native workspace fixture, while its deferred driver stays separate.
mod debugging;
mod sharing;

/// Open native execution with an isolated saved configuration; no terminal package is installed.
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
    fixture_with_provider(cx, root, program, args, true)
}

/// Built-in acceptance deliberately runs without an installed terminal package.
fn builtin_fixture<'a>(
    cx: &'a mut TestAppContext,
    root: &std::path::Path,
    program: &str,
    args: Vec<String>,
) -> (
    plugin_runtime::Manager,
    Entity<EditorApp>,
    &'a mut gpui_kit::VisualTestContext,
) {
    fixture_with_provider(cx, root, program, args, false)
}

/// Shared initialization supports saved projection fixtures and real configuration-provider fixtures.
fn fixture_with_provider<'a>(
    cx: &'a mut TestAppContext,
    root: &std::path::Path,
    program: &str,
    args: Vec<String>,
    legacy: bool,
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
    if !legacy {
        let configuration = Package::read(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/plugin-api-test/configuration-example.zip"),
        )
        .unwrap();
        manager
            .install(&configuration, configuration.manifest.permissions.clone())
            .unwrap();
    }

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
            if legacy {
                app.run_controls.upsert(set.configurations[0].clone(), &key).unwrap();
            } else {
                let configuration = set.configurations[0].clone();
                let editor_core::RunTarget::Program { program, args } = &configuration.target else { unreachable!() };
                let data = editor_core::PluginConfiguration {
                    provider:"configuration-example".into(), template:"program".into(),
                    values:serde_json::json!({"name":configuration.name,"program":program,"arguments":args,"horizontal":false}).to_string(),
                    pending_events:vec![], name:configuration.name.clone(), program:program.clone(),
                    revision:0, validation:editor_core::ConfigurationValidation::Valid,
                };
                app.run_controls.accept_configuration_projection(configuration,data).unwrap();
            }
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

/// Read the actual native terminal's glyphs after the ordinary application frame has drawn.
fn painted_text(app: &Entity<EditorApp>, cx: &mut gpui_kit::VisualTestContext) -> String {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    crate::terminal::tests::painted(app, cx)
}

/// A configuration's environment reaches the program it starts, through the public contract.
#[gpui::test]
#[ignore = "requires real native process acceptance"]
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
            && painted_text(&app, cx).contains("ENV_REACHED_CHILD")
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
    assert!(painted_text(&app, cx).contains("ENV_REACHED_CHILD"));

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
        if launches.len() > before && painted_text(&app, cx).contains("NO_ENV_HERE") {
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
#[ignore = "requires real native process acceptance"]
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
            && painted_text(&app, cx).contains("SCRIPT_MODE_OK")
        {
            session = Some(*id);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let session = session.unwrap_or_else(|| {
        let status = cx.update(|_, cx| app.read(cx).status.clone());
        let painted = painted_text(&app, cx);
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
    let painted = painted_text(&app, cx);
    assert!(painted.contains("SCRIPT_MODE_OK2"), "{painted:?}");
    let stored = cx.update(|_, cx| {
        editor_core::storage_path(&app.read(cx).workspace_key()).expect("host-local path")
    });
    let _ = std::fs::remove_file(stored);
    let _ = config;
}

/// A restricted workspace never starts a program, whatever the stored configuration says.
#[gpui::test]
#[ignore = "package configuration examples through the public SDK first"]
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
