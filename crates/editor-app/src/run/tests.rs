//! Launch decisions and session bookkeeping keep one meaning for visible run state.
use super::*;

/// Controls backed by a real host-local directory so persistence is exercised, not bypassed.
fn controls() -> RunControls {
    let root = tempfile::tempdir().unwrap().keep();
    RunControls::load("C:/work", Some(root))
}

fn config(id: &str, name: &str) -> RunConfig {
    RunConfig {
        id: id.into(),
        name: name.into(),
        target: RunTarget::Program {
            program: "powershell.exe".into(),
            args: vec!["-NoProfile".into(), "Write-Output ok".into()],
        },
        directory: None,
        env: Default::default(),
        tool_paths: Default::default(),
        build: Default::default(),
        prelaunch: Default::default(),
        local: true,
    }
}

fn snapshot(
    id: u64,
    config: &str,
    request_id: u64,
    state: plugin_runtime::ExecutionState,
) -> HostRunSnapshot {
    HostRunSnapshot {
        id,
        config: config.into(),
        request_id,
        plugin: "terminal".into(),
        state,
        provider_session: Some("1".into()),
        failure: None,
    }
}

/// A launch resolves to the session that already exists instead of starting a duplicate program.
#[test]
fn a_running_configuration_resolves_to_its_session() {
    let mut controls = controls();
    controls
        .upsert(config("run-1", "本机程序"), "C:/work")
        .unwrap();
    let request_id = controls.begin("run-1");
    controls.reconcile(&[snapshot(
        7,
        "run-1",
        request_id,
        plugin_runtime::ExecutionState::Running,
    )]);

    assert_eq!(
        controls.plan_launch("run-1", "C:/work"),
        LaunchPlan::Existing { session: 7 }
    );
    assert_eq!(controls.sessions().len(), 1);
    assert_eq!(controls.active_sessions().len(), 1);
    assert_eq!(controls.sessions_for("run-1")[0].plugin, "terminal");
    assert!(!controls.is_pending("run-1"));
}

/// A configuration without a directory launches from the workspace root, arguments stay literal.
#[test]
fn a_valid_configuration_plans_a_literal_request() {
    let mut controls = controls();
    controls
        .upsert(config("run-1", "带空格"), "C:/work")
        .unwrap();
    let plan = controls.plan_launch("run-1", "C:/work/project");
    let request = RunControls::request_for(&plan).expect("valid configuration plans a start");
    assert_eq!(request.program, "powershell.exe");
    assert_eq!(request.args, vec!["-NoProfile", "Write-Output ok"]);
    assert_eq!(request.cwd.as_deref(), Some("C:/work/project"));
    assert_eq!(request.name.as_deref(), Some("带空格"));

    // An explicit absolute directory wins over the workspace root.
    let mut stored = config("run-2", "指定目录");
    stored.directory = Some("C:/work/target".into());
    controls.upsert(stored, "C:/work").unwrap();
    let plan = controls.plan_launch("run-2", "C:/work/project");
    assert_eq!(
        RunControls::request_for(&plan).unwrap().cwd.as_deref(),
        Some("C:/work/target")
    );
}

/// An unknown or invalid configuration explains itself rather than launching something else.
#[test]
fn invalid_targets_are_reported_instead_of_launched() {
    let mut controls = controls();
    assert!(matches!(
        controls.plan_launch("missing", "C:/work"),
        LaunchPlan::Invalid { .. }
    ));
    // A relative directory cannot be stored, so it is rejected before it reaches the launch path.
    let mut broken = config("run-1", "坏目录");
    broken.directory = Some("relative".into());
    assert!(controls.upsert(broken, "C:/work").is_err());
    assert!(controls.configurations().is_empty());
    assert_eq!(controls.error.is_some(), true);
}

/// A launch is only adopted by the request that produced it, and published state stays authoritative.
#[test]
fn sessions_follow_their_own_request_and_later_state() {
    let mut controls = controls();
    controls
        .upsert(config("run-1", "本机程序"), "C:/work")
        .unwrap();
    let mine = controls.begin("run-1");
    // Another window's session must not be adopted as this editor's result.
    controls.reconcile(&[snapshot(
        1,
        "run-1",
        mine + 100,
        plugin_runtime::ExecutionState::Running,
    )]);
    assert!(controls.sessions().is_empty());
    assert!(controls.is_pending("run-1"));

    controls.reconcile(&[snapshot(
        9,
        "run-1",
        mine,
        plugin_runtime::ExecutionState::Starting,
    )]);
    assert!(
        controls.is_pending("run-1"),
        "a requested start is still pending"
    );

    // The provider retires: the published state replaces the stale one for a known session.
    controls.reconcile(&[HostRunSnapshot {
        failure: Some("provider retired".into()),
        state: plugin_runtime::ExecutionState::Failed,
        ..snapshot(9, "run-1", mine, plugin_runtime::ExecutionState::Failed)
    }]);
    let sessions = controls.sessions();
    assert_eq!(sessions.len(), 1);
    assert!(!sessions[0].is_active());
    assert_eq!(sessions[0].failure.as_deref(), Some("provider retired"));
    assert!(controls.active_sessions().is_empty());
    // A finished session no longer blocks a new launch of the same configuration.
    assert!(matches!(
        controls.plan_launch("run-1", "C:/work"),
        LaunchPlan::Start { .. }
    ));
}

