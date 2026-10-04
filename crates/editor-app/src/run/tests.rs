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
        source: editor_core::RunConfigSource::Local,
        from_target: None,
        provider: None,
        breakpoints: Default::default(),
        local: true,
    }
}

/// A configuration whose build actions and pre-launch steps are the given edited lines.
fn with_steps(id: &str, name: &str, build: &str, prelaunch: &str) -> RunConfig {
    let draft = RunConfigDraft {
        id: id.into(),
        name: name.into(),
        shell: false,
        program: "app.exe".into(),
        arguments: String::new(),
        script: String::new(),
        directory: None.or(Some(String::new())).unwrap_or_default(),
        environment: String::new(),
        tool_paths: String::new(),
        source: editor_core::RunConfigSource::Local,
        from_target: None,
        provider: None,
        breakpoints: String::new(),
        share: false,
        build: build.into(),
        prelaunch: prelaunch.into(),
    };
    draft
        .to_config()
        .expect("the fixture is a valid configuration")
}

/// Controls backed by a real host-local directory and a real project directory.
fn shared_controls() -> (RunControls, std::path::PathBuf, std::path::PathBuf) {
    let project = tempfile::tempdir().unwrap().keep();
    let local = tempfile::tempdir().unwrap().keep();
    let controls = RunControls::load_with_project(
        &project.display().to_string(),
        Some(local.clone()),
        Some(project.clone()),
    );
    (controls, project, local)
}

/// Sharing writes the portable half into the project and leaves this machine's own values here.
#[test]
fn sharing_writes_the_project_file_and_keeps_local_values_local() {
    let (mut controls, project, local) = shared_controls();
    let workspace = project.display().to_string();
    let mut configuration = config("run-1", "共享");
    configuration.directory = Some(project.display().to_string());
    configuration.env = [("SECRET".to_owned(), "s3cret".to_owned())].into();
    configuration.tool_paths = vec![project.join("tools").display().to_string()];
    // Nothing is written into the project until the user chooses to share.
    assert!(!editor_core::project_path(&project).exists());
    configuration.local = true;
    controls.upsert(configuration.clone(), &workspace).unwrap();
    assert!(
        !editor_core::project_path(&project).exists(),
        "a local-only save does not touch the project"
    );

    // Choosing the project writes the shared file and keeps the personal values host-local.
    configuration.local = false;
    controls.upsert(configuration.clone(), &workspace).unwrap();
    let path = editor_core::project_path(&project);
    let bytes = std::fs::read_to_string(&path).unwrap();
    assert!(bytes.contains("共享"));
    assert!(
        !bytes.contains("s3cret"),
        "a value never enters the project file"
    );
    assert!(
        !bytes.contains("SECRET"),
        "a variable name never enters the project file"
    );
    assert!(
        bytes.contains(editor_core::WORKSPACE_TOKEN),
        "the project root is stored as a token"
    );
    // The machine's own file keeps what is local and no copy of the portable half.
    let stored = editor_core::load(&local, &workspace).unwrap();
    let entry = stored.find("run-1").expect("the local half is stored");
    assert_eq!(entry.env.get("SECRET").map(String::as_str), Some("s3cret"));
    assert_eq!(entry.tool_paths, configuration.tool_paths);
    // The local record is complete, but the project's file is what decides what runs: drifting this
    // copy cannot change the shared definition.
    assert_eq!(entry.name, "共享");

    // Reopening merges them back into one configuration with its environment intact.
    let reopened =
        RunControls::load_with_project(&workspace, Some(local.clone()), Some(project.clone()));
    let merged = reopened
        .configuration("run-1")
        .expect("the entry is loaded");
    assert_eq!(merged.name, "共享");
    assert_eq!(merged.env.get("SECRET").map(String::as_str), Some("s3cret"));
    assert_eq!(merged.source, editor_core::RunConfigSource::Project);
    assert_eq!(merged.directory.as_deref(), Some(workspace.as_str()));
    // A drifting local copy cannot change what the project's file says: the next load reads the
    // project's definition for the portable half.
    let mut drifted = editor_core::load(&local, &workspace).unwrap();
    let mut drifted_entry = drifted.find("run-1").cloned().unwrap();
    drifted_entry.name = "本机改名".into();
    drifted.upsert(drifted_entry).unwrap();
    editor_core::save(&local, &workspace, &drifted).unwrap();
    let reloaded =
        RunControls::load_with_project(&workspace, Some(local.clone()), Some(project.clone()));
    assert_eq!(
        reloaded
            .configuration("run-1")
            .map(|config| config.name.as_str()),
        Some("共享"),
        "the project's file owns the shared definition"
    );
    assert_eq!(
        reloaded
            .configuration("run-1")
            .and_then(|config| config.env.get("SECRET"))
            .map(String::as_str),
        Some("s3cret"),
        "this machine's own value still applies"
    );
    let _ = path;
}

/// Unsharing removes the entry from the project without deleting what the user typed.
#[test]
fn unsharing_leaves_the_project_file_without_the_entry() {
    let (mut controls, project, local) = shared_controls();
    let workspace = project.display().to_string();
    let mut configuration = config("run-1", "共享");
    configuration.local = false;
    controls.upsert(configuration.clone(), &workspace).unwrap();
    assert!(
        std::fs::read_to_string(editor_core::project_path(&project))
            .unwrap()
            .contains("共享")
    );

    // Choosing local again removes the shared entry and keeps the configuration here.
    configuration.local = true;
    controls.upsert(configuration.clone(), &workspace).unwrap();
    let shared = std::fs::read_to_string(editor_core::project_path(&project)).unwrap();
    assert!(
        !shared.contains("共享"),
        "an unshared configuration is gone from the project: {shared} (stored={:?})",
        controls.configuration("run-1").map(|entry| entry.local)
    );
    let reopened =
        RunControls::load_with_project(&workspace, Some(local.clone()), Some(project.clone()));
    let kept = reopened
        .configuration("run-1")
        .expect("the configuration is still this machine's own");
    assert_eq!(kept.source, editor_core::RunConfigSource::Local);
    assert!(
        kept.local,
        "the reopened entry remembers that it is this machine's own"
    );
}

/// A hand-edited shared file is reported and does not replace this machine's configurations.
#[test]
fn a_broken_shared_file_is_reported_without_losing_local_work() {
    let (mut controls, project, local) = shared_controls();
    let workspace = project.display().to_string();
    controls
        .upsert(config("run-1", "本机"), &workspace)
        .unwrap();
    // A file the form would refuse: the program is empty.
    let path = editor_core::project_path(&project);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        r#"{"version":1,"configurations":[{"id":"shared","name":"broken",
            "target":{"mode":"program","program":"","args":[]}}]}"#
            .as_bytes(),
    )
    .unwrap();
    let reopened =
        RunControls::load_with_project(&workspace, Some(local.clone()), Some(project.clone()));
    let error = reopened
        .error
        .as_deref()
        .expect("a broken shared file is reported");
    // The refusal names the rule the file broke, which is the reason a user can act on.
    assert!(
        error.contains("program") || error.contains("Shared"),
        "{error}"
    );
    assert!(
        reopened.configuration("run-1").is_some(),
        "the machine's own configuration is not replaced"
    );
    assert!(reopened.configuration("shared").is_none());
}

/// A configuration removed while shared is gone from the project too.
#[test]
fn removing_a_shared_configuration_removes_it_from_the_project() {
    let (mut controls, project, _local) = shared_controls();
    let workspace = project.display().to_string();
    let mut configuration = config("run-1", "共享");
    configuration.local = false;
    controls.upsert(configuration, &workspace).unwrap();
    controls.remove("run-1", &workspace).unwrap();
    let shared = std::fs::read_to_string(editor_core::project_path(&project)).unwrap();
    assert!(!shared.contains("共享"), "{shared}");
    assert!(controls.configuration("run-1").is_none());
}

/// A host-local-only workspace never creates a project file.
#[test]
fn a_workspace_that_shares_nothing_writes_no_project_file() {
    let (mut controls, project, _local) = shared_controls();
    let workspace = project.display().to_string();
    controls
        .upsert(config("run-1", "本机"), &workspace)
        .unwrap();
    controls
        .upsert(config("run-2", "另一个"), &workspace)
        .unwrap();
    controls.remove("run-1", &workspace).unwrap();
    assert!(
        !editor_core::project_path(&project).exists(),
        "nothing writes into a project until something is shared with it"
    );
}

/// A plan runs the configuration's build first, then its steps, then the program.
#[test]
fn a_plan_orders_build_steps_program() {
    let mut controls = controls();
    controls
        .upsert(
            with_steps(
                "run-1",
                "运行",
                "构建 = cargo.exe | build",
                "生成 = tool.exe | gen",
            ),
            "C:/work",
        )
        .unwrap();
    let plan = controls.prepare_launch("run-1", "C:/work", 16).unwrap();
    assert_eq!(
        plan.steps.iter().map(|step| step.kind).collect::<Vec<_>>(),
        vec![StepKind::Build, StepKind::Prelaunch, StepKind::Program]
    );
    assert_eq!(plan.steps[0].name, "构建");
    assert_eq!(plan.steps[1].name, "生成");
    assert_eq!(plan.steps[2].request.program, "app.exe");
    // Every step inherits the configuration's launch context, so a build and the program it
    // prepares cannot disagree about the directory they run in.
    assert_eq!(plan.steps[0].request.cwd.as_deref(), Some("C:/work"));
    assert!(plan.launches_program());
}

