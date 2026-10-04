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

/// A draft keeps one argument per line and never re-splits a value containing spaces.
#[test]
fn drafts_preserve_argument_boundaries() {
    let draft = RunConfigDraft {
        id: "run-1".into(),
        name: " 带空格 ".into(),
        program: " C:/Program Files/tool.exe ".into(),
        arguments: "--flag\nC:/path with spaces/file.txt\n".into(),
        directory: " C:/work ".into(),
    };
    let configuration = draft.to_config();
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
    configuration
        .validate()
        .expect("draft produces a valid configuration");

    // An empty directory means the workspace root rather than an empty path.
    let bare = RunConfigDraft::from_config(None, "run-2".into());
    assert_eq!(bare.to_config().directory, None);
    assert_eq!(
        RunConfigDraft::from_config(Some(&configuration), "run-1".into()).arguments,
        "--flag\nC:/path with spaces/file.txt"
    );
}