/// Configurations are stored host-locally and read back with the same meaning.
#[test]
fn configurations_round_trip_through_the_host_local_file() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().to_path_buf();
    let mut controls = RunControls::load("C:/work/project", Some(root.clone()));
    let id = controls.generate_id("C:/work/project");
    let mut stored = config(&id, "往返");
    stored.target = RunTarget::Script {
        interpreter: "pwsh.exe".into(),
        args: vec!["-NoProfile".into(), "-Command".into()],
        script: "Get-ChildItem".into(),
    };
    controls.upsert(stored.clone(), "C:/work/project").unwrap();
    assert!(controls.select(&id, "C:/work/project"));

    let reloaded = RunControls::load("C:/work/project", Some(root));
    assert_eq!(reloaded.configurations(), &[stored.clone()]);
    assert_eq!(reloaded.selected(), Some(&stored));
    // Interpreter mode keeps its script body as one literal argument.
    let plan = reloaded.plan_launch(&id, "C:/work/project");
    assert_eq!(
        RunControls::request_for(&plan).unwrap().args,
        vec!["-NoProfile", "-Command", "Get-ChildItem"]
    );
}

/// The unified dropdown separates running work from saved targets, in that order.
#[test]
fn the_unified_dropdown_groups_sessions_before_configurations() {
    let mut controls = controls();
    // An empty editor still explains itself instead of showing an empty menu.
    assert_eq!(
        controls.menu_entries(),
        vec![
            RunMenuEntry::Action {
                id: "run-none".into(),
                label: "(没有运行中的会话)".into(),
                enabled: false
            },
            RunMenuEntry::Separator,
            RunMenuEntry::Action {
                id: "run-empty".into(),
                label: "(尚未保存运行配置)".into(),
                enabled: false
            },
            RunMenuEntry::Separator,
            RunMenuEntry::Action {
                id: "run-edit".into(),
                label: "编辑所选配置…".into(),
                enabled: false
            },
            RunMenuEntry::Action {
                id: "run-new".into(),
                label: "新建运行配置…".into(),
                enabled: true
            },
            RunMenuEntry::Action {
                id: "run-discover".into(),
                label: "发现配置（待插件贡献）".into(),
                enabled: false
            },
        ]
    );

    // Two configurations and one running session produce three groups in one component.
    controls
        .upsert(config("run-1", "第一个"), "C:/work")
        .unwrap();
    controls
        .upsert(config("run-2", "第二个"), "C:/work")
        .unwrap();
    controls.select("run-2", "C:/work");
    let request_id = controls.begin("run-1");
    controls.reconcile(&[snapshot(
        9,
        "run-1",
        request_id,
        plugin_runtime::ExecutionState::Running,
    )]);
    assert_eq!(
        controls.menu_entries(),
        vec![
            RunMenuEntry::Session {
                id: 9,
                label: "1 · 运行中".into()
            },
            RunMenuEntry::Separator,
            RunMenuEntry::Configuration {
                id: "run-1".into(),
                label: "第一个".into()
            },
            // The selected target is marked, so the collapsed name and the menu agree.
            RunMenuEntry::Configuration {
                id: "run-2".into(),
                label: "第二个 ✓".into()
            },
            RunMenuEntry::Separator,
            RunMenuEntry::Action {
                id: "run-edit".into(),
                label: "编辑所选配置…".into(),
                enabled: true
            },
            RunMenuEntry::Action {
                id: "run-new".into(),
                label: "新建运行配置…".into(),
                enabled: true
            },
            RunMenuEntry::Action {
                id: "run-discover".into(),
                label: "发现配置（待插件贡献）".into(),
                enabled: false
            },
        ]
    );
}