/// A step that names another configuration runs that configuration's current build actions.
#[test]
fn a_step_that_references_a_build_uses_its_current_definition() {
    let mut controls = controls();
    controls
        .upsert(
            with_steps("lib", "库配置", "编译库 = cargo.exe | build | -p lib", ""),
            "C:/work",
        )
        .unwrap();
    controls
        .upsert(
            with_steps("run-1", "运行", "", "先建库 = @库配置"),
            "C:/work",
        )
        .unwrap();
    let plan = controls.prepare_launch("run-1", "C:/work", 16).unwrap();
    assert_eq!(
        plan.steps.len(),
        2,
        "the reference expands to its own actions"
    );
    assert_eq!(plan.steps[0].kind, StepKind::Prelaunch);
    assert_eq!(plan.steps[0].request.program, "cargo.exe");
    assert_eq!(plan.steps[0].request.args, vec!["build", "-p lib"]);
    // The expanded step says which step pulled it in, so a failure names both.
    assert!(plan.steps[0].name.contains("先建库") && plan.steps[0].name.contains("编译库"));

    // Editing the referenced build changes the next launch without touching the referencing step.
    controls
        .upsert(
            with_steps(
                "lib",
                "库配置",
                "编译库 = cargo.exe | build | --release",
                "",
            ),
            "C:/work",
        )
        .unwrap();
    let again = controls.prepare_launch("run-1", "C:/work", 16).unwrap();
    assert_eq!(
        again.steps[0].request.args,
        vec!["build", "--release"],
        "the command exists once, so it cannot drift"
    );
}

/// A reference that cannot resolve blocks the launch before anything starts.
#[test]
fn an_unresolvable_reference_blocks_the_plan() {
    let mut controls = controls();
    controls
        .upsert(
            with_steps("run-1", "运行", "", "先建库 = @不存在的配置"),
            "C:/work",
        )
        .unwrap();
    let message = controls
        .prepare("run-1", "C:/work", 16)
        .expect_err("a missing referenced configuration is reported");
    assert!(message.contains("不存在的配置"), "{message}");

    // A configuration cannot reference its own build: that would be a cycle, not a sequence.
    controls
        .upsert(
            with_steps(
                "run-2",
                "自引用",
                "构建 = cargo.exe | build",
                "自己 = @自引用",
            ),
            "C:/work",
        )
        .unwrap();
    let message = controls
        .prepare("run-2", "C:/work", 16)
        .expect_err("a self reference is refused");
    assert!(message.contains("自身"), "{message}");
}

/// A plan that would grow past its bound is refused rather than run halfway.
#[test]
fn a_plan_beyond_its_bound_is_refused() {
    let mut controls = controls();
    controls
        .upsert(
            with_steps(
                "run-1",
                "运行",
                "一 = cargo.exe | build\n二 = cargo.exe | test",
                "三 = tool.exe | gen",
            ),
            "C:/work",
        )
        .unwrap();
    assert!(controls.prepare("run-1", "C:/work", 3).is_ok());
    let message = controls
        .prepare("run-1", "C:/work", 2)
        .expect_err("a plan past its bound is refused");
    assert!(message.contains('2'), "{message}");
}

/// A preparation step owns its session from the moment the runtime answers its request.
#[test]
fn a_published_session_joins_the_step_that_requested_it() {
    let mut controls = controls();
    controls
        .upsert(
            with_steps("run-1", "运行", "构建 = cargo.exe | build", ""),
            "C:/work",
        )
        .unwrap();
    let plan = controls
        .prepare_launch("run-1", "C:/work", 16)
        .expect("the plan is usable");
    let launch = controls.begin("run-1");
    controls.begin_sequence("run-1", plan, launch);
    // The step is requested under its own identity before its session exists.
    let step_request = controls.begin("run-1");
    controls.note_step_request("run-1", 0, step_request);
    controls.adopt_step_sessions(&[(7, step_request, Some("1".into()))]);
    let sequence = controls.preparation("run-1").unwrap();
    assert_eq!(sequence.current_session(), Some(7));
    assert!(sequence.is_active());
    // A session belonging to no request of this editor is not adopted.
    controls.adopt_step_sessions(&[(8, step_request + 99, None)]);
    assert_eq!(
        controls.preparation("run-1").unwrap().current_session(),
        Some(7)
    );
}

/// An observed step end advances or blocks the sequence that is waiting on it.
#[test]
fn an_observed_step_end_advances_or_blocks_its_sequence() {
    let mut controls = controls();
    controls
        .upsert(
            with_steps(
                "run-1",
                "运行",
                "构建 = cargo.exe | build",
                "生成 = tool.exe | gen",
            ),
            "C:/work",
        )
        .unwrap();
    let plan = controls
        .prepare_launch("run-1", "C:/work", 16)
        .expect("the plan is usable");
    let launch = controls.begin("run-1");
    controls.begin_sequence("run-1", plan, launch);
    let first = controls.begin("run-1");
    controls.note_step_request("run-1", 0, first);
    controls.adopt_step_sessions(&[(7, first, Some("1".into()))]);
    // A successful end advances to the second step.
    let poll = controls.begin_poll("run-1", 0, 7);
    let applied = controls.reconcile_run_status(&[(
        "run-1".into(),
        poll,
        crate::extensions::RunStatus::Ended { code: Some(0) },
    )]);
    assert_eq!(applied.len(), 1, "the observation advanced the sequence");
    let sequence = controls.preparation("run-1").unwrap();
    assert_eq!(sequence.current_index(), 1);
    assert_eq!(
        sequence.current_step().map(|step| step.name.as_str()),
        Some("生成")
    );
    // A failing end blocks the launch and names the step that caused it.
    let second = controls.begin("run-1");
    controls.note_step_request("run-1", 1, second);
    controls.adopt_step_sessions(&[(8, second, Some("2".into()))]);
    let poll = controls.begin_poll("run-1", 1, 8);
    let applied = controls.reconcile_run_status(&[(
        "run-1".into(),
        poll,
        crate::extensions::RunStatus::Ended { code: Some(4) },
    )]);
    assert_eq!(applied.len(), 1);
    let sequence = controls.preparation("run-1").unwrap();
    assert!(!sequence.is_active());
    assert!(
        sequence
            .blocked_by()
            .is_some_and(|reason| reason.contains('4'))
    );
}

/// Two isolated workspaces read one shared configuration, each resolving to its own project.
#[test]
fn two_workspaces_resolve_one_shared_configuration_separately() {
    let project = tempfile::tempdir().unwrap().keep();
    let local = tempfile::tempdir().unwrap().keep();
    let workspace = project.display().to_string();
    let mut controls =
        RunControls::load_with_project(&workspace, Some(local.clone()), Some(project.clone()));
    let mut configuration = config("run-1", "共享");
    configuration.directory = Some(workspace.clone());
    // This machine's own value, which the other machine must not be able to read from the project.
    configuration.env = [("SECRET".to_owned(), "s3cret".to_owned())].into();
    configuration.local = false;
    controls.upsert(configuration, &workspace).unwrap();

    // A second workspace opens the same project file and resolves the token to its own root.
    let other = tempfile::tempdir().unwrap().keep();
    std::fs::create_dir_all(other.join(".editor")).unwrap();
    std::fs::copy(
        editor_core::project_path(&project),
        editor_core::project_path(&other),
    )
    .unwrap();
    let second = RunControls::load_with_project(
        &other.display().to_string(),
        Some(local.clone()),
        Some(other.clone()),
    );
    let resolved = second
        .configuration("run-1")
        .expect("the shared entry is loaded by the second workspace");
    assert_eq!(
        resolved.directory.as_deref(),
        Some(other.display().to_string().as_str()),
        "each workspace resolves the token to its own project"
    );
    assert!(
        resolved.env.is_empty(),
        "another machine's values are not shared with this one"
    );

    // A hand edit to the project's file reaches the next load through the same validation.
    let mut shared = editor_core::load_shared(&project).unwrap();
    shared.configurations[0].name = "项目改名".into();
    editor_core::save_shared(&project, &shared).unwrap();
    let reloaded =
        RunControls::load_with_project(&workspace, Some(local.clone()), Some(project.clone()));
    assert_eq!(
        reloaded
            .configuration("run-1")
            .map(|config| config.name.as_str()),
        Some("项目改名")
    );
    assert_eq!(
        reloaded
            .configuration("run-1")
            .and_then(|config| config.env.get("SECRET"))
            .map(String::as_str),
        Some("s3cret"),
        "the hand edit did not disturb this machine's own values"
    );
}

/// A saved or hand-edited configuration never retargets a session that already started.
#[test]
fn a_running_session_keeps_the_snapshot_it_started_with() {
    let mut controls = controls();
    let workspace = "C:/work".to_owned();
    controls
        .upsert(config("run-1", "发起"), &workspace)
        .unwrap();
    let plan = controls
        .prepare_launch("run-1", &workspace, 16)
        .expect("the plan is usable");
    let launch = controls.begin("run-1");
    controls.begin_sequence("run-1", plan, launch);
    controls.adopt_step_sessions(&[(7, launch, Some("1".into()))]);
    let started = controls
        .preparation("run-1")
        .and_then(|sequence| sequence.planned_request(0))
        .cloned()
        .expect("the sequence holds the request it started");

    // The configuration is edited while the session runs.
    let mut edited = config("run-1", "改过的");
    edited.target = RunTarget::Program {
        program: "other.exe".into(),
        args: vec!["--new".into()],
    };
    controls.upsert(edited, &workspace).unwrap();
    // The running session still reports what it started with: a save only affects later executions.
    assert_eq!(
        controls
            .preparation("run-1")
            .and_then(|sequence| sequence.planned_request(0))
            .cloned(),
        Some(started),
        "the plan is fixed when the launch is accepted"
    );
    // A later launch uses the edited definition.
    let next = controls
        .prepare_launch("run-1", &workspace, 16)
        .expect("the edited configuration is usable");
    assert_eq!(next.steps[0].request.program, "other.exe");
    assert_eq!(
        controls
            .configuration("run-1")
            .map(|config| config.name.as_str()),
        Some("改过的")
    );
}