/// A draft keeps one argument per line and never re-splits a value containing spaces.
#[test]
fn drafts_preserve_argument_boundaries() {
    let draft = RunConfigDraft {
        id: "run-1".into(),
        name: " 带空格 ".into(),
        shell: false,
        program: " C:/Program Files/tool.exe ".into(),
        script: String::new(),
        arguments: "--flag\nC:/path with spaces/file.txt\n".into(),
        directory: " C:/work ".into(),
        tool_paths: String::new(),
        // One prepared action per line: a name, then the program, then its literal arguments.
        build: "构建 = cargo.exe | build".into(),
        prelaunch: "生成代码 = tool.exe | gen | --out dir with spaces".into(),
        // Values keep everything after the first `=`, including spaces and further equals signs.
        environment: "APP_MODE=dev\nTOKEN=a=b c\nEMPTY=\n".into(),
    };
    let configuration = draft.to_config().expect("the draft is usable");
    assert_eq!(configuration.name, "带空格");
    assert_eq!(
        configuration.target.executable(),
        "C:/Program Files/tool.exe"
    );
    assert_eq!(
        configuration.literal_arguments(),
        vec!["--flag", "C:/path with spaces/file.txt"]
    );
    assert_eq!(configuration.directory.as_deref(), Some("C:/work"));
    assert_eq!(
        configuration.env.get("TOKEN").map(String::as_str),
        Some("a=b c")
    );
    assert_eq!(configuration.env.get("EMPTY").map(String::as_str), Some(""));
    configuration
        .validate()
        .expect("draft produces a valid configuration");

    // The entries round-trip through the edited text without being re-parsed as anything else.
    let reopened = RunConfigDraft::from_config(Some(&configuration), "run-1".into());
    assert_eq!(reopened.environment, "APP_MODE=dev\nEMPTY=\nTOKEN=a=b c");
    assert_eq!(
        reopened.to_config().unwrap().env,
        configuration.env,
        "reopening a configuration keeps its environment"
    );

    // An empty directory means the workspace root rather than an empty path.
    let bare = RunConfigDraft::from_config(None, "run-2".into());
    assert_eq!(bare.to_config().unwrap().directory, None);
    assert_eq!(
        RunConfigDraft::from_config(Some(&configuration), "run-1".into()).arguments,
        "--flag\nC:/path with spaces/file.txt"
    );
}

/// Prepared actions keep their name, program and literal arguments across an edit.
#[test]
fn prepared_actions_round_trip_through_the_edited_form() {
    let draft = RunConfigDraft {
        id: "run-1".into(),
        name: "构建".into(),
        shell: false,
        program: "app.exe".into(),
        arguments: String::new(),
        script: String::new(),
        directory: String::new(),
        environment: String::new(),
        tool_paths: String::new(),
        build: "构建 = cargo.exe | build | --release".into(),
        // A value containing spaces stays one argument, because the separator is the only split.
        prelaunch: "生成 = tool.exe | gen | --out dir with spaces\n复制 = copy.exe | a b".into(),
    };
    let configuration = draft.to_config().expect("the draft is usable");
    assert_eq!(configuration.build.len(), 1);
    assert_eq!(configuration.build[0].name, "构建");
    assert_eq!(
        configuration.build[0].target.arguments(),
        vec!["build", "--release"]
    );
    assert_eq!(configuration.prelaunch.len(), 2);
    assert_eq!(
        configuration.prelaunch[1].target.arguments(),
        vec!["a b"],
        "a space is not a separator"
    );
    configuration
        .validate()
        .expect("prepared actions are valid");

    // Reopening shows the same lines and produces the same actions.
    let reopened = RunConfigDraft::from_config(Some(&configuration), "run-1".into());
    assert_eq!(
        reopened.build, "构建 = cargo.exe | build | --release",
        "the rendered line is the line the user edits"
    );
    assert_eq!(reopened.to_config().unwrap().build, configuration.build);
    assert_eq!(
        reopened.to_config().unwrap().prelaunch,
        configuration.prelaunch
    );
}

/// A pre-launch step can require another configuration's build by identity, never by copy.
#[test]
fn a_prelaunch_step_references_a_build_without_copying_it() {
    let draft = RunConfigDraft {
        id: "run-1".into(),
        name: "运行".into(),
        shell: false,
        program: "app.exe".into(),
        arguments: String::new(),
        script: String::new(),
        directory: String::new(),
        environment: String::new(),
        tool_paths: String::new(),
        build: String::new(),
        prelaunch: "先构建 = @库配置\n后生成 = tool.exe | gen".into(),
    };
    let configuration = draft.to_config().expect("the draft is usable");
    assert_eq!(
        configuration.prelaunch[0].target,
        editor_core::StepTarget::Build {
            config: "库配置".into()
        }
    );
    // A reference carries no command of its own, so nothing can drift from the referenced build.
    assert_eq!(configuration.prelaunch[0].target.executable(), None);
    assert!(configuration.prelaunch[0].target.arguments().is_empty());
    configuration
        .validate()
        .expect("a reference is a valid step");

    // The reference survives an edit round-trip, including its own name.
    let reopened = RunConfigDraft::from_config(Some(&configuration), "run-1".into());
    assert_eq!(
        reopened.prelaunch,
        "先构建 = @库配置\n后生成 = tool.exe | gen"
    );
    assert_eq!(
        reopened.to_config().unwrap().prelaunch,
        configuration.prelaunch
    );

    // An empty reference is refused instead of becoming a step that can never resolve.
    let empty = RunConfigDraft {
        prelaunch: "先构建 = @".into(),
        ..reopened
    };
    assert!(empty.to_config().is_err());
    let blank = RunConfigDraft {
        prelaunch: "先构建 = @  ".into(),
        ..empty
    };
    assert!(blank.to_config().is_err());
}