/// A configuration loaded from a project file prepares the launch the file describes.
///
/// This is the whole host-side path a shared configuration travels: the project's file, this
/// machine's overrides, the merged record, and the request the runtime would be asked to start.
#[test]
fn a_loaded_shared_configuration_prepares_the_request_it_describes() {
    let project = tempfile::tempdir().unwrap().keep();
    let local = tempfile::tempdir().unwrap().keep();
    let workspace = project.display().to_string();
    let mut set = editor_core::SharedSet::default();
    set.upsert(editor_core::SharedConfig {
        id: "shared-run".into(),
        name: "共享运行".into(),
        target: RunTarget::Program {
            program: "shared.exe".into(),
            args: vec!["--from-project".into(), "a b".into()],
        },
        directory: Some(editor_core::WORKSPACE_TOKEN.to_owned()),
        build: crate::run::parse_steps("构建 = cargo.exe | build").unwrap(),
        prelaunch: crate::run::parse_steps("准备 = tool.exe | gen").unwrap(),
        breakpoints: Default::default(),
    });
    editor_core::save_shared(&project, &set).unwrap();
    // This machine's own values for the same identity travel separately.
    let mut overrides = editor_core::RunConfigSet::default();
    let mut mine = config("shared-run", "本机名字");
    mine.env = [("SECRET".to_owned(), "s3cret".to_owned())].into();
    mine.tool_paths = vec![project.join("tools").display().to_string()];
    mine.source = editor_core::RunConfigSource::Project;
    mine.from_target = None;
    mine.local = false;
    overrides.upsert(mine).unwrap();
    editor_core::save(&local, &workspace, &overrides).unwrap();

    let controls = RunControls::load_with_project(&workspace, Some(local), Some(project.clone()));
    assert!(controls.error.is_none(), "{:?}", controls.error);
    let plan = controls
        .prepare_launch("shared-run", &workspace, MAX_PREPARED_STEPS)
        .expect("the shared definition is launchable");
    // The order is the project's: its build, its step, then the program it names.
    assert_eq!(
        plan.steps.iter().map(|step| step.kind).collect::<Vec<_>>(),
        vec![StepKind::Build, StepKind::Prelaunch, StepKind::Program]
    );
    assert_eq!(plan.steps[0].request.program, "cargo.exe");
    assert_eq!(plan.steps[2].request.program, "shared.exe");
    assert_eq!(
        plan.steps[2].request.args,
        vec!["--from-project", "a b"],
        "arguments stay literal items"
    );
    assert_eq!(
        plan.steps[2].request.cwd.as_deref(),
        Some(workspace.as_str()),
        "the token resolved to this workspace"
    );
    // This machine's own environment and tool directories reach the program.
    let environment = |name: &str| {
        plan.steps[2]
            .request
            .env
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
            .map(|entry| entry.value.clone())
    };
    assert_eq!(environment("SECRET").as_deref(), Some("s3cret"));
    assert!(
        environment("PATH")
            .is_some_and(|path| path.starts_with(&project.join("tools").display().to_string())),
        "this machine's tool directory leads the program's search order"
    );
    // Nothing about this machine travelled with the shared definition.
    let bytes = std::fs::read_to_string(editor_core::project_path(&project)).unwrap();
    assert!(!bytes.contains("SECRET") && !bytes.contains("s3cret"));
    assert!(!bytes.contains("tools"));
}