/// A malformed prepared action is refused where the user can correct it.
#[test]
fn malformed_prepared_actions_are_refused() {
    let base = RunConfigDraft {
        id: "run-1".into(),
        name: "构建".into(),
        shell: false,
        program: "app.exe".into(),
        arguments: String::new(),
        script: String::new(),
        directory: String::new(),
        environment: String::new(),
        tool_paths: String::new(),
        build: "没有等号".into(),
        prelaunch: String::new(),
    };
    assert!(base.to_config().is_err());
    let nameless = RunConfigDraft {
        build: " = cargo.exe | build".into(),
        ..base
    };
    assert!(nameless.to_config().is_err());
    let empty_program = RunConfigDraft {
        build: "构建 = ".into(),
        ..nameless
    };
    assert!(empty_program.to_config().is_err());
    // Blank lines are simply absent actions rather than errors.
    let padded = RunConfigDraft {
        build: "\n构建 = cargo.exe | build\n\n".into(),
        ..empty_program
    };
    assert_eq!(padded.to_config().unwrap().build.len(), 1);
}

/// A malformed environment line is refused where the user can correct it.
#[test]
fn a_malformed_environment_line_is_refused() {
    let base = RunConfigDraft {
        id: "run-1".into(),
        name: "环境".into(),
        shell: false,
        program: "tool.exe".into(),
        script: String::new(),
        arguments: String::new(),
        directory: String::new(),
        environment: "没有等号".into(),
        tool_paths: String::new(),
        build: String::new(),
        prelaunch: String::new(),
    };
    let error = base.to_config().expect_err("a line without '=' is refused");
    assert!(error.contains("名称=值"), "{error}");
    // A name that a native program could not accept is refused too, and by the store as well.
    let invalid = RunConfigDraft {
        environment: "BAD-NAME\n".into(),
        ..base
    };
    assert!(invalid.to_config().is_err());
}

/// Shell mode is a separate mode, not quoting inside program mode.
#[test]
fn shell_mode_names_an_interpreter_and_passes_the_script_verbatim() {
    let draft = RunConfigDraft {
        id: "run-1".into(),
        name: "脚本".into(),
        shell: true,
        program: " pwsh.exe ".into(),
        arguments: "-NoProfile\n-Command\n".into(),
        // A pipeline and a quoted value survive as one script, never as separate arguments.
        script: "Get-ChildItem | Where-Object { $_.Name -like 'a b*' }".into(),
        directory: String::new(),
        environment: String::new(),
        tool_paths: String::new(),
        build: String::new(),
        prelaunch: String::new(),
    };
    let configuration = draft.to_config().expect("the draft is usable");
    assert_eq!(configuration.target.executable(), "pwsh.exe");
    assert_eq!(
        configuration.literal_arguments(),
        vec![
            "-NoProfile",
            "-Command",
            "Get-ChildItem | Where-Object { $_.Name -like 'a b*' }"
        ]
    );
    configuration
        .validate()
        .expect("an interpreter with a script body is a valid configuration");

    // Reopening keeps the mode, the interpreter and the script body.
    let reopened = RunConfigDraft::from_config(Some(&configuration), "run-1".into());
    assert!(reopened.shell);
    assert_eq!(reopened.program, "pwsh.exe");
    assert_eq!(
        reopened.script,
        "Get-ChildItem | Where-Object { $_.Name -like 'a b*' }"
    );
    assert_eq!(reopened.to_config().unwrap().target, configuration.target);

    // Switching back to program mode drops the script body instead of keeping a hidden command.
    let as_program = RunConfigDraft {
        shell: false,
        ..reopened
    }
    .to_config()
    .unwrap();
    assert!(matches!(
        as_program.target,
        RunTarget::Program { ref program, .. } if program == "pwsh.exe"
    ));
    assert_eq!(
        as_program.literal_arguments(),
        vec!["-NoProfile", "-Command"],
        "a program launch never gains the script text"
    );
}