/// A discovery corrects a target that moved, offers new ones, and marks the ones that are gone.
#[test]
fn a_discovery_reports_what_changed_without_changing_the_users_work() {
    use plugin_schema::DiscoveredTarget;
    let target = |id: &str, program: &str| DiscoveredTarget {
        id: id.to_owned(),
        provider: "rust-binary".into(),
        target_type: "rust-binary".into(),
        program: program.to_owned(),
        label: id.rsplit(':').next().unwrap_or(id).to_owned(),
        fields: Default::default(),
        found_in: "Cargo.toml".into(),
    };
    let mut controls = controls();
    let workspace = "C:/work".to_owned();
    // One configuration came from a target, one was written by hand.
    let mut mine = editor_core::configuration_for(
        &target("rust-binary:my-app", "my-app"),
        "run-1".into(),
        "我的程序".into(),
    );
    mine.target = RunTarget::Program {
        program: "my-app".into(),
        args: vec!["--verbose".into()],
    };
    controls.upsert(mine, &workspace).unwrap();
    controls
        .upsert(config("run-2", "手写"), &workspace)
        .unwrap();

    // Nothing has been discovered yet, so nothing is reported as invalid.
    assert!(!controls.discovery_ran());
    assert!(controls.invalid_targets().is_empty());

    // A discovery that still offers the target and adds one more.
    let report = controls.reconcile_discovered(&[
        target("rust-binary:my-app", "my-app"),
        target("rust-binary:other", "other"),
    ]);
    controls.note_discovery();
    assert!(
        report.repaired.is_empty(),
        "nothing changed for the stored one"
    );
    assert_eq!(report.offered, vec!["rust-binary:other".to_owned()]);
    assert!(report.missing.is_empty());
    assert!(controls.discovery_ran());
    assert!(
        !controls.menu_entries().iter().any(|entry| {
            matches!(entry, RunMenuEntry::Configuration { label, .. } if label.contains('⚠'))
        }),
        "a target that is still offered is not marked invalid"
    );

    // The same target now names another program: only the program changes.
    let report = controls.reconcile_discovered(&[target("rust-binary:my-app", "renamed-app")]);
    assert_eq!(report.repaired, vec!["我的程序".to_owned()]);
    let repaired = controls.configuration("run-1").expect("still stored");
    assert_eq!(repaired.target.executable(), "renamed-app");
    assert_eq!(
        repaired.literal_arguments(),
        vec!["--verbose"],
        "the user's arguments survive the correction"
    );
    assert_eq!(repaired.name, "我的程序");

    // The target disappears: the configuration is marked where it is chosen, not deleted.
    let report = controls.reconcile_discovered(&[]);
    assert_eq!(
        report.missing,
        vec![("run-1".to_owned(), "我的程序".to_owned())]
    );
    assert!(controls.configuration("run-1").is_some());
    assert!(controls.target_missing("run-1"));
    // The list a caller acts on names the same configuration the report did, and names only that one.
    // Without this the empty case above would be the method's only coverage, and a method that never
    // reports anything would look tested.
    assert_eq!(
        controls.invalid_targets(),
        vec![("run-1".to_owned(), "我的程序".to_owned())],
        "the lost target is reported with the name the user knows"
    );
    assert!(
        !controls.target_missing("run-2"),
        "a hand-written configuration has no target to lose"
    );
    let labels = controls
        .menu_entries()
        .into_iter()
        .filter_map(|entry| match entry {
            RunMenuEntry::Configuration { label, .. } => Some(label),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        labels.iter().any(|label| label.contains("目标已失效")),
        "{labels:?}"
    );
}

/// Confirming a candidate stores one configuration, and confirming it again reuses that one.
#[test]
fn confirming_a_candidate_stores_it_once() {
    use plugin_schema::DiscoveredTarget;
    let target = |id: &str, label: &str, target_type: &str| DiscoveredTarget {
        id: id.to_owned(),
        provider: "rust-binary".into(),
        target_type: target_type.to_owned(),
        program: label.to_owned(),
        label: label.to_owned(),
        fields: Default::default(),
        found_in: "Cargo.toml".into(),
    };
    let mut controls = controls();
    let workspace = "C:/work".to_owned();
    controls.reconcile_discovered(&[
        target("rust-binary:my-app", "my-app", "rust-binary"),
        target("tools:fmt", "fmt", "tool-command"),
    ]);
    controls.note_discovery();

    // Both candidates are offered, and neither has been stored yet.
    let offered = controls
        .menu_entries()
        .into_iter()
        .filter_map(|entry| match entry {
            RunMenuEntry::Target {
                label, target_type, ..
            } => Some((label, target_type)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        offered,
        vec![
            ("my-app".to_owned(), "rust-binary".to_owned()),
            ("fmt".to_owned(), "tool-command".to_owned())
        ],
        "a candidate from any provider is offered the same way"
    );
    assert!(
        controls.configurations().is_empty(),
        "discovery stores nothing"
    );

    // Confirming one stores exactly that target, named as the user saw it.
    let stored = controls
        .confirm_target("rust-binary:my-app", &workspace)
        .expect("the candidate is confirmed");
    assert_eq!(controls.configurations().len(), 1);
    let configuration = controls.configuration(&stored).expect("it is stored");
    assert_eq!(configuration.name, "my-app");
    assert_eq!(configuration.target.executable(), "my-app");
    assert_eq!(
        configuration.from_target.as_deref(),
        Some("rust-binary:my-app")
    );
    // The stored one is selected, so the form that opens next edits what was just confirmed.
    assert_eq!(
        controls.selected().map(|config| config.id.as_str()),
        Some(stored.as_str())
    );

    // Confirming the same candidate again reuses the stored configuration.
    let again = controls
        .confirm_target("rust-binary:my-app", &workspace)
        .expect("confirming again resolves");
    assert_eq!(again, stored);
    assert_eq!(controls.configurations().len(), 1);

    // A confirmed candidate is no longer offered, and an unknown one is refused by name.
    let labels = controls
        .menu_entries()
        .into_iter()
        .filter_map(|entry| match entry {
            RunMenuEntry::Target { label, .. } => Some(label),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(labels, vec!["fmt".to_owned()]);
    let error = controls
        .confirm_target("nobody:missing", &workspace)
        .expect_err("an unknown candidate is refused");
    assert!(error.contains("nobody:missing"), "{error}");
}

/// A configuration can follow the default or ask for a provider, and says when its own is unusable.
#[test]
fn a_configuration_can_ask_for_one_provider() {
    use plugin_runtime::ProviderCandidate;
    let candidate = |plugin: &str, unavailable: Option<&str>, selected: bool| ProviderCandidate {
        plugin: plugin.to_owned(),
        unavailable: unavailable.map(str::to_owned),
        selected,
    };
    let mut controls = controls();
    let workspace = "C:/work".to_owned();
    controls
        .upsert(config("run-1", "第一个"), &workspace)
        .unwrap();

    // A stored configuration follows the default until it is told otherwise.
    assert_eq!(
        controls.configuration("run-1").unwrap().provider,
        None,
        "a new configuration follows the default"
    );
    let providers = vec![
        candidate("terminal", None, true),
        candidate("broken", Some("与当前宿主不兼容"), false),
    ];
    assert!(
        controls.provider_unavailable("run-1", &providers).is_none(),
        "following the default is never invalid"
    );

    // Asking for a provider records it on the configuration, so it persists with the configuration.
    controls
        .choose_provider("run-1", Some("terminal"), &workspace)
        .unwrap();
    assert_eq!(
        controls.configuration("run-1").unwrap().provider.as_deref(),
        Some("terminal")
    );
    assert!(controls.provider_unavailable("run-1", &providers).is_none());

    // A provider that cannot run is named with its reason instead of being used anyway.
    controls
        .choose_provider("run-1", Some("broken"), &workspace)
        .unwrap();
    let reason = controls
        .provider_unavailable("run-1", &providers)
        .expect("an unusable provider is reported");
    assert!(reason.contains("不兼容"), "{reason}");

    // A provider that is not installed at all is reported as missing, never substituted.
    controls
        .choose_provider("run-1", Some("gone"), &workspace)
        .unwrap();
    let reason = controls
        .provider_unavailable("run-1", &providers)
        .expect("a missing provider is reported");
    assert!(reason.contains("gone"), "{reason}");

    // Asking for the default again clears the request rather than picking a provider.
    controls.choose_provider("run-1", None, &workspace).unwrap();
    assert_eq!(controls.configuration("run-1").unwrap().provider, None);
    assert!(controls.provider_unavailable("run-1", &providers).is_none());
    // An unknown configuration is refused by name.
    let error = controls
        .choose_provider("nobody", Some("terminal"), &workspace)
        .expect_err("an unknown configuration is refused");
    assert!(error.contains("nobody"), "{error}");
}

/// A debug click is refused with a reason, and never becomes an ordinary run.
#[test]
fn a_debug_launch_is_refused_rather_than_replaced_by_a_plain_run() {
    let mut controls = controls();
    let workspace = "C:/work".to_owned();
    controls
        .upsert(config("run-1", "第一个"), &workspace)
        .unwrap();

    // Before the host has answered, nothing is offered: a capability may not be assumed.
    assert!(controls.debug_availability().is_err());
    let reason = controls
        .debug_blocker("run-1")
        .expect("an unconfirmed capability blocks debugging");
    assert!(reason.contains("尚未确认"), "{reason}");

    // No provider installed is reported as such, not as a reason to run plainly.
    controls.note_debug_availability(Err("没有安装提供调试能力的插件".into()));
    let reason = controls
        .debug_blocker("run-1")
        .expect("a missing capability blocks debugging");
    assert!(reason.contains("没有安装"), "{reason}");

    // A provider that cannot serve is reported with its own reason.
    controls.note_debug_availability(Err(
        "已安装的调试提供者都不能用：adapter（与当前宿主不兼容）".into(),
    ));
    let reason = controls
        .debug_blocker("run-1")
        .expect("an unusable provider blocks");
    assert!(reason.contains("adapter"), "{reason}");
    // The configuration itself is still launchable: only debugging is unavailable.
    assert!(controls.launch_blocker("run-1").is_none());

    // A confirmed capability names the provider, and a usable configuration is debugged.
    controls.note_debug_availability(Ok("adapter".into()));
    assert_eq!(controls.debug_availability(), Ok("adapter"));
    assert!(controls.debug_blocker("run-1").is_none());

    // A configuration that cannot run cannot be debugged either, and the two controls agree because
    // they ask the same question.
    let mut broken = config("run-2", "坏的");
    broken.target = RunTarget::Program {
        program: String::new(),
        args: Vec::new(),
    };
    // The store refuses to save this, which is right; the question here is what the controls do with
    // one that is already stored and unusable, so it is placed directly.
    let mut stored = editor_core::RunConfigSet::default();
    stored.configurations.push(broken);
    let mut broken_controls = self::controls();
    broken_controls.replace_configs(stored);
    broken_controls.note_debug_availability(Ok("adapter".into()));
    assert!(
        broken_controls.debug_blocker("run-2").is_some(),
        "a configuration that cannot run cannot be debugged"
    );
    assert_eq!(
        broken_controls.debug_blocker("run-2"),
        broken_controls.launch_blocker("run-2"),
        "the debug control and the launch path agree about a broken configuration"
    );
    // An unknown configuration is refused by the same path rather than starting something else.
    assert!(broken_controls.debug_blocker("nobody").is_some());
}

/// Breakpoints survive the edit round trip, and an unusable line is refused with its reason.
#[test]
fn breakpoints_round_trip_through_the_edit_form() {
    let mut controls = controls();
    let workspace = "C:/work".to_owned();
    controls
        .upsert(config("run-1", "第一个"), &workspace)
        .unwrap();

    // A whole list can be pasted at once; the drive letter is not mistaken for the separator.
    let parsed =
        parse_breakpoints("C:\\work\\src\\main.rs:12\nsrc/lib.rs:4\n\n# 注释\nsrc/main.rs:9")
            .expect("the list parses");
    assert_eq!(parsed.len(), 3, "{parsed:?}");
    assert!(parsed.contains("src/main.rs", 9));
    // The same location twice is a correction, not a second breakpoint.
    let repeated = parse_breakpoints("src/main.rs:9\nsrc/main.rs:9").expect("a repeat is accepted");
    assert_eq!(repeated.len(), 1);

    // A line that is not a location is refused by name rather than dropped silently.
    let error = parse_breakpoints("src/main.rs").expect_err("a location needs a line");
    assert!(error.contains("源文件:行号"), "{error}");
    let error = parse_breakpoints("src/main.rs:abc").expect_err("a line is a number");
    assert!(error.contains("数字"), "{error}");
    let error = parse_breakpoints("src/main.rs:0").expect_err("line zero is not a location");
    assert!(error.contains('1'), "{error}");

    // The stored configuration keeps them, and the draft renders them back the way they were read.
    let mut draft = RunConfigDraft::from_config(controls.configuration("run-1"), "run-1".into());
    draft.breakpoints = "src/main.rs:9\nsrc/lib.rs:4\nC:\\work\\src\\main.rs:12".into();
    controls
        .upsert(draft.to_config().expect("the draft is valid"), &workspace)
        .unwrap();
    let stored = controls.configuration("run-1").expect("still stored");
    assert_eq!(stored.breakpoints.len(), 3);
    let rendered = render_breakpoints(&stored.breakpoints);
    assert_eq!(
        parse_breakpoints(&rendered).expect("the rendering parses back"),
        stored.breakpoints,
        "rendering and parsing agree, so the form does not change what is stored"
    );
    assert!(
        rendered
            .lines()
            .next()
            .unwrap()
            .starts_with("C:\\work\\src"),
        "the list reads in project order: {rendered}"
    );
}

/// The debug controls follow the session's own state, not just whether debugging is available.
#[test]
fn debug_controls_follow_the_session_state() {
    use editor_core::DebugSessionState;
    let mut controls = controls();
    // Nothing is confirmed yet, so nothing is offered — including stopping.
    assert!(!controls.debug_controls().can_start());
    assert!(!controls.debug_controls().can_stop());

    controls.note_debug_availability(Ok("adapter".into()));
    // A provider that has not said what it can do offers no ability-dependent control.
    assert!(controls.debug_controls().pause.is_err());
    assert!(
        controls.debug_controls().step[0]
            .1
            .as_ref()
            .unwrap_err()
            .contains("单步"),
        "an undeclared ability is reported rather than assumed"
    );
    // With the provider's own declaration, the abilities follow it.
    controls.note_debug_capabilities(editor_core::DebugCapabilities {
        breakpoints: true,
        resume_pause: true,
        step: true,
        inspect: true,
    });
    // Confirmed and idle: starting is offered and stopping is not.
    assert!(controls.debug_controls().can_start());
    assert!(!controls.debug_controls().can_stop());

    // Connecting is a state a user needs a way out of.
    controls.note_debug_state("run-1", DebugSessionState::Starting);
    assert!(controls.debug_controls().can_stop());
    assert!(!controls.debug_controls().can_start());

    // Running: pausing is offered, resuming is not, and starting a second session is refused.
    controls.note_debug_state("run-1", DebugSessionState::Running);
    let running = controls.debug_controls();
    assert!(running.pause.is_ok() && running.resume.is_err());
    assert!(running.start.is_err() && running.can_stop());

    // Paused: the location is carried, resuming is offered, pausing is not.
    controls.note_debug_state(
        "run-1",
        DebugSessionState::Paused {
            source: "src/main.rs".into(),
            line: 12,
            reason: Some("breakpoint".into()),
        },
    );
    assert_eq!(
        controls.debug_state().paused_at(),
        Some(("src/main.rs", 12)),
        "the panel shows where the target stopped"
    );
    let paused = controls.debug_controls();
    assert!(paused.resume.is_ok() && paused.pause.is_err());

    // A failed session reports the provider's own reason and offers only a new start.
    controls.note_debug_state(
        "run-1",
        DebugSessionState::Failed {
            reason: "适配器退出".into(),
        },
    );
    let failed = controls.debug_controls();
    assert!(failed.can_start() && !failed.can_stop());
    assert!(failed.stop.unwrap_err().contains("适配器退出"));

    // Every state and availability pair leaves an unavailable control with a reason.
    for availability in [Ok("adapter".to_owned()), Err("无提供者".to_owned())] {
        controls.note_debug_availability(availability);
        for state in [
            DebugSessionState::Disconnected,
            DebugSessionState::Starting,
            DebugSessionState::Running,
            DebugSessionState::Paused {
                source: "src/main.rs".into(),
                line: 1,
                reason: None,
            },
            DebugSessionState::Exited,
            DebugSessionState::Failed {
                reason: "失败".into(),
            },
        ] {
            controls.note_debug_state("run-1", state.clone());
            let derived = controls.debug_controls();
            let mut outcomes = vec![
                &derived.start,
                &derived.resume,
                &derived.pause,
                &derived.stop,
            ];
            outcomes.extend(derived.step.iter().map(|(_, outcome)| outcome));
            for outcome in outcomes {
                if let Err(reason) = outcome {
                    assert!(!reason.trim().is_empty(), "{state:?} has a silent refusal");
                }
            }
        }
    }
}

/// The inspection view belongs to the selected session, and a late answer cannot replace it.
#[test]
fn inspection_follows_the_selected_session() {
    use editor_core::{DebugSessionState, InspectionError, StackFrame};
    let mut controls = controls();
    let workspace = "C:/work".to_owned();
    controls
        .upsert(config("run-1", "第一个"), &workspace)
        .unwrap();
    controls
        .upsert(config("run-2", "第二个"), &workspace)
        .unwrap();

    // Two configurations debugged together: each keeps its own state.
    controls.note_debug_state(
        "run-1",
        DebugSessionState::Paused {
            source: "src/main.rs".into(),
            line: 12,
            reason: Some("breakpoint".into()),
        },
    );
    controls.note_debug_state("run-2", DebugSessionState::Running);
    assert_eq!(
        controls.debug_sessions().count(),
        2,
        "both sessions are listed"
    );
    // The panel starts on the first session, and pausing it did not pause the other.
    assert_eq!(
        controls.debug_session().map(|(config, _)| config),
        Some("run-1")
    );
    assert_eq!(
        controls
            .debug_session_of("run-2")
            .map(|session| session.state().clone()),
        Some(DebugSessionState::Running)
    );

    // Frames apply to the selected session's current pause.
    let scope = controls.begin_debug_pause().expect("a selected session");
    controls
        .apply_debug_frames(
            scope,
            vec![StackFrame {
                id: 0,
                name: "probe::add".into(),
                source: "src/main.rs".into(),
                line: 12,
            }],
        )
        .expect("the pause is described");
    assert_eq!(controls.debug_frames().len(), 1);
    assert_eq!(controls.debug_location(), Some(("src/main.rs", 12)));
    controls
        .apply_debug_variables(
            scope,
            0,
            vec![editor_core::DebugVariable {
                name: "left".into(),
                value: "2".into(),
            }],
        )
        .expect("frame 0's variables");
    assert_eq!(controls.debug_variables(0).len(), 1);

    // Selecting the other session changes what the panel describes, and it has no pause data.
    assert!(controls.select_debug_session("run-2"));
    assert!(controls.debug_frames().is_empty());
    assert_eq!(controls.debug_location(), None);

    // A late answer about the first session's pause is refused, so it cannot replace this view.
    assert_eq!(
        controls.apply_debug_frames(
            scope,
            vec![StackFrame {
                id: 9,
                name: "late".into(),
                source: "src/lib.rs".into(),
                line: 3,
            }]
        ),
        Err(InspectionError::StalePause)
    );
    assert!(controls.debug_frames().is_empty());

    // Going back to the first session shows its own frames again, described for its own pause.
    assert!(controls.select_debug_session("run-1"));
    assert_eq!(controls.debug_frames().len(), 1);
    assert_eq!(
        controls.select_debug_frame(0),
        Ok(()),
        "the frame belongs to this pause"
    );
    assert_eq!(controls.debug_location(), Some(("src/main.rs", 12)));

    // A state that is no longer a pause ends that session's data, and only that session's.
    controls.note_debug_state("run-1", DebugSessionState::Running);
    assert!(controls.debug_frames().is_empty());
    assert_eq!(controls.debug_pause_scope(), None);

    // Ending a session hands the panel to one that is still there.
    controls.end_debug_session("run-1");
    assert_eq!(
        controls.debug_session().map(|(config, _)| config),
        Some("run-2")
    );
    controls.end_debug_session("run-2");
    assert!(controls.debug_session().is_none());
    assert_eq!(
        controls.debug_state(),
        DebugSessionState::Disconnected,
        "with no session the panel reports nothing rather than a stale state"
    );
    assert_eq!(controls.begin_debug_pause(), None);
    assert_eq!(
        controls.apply_debug_frames(scope, Vec::new()),
        Err(InspectionError::NoSession)
    );
}

/// The panel describes the selected session's frames and variables, and says where they are.
#[test]
fn the_panel_describes_the_selected_sessions_pause() {
    use editor_core::{DebugSessionState, DebugVariable, StackFrame};
    let mut controls = controls();
    let workspace = "C:/work".to_owned();
    controls
        .upsert(config("run-1", "第一个"), &workspace)
        .unwrap();
    controls
        .upsert(config("run-2", "第二个"), &workspace)
        .unwrap();
    controls.note_debug_state(
        "run-1",
        DebugSessionState::Paused {
            source: "src/main.rs".into(),
            line: 2,
            reason: Some("breakpoint".into()),
        },
    );
    controls.note_debug_state("run-2", DebugSessionState::Running);

    // Nothing is described before any frame arrives, and there is no location to reveal.
    assert!(controls.debug_panel_rows().frames.is_empty());
    assert_eq!(controls.debug_panel_rows().location, None);
    let scope = controls.begin_debug_pause().expect("a selected session");
    controls
        .apply_debug_frames(
            scope,
            vec![
                StackFrame {
                    id: 0,
                    name: "probe::add".into(),
                    source: "src/main.rs".into(),
                    line: 2,
                },
                StackFrame {
                    id: 1,
                    name: "probe::main".into(),
                    source: "src/main.rs".into(),
                    line: 7,
                },
            ],
        )
        .expect("the pause is described");
    controls
        .apply_debug_variables(
            scope,
            0,
            vec![
                DebugVariable {
                    name: "left".into(),
                    value: "2".into(),
                },
                DebugVariable {
                    name: "text".into(),
                    value: "\"a b\"".into(),
                },
            ],
        )
        .expect("frame 0's variables");

    let rows = controls.debug_panel_rows();
    assert_eq!(rows.frames.len(), 2);
    assert_eq!(rows.frames[0].selector, "run-debug-frame-0");
    assert_eq!(rows.frames[0].label, "probe::add  src/main.rs:2");
    assert!(
        rows.frames[0].selected,
        "the stopping frame is selected first"
    );
    assert!(!rows.frames[1].selected);
    assert_eq!(rows.location.as_deref(), Some("src/main.rs:2"));
    // Values are the provider's rendering, including its own quoting.
    assert_eq!(rows.variables.len(), 2);
    assert_eq!(rows.variables[1].label, "text = \"a b\"");
    assert_eq!(rows.variables[1].selector, "run-debug-variable-0-text");
    assert!(
        !rows.another_paused,
        "the selected session is the one stopped; the other is running"
    );

    // Selecting the second frame moves the location and shows that frame's own variables.
    controls.select_debug_frame(1).expect("the frame exists");
    let rows = controls.debug_panel_rows();
    assert_eq!(rows.location.as_deref(), Some("src/main.rs:7"));
    assert!(rows.frames[1].selected && !rows.frames[0].selected);
    assert!(rows.variables.is_empty(), "frame 1 was never read");

    // Selecting the running session describes nothing of it, and notices the other is stopped.
    assert!(controls.select_debug_session("run-2"));
    let rows = controls.debug_panel_rows();
    assert!(rows.frames.is_empty());
    assert_eq!(rows.location, None);
    assert!(
        rows.another_paused,
        "a breakpoint in run-1 must not move the view while run-2 is being read"
    );
}

/// Answers are joined to the request they answer, and a late one cannot replace a newer view.
#[test]
fn only_the_answer_to_a_request_reaches_the_view() {
    use editor_core::{DebugSessionState, DebugVariable, InspectionError, StackFrame};
    let mut controls = controls();
    let workspace = "C:/work".to_owned();
    controls
        .upsert(config("run-1", "第一个"), &workspace)
        .unwrap();
    controls.note_debug_state(
        "run-1",
        DebugSessionState::Paused {
            source: "src/main.rs".into(),
            line: 2,
            reason: None,
        },
    );

    // Nothing can be asked for before a pause exists.
    assert_eq!(
        controls.begin_debug_request(DebugMethod::Frames, None),
        None
    );
    let scope = controls.begin_debug_pause().expect("a selected session");
    let frames = controls
        .begin_debug_request(DebugMethod::Frames, None)
        .expect("a pause is being inspected");
    assert_eq!(controls.pending_debug_requests(), 1);
    // Asking twice about one pause would leave two answers racing to describe it.
    assert_eq!(
        controls.begin_debug_request(DebugMethod::Frames, None),
        None
    );
    assert_eq!(controls.pending_debug_requests(), 1);

    let described = vec![StackFrame {
        id: 0,
        name: "probe::add".into(),
        source: "src/main.rs".into(),
        line: 2,
    }];
    controls
        .apply_debug_answer(frames, Some(described.clone()), None)
        .expect("the answer matches its request");
    assert_eq!(controls.debug_frames().len(), 1);
    assert_eq!(
        controls.pending_debug_requests(),
        0,
        "an answered request is no longer awaited"
    );

    // A request for one frame's variables names the frame, and an answer to nothing is refused.
    let variables = controls
        .begin_debug_request(DebugMethod::Variables, Some(0))
        .expect("a pause is being inspected");
    controls
        .apply_debug_answer(
            variables,
            None,
            Some(vec![DebugVariable {
                name: "left".into(),
                value: "2".into(),
            }]),
        )
        .expect("the answer matches its request");
    assert_eq!(controls.debug_variables(0).len(), 1);
    assert_eq!(
        controls.apply_debug_answer(999, Some(Vec::new()), None),
        Err(InspectionError::NoSession),
        "an answer to a request this editor never sent cannot arrive"
    );

    // A step ends the pause; the answer still on its way about it is refused, so the view is not
    // replaced by a description of a moment the target has left.
    let late = controls
        .begin_debug_request(DebugMethod::Frames, None)
        .expect("a pause is being inspected");
    let next = controls.begin_debug_pause().expect("a new pause");
    assert_ne!(next, scope);
    assert_eq!(
        controls.apply_debug_answer(late, Some(described), None),
        Err(InspectionError::StalePause)
    );
    assert!(
        controls.debug_frames().is_empty(),
        "the late answer did not describe the new pause"
    );

    // A malformed answer is dropped rather than awaited forever.
    let abandoned = controls
        .begin_debug_request(DebugMethod::Frames, None)
        .expect("a pause is being inspected");
    controls.abandon_debug_request(abandoned);
    assert_eq!(controls.pending_debug_requests(), 0);
    assert_eq!(
        controls.apply_debug_answer(abandoned, Some(Vec::new()), None),
        Err(InspectionError::NoSession)
    );
}

/// A step is answered by a new state, which begins a new pause and retires the old one.
#[test]
fn a_step_answer_begins_the_next_pause() {
    use editor_core::{DebugSessionState, DebugStep, InspectionError, StackFrame};
    let mut controls = controls();
    let workspace = "C:/work".to_owned();
    controls
        .upsert(config("run-1", "第一个"), &workspace)
        .unwrap();
    controls.note_debug_state(
        "run-1",
        DebugSessionState::Paused {
            source: "src/main.rs".into(),
            line: 2,
            reason: Some("breakpoint".into()),
        },
    );
    let scope = controls.begin_debug_pause().expect("a session is selected");
    controls
        .apply_debug_frames(
            scope,
            vec![StackFrame {
                id: 0,
                name: "probe::add".into(),
                source: "src/main.rs".into(),
                line: 2,
            }],
        )
        .expect("the pause is described");
    assert_eq!(controls.debug_frames().len(), 1);

    // A step names the direction, and a second step in the same pause is refused: two answers would
    // race to describe the same move.
    let step = controls
        .begin_debug_request(DebugMethod::Step(DebugStep::Over), None)
        .expect("a pause is being inspected");
    assert_eq!(
        controls.begin_debug_request(DebugMethod::Step(DebugStep::Over), None),
        None
    );
    assert_eq!(
        controls.begin_debug_request(DebugMethod::Step(DebugStep::Into), None),
        Some(step + 1),
        "a different direction is a different request"
    );
    controls.abandon_debug_request(step + 1);

    // The provider answers with a new state. The target is stopped somewhere else now, so the old
    // frames stop describing it and a new pause begins.
    controls
        .apply_debug_step(
            step,
            DebugSessionState::Paused {
                source: "src/main.rs".into(),
                line: 3,
                reason: Some("step".into()),
            },
        )
        .expect("the step answer matches its request");
    assert!(
        controls.debug_frames().is_empty(),
        "the frames described the moment before the step"
    );
    let next = controls.debug_pause_scope().expect("a new pause");
    assert_ne!(next, scope, "the new pause is its own moment");
    assert_eq!(
        controls.debug_state().paused_at(),
        Some(("src/main.rs", 3)),
        "the location is what the provider reported"
    );

    // A step answered by a running target ends the pause rather than inventing one.
    let running = controls
        .begin_debug_request(DebugMethod::Step(DebugStep::Out), None)
        .expect("a pause is being inspected");
    controls
        .apply_debug_step(running, DebugSessionState::Running)
        .expect("the step answer matches its request");
    assert_eq!(controls.debug_pause_scope(), None);

    // An answer about a pause that has ended is refused, as it is for a view.
    controls.note_debug_state(
        "run-1",
        DebugSessionState::Paused {
            source: "src/main.rs".into(),
            line: 9,
            reason: None,
        },
    );
    // The step belongs to the pause that is current when it is asked for; a later pause ends it.
    controls.begin_debug_pause().expect("a session is selected");
    let stale = controls
        .begin_debug_request(DebugMethod::Step(DebugStep::Into), None)
        .expect("a pause is being inspected");
    controls.begin_debug_pause().expect("a session is selected");
    assert_eq!(
        controls.apply_debug_step(
            stale,
            DebugSessionState::Paused {
                source: "src/main.rs".into(),
                line: 10,
                reason: None,
            }
        ),
        Err(InspectionError::StalePause)
    );
    // An answer to a view request is not an answer to a step.
    let view = controls
        .begin_debug_request(DebugMethod::Frames, None)
        .expect("a pause is being inspected");
    assert_eq!(
        controls.apply_debug_step(view, DebugSessionState::Running),
        Err(InspectionError::NoSession)
    );
}

/// A lifecycle change names the sessions it would take away, and a plugin serving nothing names none.
#[test]
fn a_plugin_lifecycle_change_names_the_sessions_it_affects() {
    use editor_core::DebugSessionState;
    let mut controls = controls();
    let workspace = "C:/work".to_owned();
    controls
        .upsert(config("run-1", "第一个"), &workspace)
        .unwrap();
    controls
        .upsert(config("run-2", "第二个"), &workspace)
        .unwrap();

    // Nothing is running or being debugged, so a change takes nothing away.
    let idle = controls.plugin_session_impact("adapter", Some("adapter"));
    assert!(idle.is_empty());
    assert_eq!(idle.summary(), None);

    // One session served by one plugin: it is named by the plugin that answered it, and by the
    // configuration name a user knows it by.
    let request_id = controls.begin("run-1");
    controls.reconcile(&[snapshot(
        9,
        "run-1",
        request_id,
        plugin_runtime::ExecutionState::Running,
    )]);
    let plugin = controls
        .sessions()
        .into_iter()
        .find(|session| session.id == 9)
        .map(|session| session.plugin)
        .expect("the session records the plugin that answered it");
    let impact = controls.plugin_session_impact(&plugin, Some("adapter"));
    assert_eq!(impact.running, vec!["第一个".to_owned()]);
    let summary = impact.summary().expect("a change takes a session away");
    assert!(summary.contains("第一个"), "{summary}");
    assert!(
        controls
            .plugin_session_impact("nobody", Some("adapter"))
            .running
            .is_empty(),
        "a plugin that is not serving this session is not credited with it"
    );

    // A finished session is not something a change takes away: it is already over.
    controls.reconcile(&[snapshot(
        9,
        "run-1",
        request_id,
        plugin_runtime::ExecutionState::Failed,
    )]);
    assert!(
        controls
            .plugin_session_impact(&plugin, Some("adapter"))
            .running
            .is_empty(),
        "a session that has already ended is not reported as running"
    );

    // The debug provider's own removal ends its sessions, and another plugin's does not.
    controls.note_debug_state(
        "run-2",
        DebugSessionState::Paused {
            source: "src/main.rs".into(),
            line: 2,
            reason: None,
        },
    );
    let impact = controls.plugin_session_impact("adapter", Some("adapter"));
    assert_eq!(impact.debugging, vec!["第二个".to_owned()]);
    assert!(impact.running.is_empty());
    let summary = impact.summary().expect("a debug session is affected");
    assert!(
        summary.contains("调试会话") && summary.contains("第二个"),
        "{summary}"
    );
    assert!(
        controls
            .plugin_session_impact("terminal", Some("adapter"))
            .debugging
            .is_empty(),
        "a plugin that is not the selected debug provider ends no debug session"
    );
    // With no debug provider selected, no plugin is credited with the debug sessions either.
    assert!(controls.plugin_session_impact("adapter", None).is_empty());
}

/// Stopping a session does not disturb its configuration, its breakpoints or the other sessions.
#[test]
fn stopping_a_session_leaves_everything_else_alone() {
    use editor_core::{DebugSessionState, StackFrame};
    let mut controls = controls();
    let workspace = "C:/work".to_owned();
    let mut mine = config("run-1", "第一个");
    mine.breakpoints
        .insert("src/main.rs", 7)
        .expect("the location is valid");
    controls.upsert(mine, &workspace).unwrap();
    controls
        .upsert(config("run-2", "第二个"), &workspace)
        .unwrap();

    // Two sessions running, one of them also paused in a debugger with frames described.
    let first = controls.begin("run-1");
    let second = controls.begin("run-2");
    controls.reconcile(&[
        snapshot(1, "run-1", first, plugin_runtime::ExecutionState::Running),
        snapshot(2, "run-2", second, plugin_runtime::ExecutionState::Running),
    ]);
    controls.note_debug_state(
        "run-1",
        DebugSessionState::Paused {
            source: "src/main.rs".into(),
            line: 7,
            reason: Some("breakpoint".into()),
        },
    );
    let scope = controls.begin_debug_pause().expect("a selected session");
    controls
        .apply_debug_frames(
            scope,
            vec![StackFrame {
                id: 0,
                name: "probe::main".into(),
                source: "src/main.rs".into(),
                line: 7,
            }],
        )
        .expect("the pause is described");
    let request = controls.begin_debug_request(DebugMethod::Frames, None);
    assert!(request.is_some(), "a request is in flight");

    // Stopping one session is a launch operation: it does not touch the debugger's pause, the other
    // session, the stored breakpoints or a request that is already in flight.
    let stop = controls.begin_stop("run-1", 1);
    controls.reconcile(&[snapshot(
        2,
        "run-2",
        second,
        plugin_runtime::ExecutionState::Running,
    )]);
    let reported = controls.reconcile_stops(&[("run-1".to_owned(), stop, Ok(()))]);
    assert_eq!(reported, vec![(1, Ok(()))]);
    assert!(controls.pending_debug_requests() >= 1);
    assert_eq!(
        controls.debug_frames().len(),
        1,
        "a pause in the debugger is not a launch session to stop"
    );
    assert_eq!(controls.debug_location(), Some(("src/main.rs", 7)));
    assert_eq!(
        controls.configuration("run-1").unwrap().breakpoints.len(),
        1,
        "breakpoints belong to the configuration, not to the session that stopped"
    );
    assert_eq!(
        controls.running_for("run-2").map(|session| session.id),
        Some(2),
        "the other session is untouched"
    );
    // A stop is answered once: a repeated answer is not a second stop.
    let repeated = controls.reconcile_stops(&[("run-1".to_owned(), stop, Ok(()))]);
    assert!(repeated.is_empty(), "the answer was already reported");
}

/// A stop that is never answered leaves the session as it was, rather than inventing an end.
#[test]
fn a_silent_provider_leaves_the_session_as_reported() {
    let mut controls = controls();
    let workspace = "C:/work".to_owned();
    controls
        .upsert(config("run-1", "第一个"), &workspace)
        .unwrap();
    let request_id = controls.begin("run-1");
    controls.reconcile(&[snapshot(
        1,
        "run-1",
        request_id,
        plugin_runtime::ExecutionState::Running,
    )]);
    let stop = controls.begin_stop("run-1", 1);

    // Nothing arrives: the last reported state stands. The host does not decide a program ended
    // because it stopped hearing about it.
    assert_eq!(
        controls.running_for("run-1").map(|session| session.state),
        Some(plugin_runtime::ExecutionState::Running),
        "silence is not an ending"
    );
    // A refusal is reported and also leaves the state as reported.
    assert_eq!(
        controls.reconcile_stops(&[("run-1".to_owned(), stop, Err("提供者不可达".into()))]),
        vec![(1, Err("提供者不可达".to_owned()))]
    );
    assert_eq!(
        controls.running_for("run-1").map(|session| session.state),
        Some(plugin_runtime::ExecutionState::Running)
    );
}

/// A control action withholds the others until its answer arrives, and names the provider's session.
#[test]
fn a_control_action_is_awaited_before_the_next_one() {
    use editor_core::{DebugSessionState, StackFrame};
    let mut controls = controls();
    let workspace = "C:/work".to_owned();
    controls
        .upsert(config("run-1", "第一个"), &workspace)
        .unwrap();
    // A capable provider is confirmed first, so an unavailable control would be about the action
    // rather than about a missing ability.
    controls.note_debug_availability(Ok("adapter".into()));
    controls.note_debug_capabilities(editor_core::DebugCapabilities {
        breakpoints: true,
        resume_pause: true,
        step: true,
        inspect: true,
    });
    controls.note_debug_state(
        "run-1",
        DebugSessionState::Paused {
            source: "src/main.rs".into(),
            line: 2,
            reason: None,
        },
    );
    // A session without the provider's own identity cannot be asked anything: there is nothing to
    // address the question to, so a call is never sent to a session the host invented.
    assert_eq!(controls.debug_provider_session(), None);

    // The identity is kept exactly as the provider reported it.
    controls.note_debug_provider_session("run-1", "provider-7");
    assert_eq!(
        controls.debug_provider_session().as_deref(),
        Some("provider-7")
    );

    // A different identity means a different session, so what described the old one is dropped.
    let scope = controls.begin_debug_pause().expect("a session is selected");
    controls
        .apply_debug_frames(
            scope,
            vec![StackFrame {
                id: 0,
                name: "f".into(),
                source: "src/main.rs".into(),
                line: 2,
            }],
        )
        .expect("the pause is described");
    assert_eq!(controls.debug_frames().len(), 1);
    controls.note_debug_provider_session("run-1", "provider-8");
    assert!(
        controls.debug_frames().is_empty(),
        "the frames belonged to the session that reported them"
    );
    assert_eq!(controls.debug_pause_scope(), None);

    // While an action is outstanding every control is withheld with a reason, because the session's
    // state is about to change and a second click would race the answer.
    controls.note_debug_state(
        "run-1",
        DebugSessionState::Paused {
            source: "src/main.rs".into(),
            line: 2,
            reason: None,
        },
    );
    assert!(controls.debug_controls().resume.is_ok());
    controls.note_debug_action();
    assert!(controls.debug_action_pending());
    let busy = controls.debug_controls();
    // Starting is unavailable for its own reason — a session is already being served — while every
    // control that would act on it is withheld because the action in flight is about to change it.
    assert!(busy.start.is_err());
    for outcome in [&busy.resume, &busy.pause, &busy.stop] {
        assert!(
            outcome.as_ref().unwrap_err().contains("正在进行"),
            "{outcome:?}"
        );
    }
    for (_, outcome) in &busy.step {
        assert!(outcome.as_ref().unwrap_err().contains("正在进行"));
    }
    // The answer releases them.
    controls.note_debug_action_finished();
    assert!(!controls.debug_action_pending());
    assert!(controls.debug_controls().resume.is_ok());
}

/// A start establishes the session its own configuration asked for, and says what it will run.
#[test]
fn a_start_request_carries_what_the_configuration_says() {
    use editor_core::DebugSessionState;
    let mut controls = controls();
    let workspace = "C:/work".to_owned();
    let mut mine = config("run-1", "第一个");
    mine.target = RunTarget::Program {
        program: "app.exe".into(),
        args: vec!["--flag".into(), "a b".into()],
    };
    mine.env = [("MODE".to_owned(), "dev".to_owned())].into();
    mine.breakpoints
        .insert("src/main.rs", 7)
        .expect("the location is valid");
    controls.upsert(mine, &workspace).unwrap();
    // A start names the configuration it begins, which is the selected one.
    assert!(controls.select("run-1", &workspace));

    // What a debug launch asks for is what a run would ask for, plus where to stop.
    let request = controls
        .debug_launch_request("run-1", &workspace)
        .expect("the configuration is launchable");
    assert_eq!(request["program"], "app.exe");
    assert_eq!(
        request["args"][1], "a b",
        "an argument with spaces stays one argument"
    );
    assert_eq!(request["breakpoints"][0]["source"], "src/main.rs");
    assert_eq!(request["breakpoints"][0]["line"], 7);
    assert_eq!(request["env"][0]["name"], "MODE");
    // A configuration without breakpoints omits the field rather than sending an empty list.
    controls
        .upsert(config("run-2", "第二个"), &workspace)
        .unwrap();
    let plain = controls
        .debug_launch_request("run-2", &workspace)
        .expect("the second configuration is launchable");
    assert!(plain.get("breakpoints").is_none());
    // A configuration that cannot be launched has no debug request either.
    assert!(
        controls
            .debug_launch_request("nobody", &workspace)
            .is_none()
    );

    // Starting is the one request that is not about a pause, so it is accepted while disconnected.
    controls.note_debug_availability(Ok("adapter".into()));
    let start = controls
        .begin_debug_request(DebugMethod::Start, None)
        .expect("starting is accepted before any pause exists");
    assert!(controls.debug_request_is_start(start));
    // A view request is still refused without a pause.
    assert_eq!(
        controls.begin_debug_request(DebugMethod::Frames, None),
        None
    );

    // The answer establishes the session under the identity the provider reported.
    controls
        .apply_debug_start(start, "provider-3", DebugSessionState::Running)
        .expect("the start answer matches its request");
    assert_eq!(
        controls.debug_provider_session().as_deref(),
        Some("provider-3")
    );
    assert_eq!(controls.debug_state(), DebugSessionState::Running);
    // An answer to a request this editor never sent cannot establish anything.
    assert!(
        controls
            .apply_debug_start(999, "provider-4", DebugSessionState::Running)
            .is_err()
    );
    // A view answer cannot be applied as a start.
    let start_again = controls
        .begin_debug_request(DebugMethod::Start, None)
        .expect("starting is accepted");
    assert!(
        controls
            .apply_debug_answer(start_again, Some(Vec::new()), None)
            .is_err()
    );
}

/// The editor follows a pause nobody asked for, and stays put for one the user asked for.
///
/// This is the half of breakpoint auto-location that can be stated without a debugger: which pause
/// moves the caret. A breakpoint hit is the case where being shown the source is the point; after the
/// user resumes or steps, moving the caret would fight the user for control of the file they are
/// reading. Each pause is decided once, so a repeated state report cannot move the caret again.
#[test]
fn following_a_pause_is_decided_once_per_pause() {
    use editor_core::DebugSessionState;
    let mut controls = controls();
    let workspace = "C:/work".to_owned();
    controls
        .upsert(config("run-1", "第一个"), &workspace)
        .unwrap();

    // Nothing has been followed yet, so the first question about a pause is answered yes: a stop with
    // no session behind it cannot happen, and arming on construction means no pause is ever missed
    // because a note arrived in an unexpected order.
    assert!(controls.take_debug_position_to_follow());
    // Answering one pause consumes the decision: a later report about the same pause is not a new one.
    assert!(
        !controls.take_debug_position_to_follow(),
        "each pause is followed once"
    );

    // A breakpoint hit while the target ran on its own is followed.
    controls.note_debug_state(
        "run-1",
        DebugSessionState::Paused {
            source: "src/main.rs".into(),
            line: 7,
            reason: Some("断点".into()),
        },
    );
    controls.note_debug_session_begun();
    assert!(controls.take_debug_position_to_follow());

    // Stepping is the user driving: the pause it produces is not chased.
    controls.note_debug_state(
        "run-1",
        DebugSessionState::Paused {
            source: "src/main.rs".into(),
            line: 8,
            reason: None,
        },
    );
    controls.note_debug_moved_by_user();
    assert!(
        !controls.take_debug_position_to_follow(),
        "a step does not move the caret the user just placed"
    );
    // Resuming is the same: the user is driving, so nothing moves until a stop arrives that they did
    // not ask for.
    controls.note_debug_state("run-1", DebugSessionState::Running);
    controls.note_debug_moved_by_user();
    assert!(!controls.take_debug_position_to_follow());
    // A pause reported without the user driving to it is followed again, which is what an independent
    // breakpoint hit looks like from here.
    controls.note_debug_session_begun();
    assert!(controls.take_debug_position_to_follow());
}

/// A preparation whose step has no session yet is still visible as work in flight.
///
/// Written while looking for a gap in `has_work_in_flight` and kept because it found the opposite: the
/// pending-launch list already covers a preparation from the moment its sequence begins, which is why
/// the extra "a sequence is active" condition I first added here turned out to be redundant and was
/// removed. The check states the guarantee and its reason rather than the change I expected to need —
/// a step that has not reached a provider has no session, so only the pending launch can account for it.
#[test]
fn a_preparation_without_a_session_is_still_work_in_flight() {
    let mut controls = controls();
    let workspace = "C:/work".to_owned();
    controls
        .upsert(config("run-1", "第一个"), &workspace)
        .unwrap();
    // Nothing has happened, so there is nothing to ask about.
    assert!(!controls.has_work_in_flight());

    // A composed plan for the configuration, begun without any session published yet.
    let plan = controls
        .prepare_launch("run-1", &workspace, 16)
        .expect("the plan is usable");
    let request_id = controls.begin("run-1");
    controls.begin_sequence("run-1", plan, request_id);
    let sequence = controls.preparation("run-1").expect("the sequence began");
    assert!(
        sequence.current_session().is_none(),
        "the premise: a preparation whose step has not reached a provider has no session"
    );
    assert!(
        controls.active_sessions().is_empty(),
        "and therefore contributes no active session, so only the pending launch accounts for it"
    );
    assert!(
        controls.has_work_in_flight(),
        "the preparation is still work in flight"
    );

    // Asking it to stop is what the leave confirmation does, and it blocks the launch for good.
    let stopped = controls.stop_preparations();
    assert_eq!(
        stopped.len(),
        1,
        "the preparing configuration was asked to stop"
    );
    assert!(
        controls
            .preparation("run-1")
            .expect("still recorded")
            .blocked_by()
            .is_some(),
        "and the launch is blocked rather than left to start its program"
    );
}

/// A breakpoint answer replaces what is known, and a session begins knowing nothing.
///
/// Three states have to stay distinct or the panel lies: never asked about, asked and refused, and
/// asked and bound. The first is `None`, which is why the accessor is not a boolean.
#[test]
fn breakpoint_answers_replace_what_is_known_about_a_session() {
    let mut controls = controls();
    let workspace = "C:/work".to_owned();
    controls
        .upsert(config("run-1", "第一个"), &workspace)
        .unwrap();

    // Nothing has been asked, so nothing is claimed either way.
    assert_eq!(controls.debug_breakpoint_verified("src/main.rs", 4), None);

    controls.note_debug_breakpoints([
        ("src/main.rs".to_owned(), 4, true),
        ("src/main.rs".to_owned(), 9, false),
        ("src/lib.rs".to_owned(), 12, true),
    ]);
    assert_eq!(
        controls.debug_breakpoint_verified("src/main.rs", 4),
        Some(true)
    );
    assert_eq!(
        controls.debug_breakpoint_verified("src/main.rs", 9),
        Some(false),
        "a position the provider refused is reported as refused, not as waiting"
    );
    assert_eq!(
        controls.debug_breakpoint_verified("src/lib.rs", 12),
        Some(true)
    );
    assert_eq!(
        controls.debug_breakpoint_verified("src/other.rs", 1),
        None,
        "a position no answer mentioned is not described"
    );

    // A later answer describes the set it was asked about: a breakpoint removed in between is simply
    // absent, which is what makes an answer about a removed position meaningful.
    controls.note_debug_breakpoints([("src/main.rs".to_owned(), 4, true)]);
    assert_eq!(controls.debug_breakpoint_verified("src/main.rs", 9), None);
    assert_eq!(controls.debug_breakpoint_verified("src/lib.rs", 12), None);

    // Beginning a session forgets the previous one's answer: it described a different target.
    controls.note_debug_session_begun();
    assert_eq!(controls.debug_breakpoint_verified("src/main.rs", 4), None);
}

/// Setting breakpoints is a session request, so it is accepted before any pause exists.
#[test]
fn breakpoints_are_requested_for_the_session_not_a_pause() {
    use editor_core::DebugSessionState;
    let mut controls = controls();
    let workspace = "C:/work".to_owned();
    controls
        .upsert(config("run-1", "第一个"), &workspace)
        .unwrap();
    controls.select("run-1", &workspace);

    // No pause has begun, which is exactly when a user sets breakpoints before starting.
    assert_eq!(controls.debug_pause_scope(), None);
    let request = controls
        .begin_debug_request(DebugMethod::Breakpoints, None)
        .expect("a session request is accepted without a pause");
    assert!(!controls.debug_request_is_start(request));
    // A view request in the same state is still refused, so this is not a general loosening.
    assert_eq!(
        controls.begin_debug_request(DebugMethod::Frames, None),
        None
    );
    // The answer is applied as breakpoints rather than as a session start.
    assert!(controls.apply_debug_answer(request, None, None).is_err());
    let _ = DebugSessionState::Running;
}

/// The panel lists the selected configuration's positions, from the configuration's own set.
///
/// This is the same list the editable field renders, read separately for the part that is evidence:
/// what the provider bound cannot travel in the field, because the field is parsed back on every
/// keystroke and an annotation would have to survive the parser. The order is the model's, not one this
/// view imposes — a first version of this check asserted insertion order and was wrong about that.
#[test]
fn the_panel_lists_the_selected_configurations_breakpoints() {
    let workspace = "C:/work".to_owned();
    // With no configuration at all there is nothing to list, which is not an error.
    let mut without = controls();
    assert!(without.debug_breakpoint_positions().is_empty());
    without.remove("run-1", &workspace).unwrap();
    assert!(without.debug_breakpoint_positions().is_empty());

    let mut controls = controls();
    let mut stored = config("run-1", "第一个");
    stored.breakpoints.insert("src/main.rs", 4).unwrap();
    stored.breakpoints.insert("src/lib.rs", 12).unwrap();
    controls.upsert(stored, &workspace).unwrap();
    // The panel lists the selected configuration's positions, so the selection is stated rather than
    // assumed.
    controls.select("run-1", &workspace);

    // The positions come from the configuration's own set, which keeps them ordered by source; the
    // panel and the field therefore agree without either imposing an order of its own.
    assert_eq!(
        controls.debug_breakpoint_positions(),
        vec![("src/lib.rs".to_owned(), 12), ("src/main.rs".to_owned(), 4)]
    );
    // Nothing has been asked of a provider yet, so every position waits.
    assert_eq!(controls.debug_breakpoint_verified("src/main.rs", 4), None);
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
                label: "发现运行目标…".into(),
                enabled: true
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
                label: "发现运行目标…".into(),
                enabled: true
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
        source: editor_core::RunConfigSource::Local,
        from_target: None,
        provider: None,
        breakpoints: String::new(),
        share: false,
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
        source: editor_core::RunConfigSource::Local,
        from_target: None,
        provider: None,
        breakpoints: String::new(),
        share: false,
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
        source: editor_core::RunConfigSource::Local,
        from_target: None,
        provider: None,
        breakpoints: String::new(),
        share: false,
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

/// Actions can be added, removed and reordered without leaving the edited form.
#[test]
fn prepared_actions_can_be_reordered_in_place() {
    let text = "一 = a.exe\n二 = b.exe\n三 = c.exe";
    // Moving swaps neighbours, so the order the user sees is the order that will run.
    assert_eq!(
        move_step(text, 1, true).as_deref(),
        Some("二 = b.exe\n一 = a.exe\n三 = c.exe")
    );
    assert_eq!(
        move_step(text, 1, false).as_deref(),
        Some("一 = a.exe\n三 = c.exe\n二 = b.exe")
    );
    // A move past either end is refused rather than clamped, so its control can stay disabled.
    assert_eq!(move_step(text, 0, true), None);
    assert_eq!(move_step(text, 2, false), None);
    assert_eq!(move_step(text, 9, true), None);
    // Removing takes the action the user addressed.
    assert_eq!(
        remove_step(text, 1).as_deref(),
        Some("一 = a.exe\n三 = c.exe")
    );
    assert_eq!(remove_step(text, 9), None);
    // Adding appends a template row that is not yet an action, so it can be edited in place.
    let added = add_step(text);
    assert!(added.starts_with(text));
    assert_eq!(added.lines().last(), Some("# 名称 = 程序 | 参数"));
    let parsed = parse_steps(&added).expect("a template row is not an action yet");
    assert_eq!(parsed.len(), 3, "the template is not an action");
    assert_eq!(
        step_lines("一 = a.exe\n\n二 = b.exe\n"),
        vec!["一 = a.exe".to_owned(), "二 = b.exe".to_owned()]
    );
    // Blank rows never shift which action a row control addresses.
    assert_eq!(
        move_step("一 = a.exe\n\n二 = b.exe", 1, true).as_deref(),
        Some("二 = b.exe\n一 = a.exe")
    );
    // The edited text is still read by the same parser, so reordering cannot change an action's
    // meaning.
    let reordered = move_step(text, 2, true).unwrap();
    let steps = parse_steps(&reordered).expect("the reordered list is well formed");
    assert_eq!(
        steps
            .iter()
            .map(|step| step.name.as_str())
            .collect::<Vec<_>>(),
        vec!["一", "三", "二"]
    );
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
        source: editor_core::RunConfigSource::Local,
        from_target: None,
        provider: None,
        breakpoints: String::new(),
        share: false,
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
        source: editor_core::RunConfigSource::Local,
        from_target: None,
        provider: None,
        breakpoints: String::new(),
        share: false,
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
        source: editor_core::RunConfigSource::Local,
        from_target: None,
        provider: None,
        breakpoints: String::new(),
        share: false,
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
