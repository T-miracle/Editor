//! The editor's run controls are an ordinary consumer of the published execution contract.
//!
//! These checks start a real program through the host session API — by contract and scope, never by
//! plugin identity — and assert only externally observable results: session identity, the provider's
//! own answer, the requested native panel, and real process ownership.
#![cfg(windows)]
// The shared fixture module serves several integration tests; this one uses only its contract name.
#[path = "support/interactive_packages.rs"]
#[allow(dead_code)]
mod packages;
use packages::CONTRACT;
use plugin_runtime::{
    ExecutionState, Manager, Package, RunRequest, plugin_protocol::api::RequestUpdate,
};
use serde_json::Value;
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn terminal() -> Package {
    Package::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/terminal.zip"))
        .unwrap()
}

fn manager(root: &Path) -> Manager {
    Manager::open(
        root.join("plugins"),
        plugin_runtime::plugin_protocol::Environment {
            workspace: root.display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap()
}

/// Polling drives real service and process completion, bounded independently of guest fuel.
///
/// The final observed value is returned so a caller can assert the final outcome directly.
fn wait_until<T>(
    manager: &mut Manager,
    ready: impl Fn(&Manager) -> T,
    done: impl Fn(&T) -> bool,
) -> T {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut observed = ready(manager);
    while !done(&observed) && Instant::now() < deadline {
        manager.poll();
        std::thread::sleep(Duration::from_millis(20));
        observed = ready(manager);
    }
    observed
}

/// Answer the provider's ordinary native panel request so its session becomes visible.
fn reveal_panel(manager: &mut Manager) -> usize {
    let Some(instance) = manager.live.get_mut("terminal") else {
        return 0;
    };
    let requests = instance.take_editor_requests();
    let count = requests.len();
    for request in requests {
        let operation = request.operation().clone();
        assert!(request.begin());
        let _ = request.finish(Ok(
            plugin_runtime::plugin_protocol::api::EditorValue::PanelVisibility {
                panel: "terminal".into(),
                visible: matches!(
                    operation,
                    plugin_runtime::plugin_protocol::api::EditorOperation::SetPanelVisibility {
                        visible: true,
                        ..
                    }
                ),
            },
        ));
    }
    count
}

/// A repeat launch must locate the retained session instead of creating a second program.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn host_controls_start_once_and_locate_the_retained_session() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = manager(root.path());
    let package = terminal();
    let grants = package.manifest.permissions.clone();
    manager.install(&package, grants).unwrap();
    let request = RunRequest {
        program: "powershell.exe".into(),
        args: vec![
            "-NoProfile".into(),
            "-Command".into(),
            "[Console]::Write('HOST_START_OK'); Start-Sleep -Seconds 60".into(),
        ],
        cwd: Some(root.path().display().to_string()),
        name: Some("宿主运行".into()),
        env: Vec::new(),
    };
    let session = manager.start_execution(request.clone()).unwrap();
    assert_eq!(session.plugin(), "terminal");
    // A queued request is not an answer: nothing claims the program is running yet.
    assert_eq!(session.snapshot().state, ExecutionState::Starting);
    let active = wait_until(
        &mut manager,
        |manager| manager.execution(session.id()).unwrap().snapshot().state,
        |state| *state != ExecutionState::Starting,
    );
    assert_eq!(active, ExecutionState::Running);
    let snapshot = manager.execution(session.id()).unwrap().snapshot();
    assert_eq!(snapshot.plugin, "terminal");
    assert!(
        snapshot.provider_session.is_some(),
        "the provider's own session identity is reported without being reinterpreted"
    );
    // The provider's answer is terminal as a request; the session lifecycle continues separately.
    assert!(
        manager
            .execution(session.id())
            .unwrap()
            .update()
            .is_terminal()
    );
    // The provider owns its presentation: it asks the editor for its ordinary panel.
    assert!(reveal_panel(&mut manager) >= 1);
    // A second request for the same literal command resolves to the retained session.
    let located = manager
        .execution_for(&request, Some(&root.path().display().to_string()))
        .expect("repeat launch locates the retained session");
    assert_eq!(located.id(), session.id());
    // The session outlives its provider only as a visible result: the program it started is no
    // longer managed here, so it is reported as failed rather than as still running.
    manager.disable("terminal").unwrap();
    let ended = wait_until(
        &mut manager,
        |manager| manager.execution(session.id()).unwrap().snapshot().state,
        |state| *state == ExecutionState::Failed,
    );
    assert_eq!(ended, ExecutionState::Failed);
    assert!(
        manager
            .execution(session.id())
            .unwrap()
            .snapshot()
            .provider_session
            .is_some()
    );
    // A missing provider is reported instead of being replaced by an unrelated command.
    let missing = manager
        .start_execution(RunRequest {
            program: "powershell.exe".into(),
            args: vec!["-NoProfile".into()],
            cwd: None,
            name: None,
            env: Vec::new(),
        })
        .unwrap_err()
        .to_string();
    assert!(missing.contains(CONTRACT), "{missing}");
}

/// A session's end is observed through its provider, never predicted from elapsed time.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn host_observes_a_program_exit_through_its_provider() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = manager(root.path());
    let package = terminal();
    let grants = package.manifest.permissions.clone();
    manager.install(&package, grants).unwrap();
    // A program that ends on its own, so the observation is of a real exit rather than a stop.
    let request = RunRequest {
        program: "powershell.exe".into(),
        args: vec!["-NoProfile".into(), "-Command".into(), "exit 7".into()],
        cwd: Some(root.path().display().to_string()),
        name: Some("会退出".into()),
        env: Vec::new(),
    };
    let session = manager.start_execution(request).unwrap();
    let running = wait_until(
        &mut manager,
        |manager| manager.execution(session.id()).unwrap().snapshot().state,
        |state| *state == ExecutionState::Running,
    );
    assert_eq!(running, ExecutionState::Running);
    assert!(reveal_panel(&mut manager) >= 1);

    // Polling the provider is what turns "the program is gone" into a fact with a status.
    let mut observed = None;
    let deadline = Instant::now() + Duration::from_secs(30);
    while observed.is_none() && Instant::now() < deadline {
        manager.poll();
        let query = manager.query_execution(session.id()).unwrap();
        manager.poll();
        if let RequestUpdate::Completed { result: Ok(value) } = query.status() {
            let state = value
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if state != "running" {
                observed = Some(value.clone());
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let value = observed.expect("the provider reported the program's end");
    assert_eq!(value.get("state").and_then(Value::as_str), Some("exited"));
    assert_eq!(
        value.get("code").and_then(Value::as_u64),
        Some(7),
        "the exit status is the program's own, not a guess"
    );
    // The observation did not create a second session for the run controls to show.
    assert_eq!(manager.executions().len(), 1);
    // An unknown session is refused instead of answering for another program.
    assert!(manager.query_execution(session.id() + 500).is_err());
}

/// The host stops a running program through the provider's own session control.
///
/// The request names the session the provider returned; the host never borrows the provider's
/// private process handle, and an acknowledgement is not treated as proof that the program exited.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn host_controls_stop_the_program_a_session_owns() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = manager(root.path());
    let package = terminal();
    let grants = package.manifest.permissions.clone();
    manager.install(&package, grants).unwrap();
    let request = RunRequest {
        program: "powershell.exe".into(),
        args: vec![
            "-NoProfile".into(),
            "-Command".into(),
            "Start-Sleep -Seconds 60".into(),
        ],
        cwd: Some(root.path().display().to_string()),
        name: Some("可停止".into()),
        env: Vec::new(),
    };
    let session = manager.start_execution(request.clone()).unwrap();
    let running = wait_until(
        &mut manager,
        |manager| manager.execution(session.id()).unwrap().snapshot().state,
        |state| *state == ExecutionState::Running,
    );
    assert_eq!(running, ExecutionState::Running);
    assert!(manager.execution(session.id()).unwrap().stoppable());
    // A delegated program and the provider's own private shell are both alive here.
    assert_eq!(manager.live["terminal"].process_count(), 2);
    assert!(reveal_panel(&mut manager) >= 1);

    // Stopping is accepted by the exact incarnation that started the program.
    manager.stop_execution(session.id()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while manager.live["terminal"].process_count() > 1 && Instant::now() < deadline {
        manager.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    // The owned program is gone while the provider's private session remains.
    assert_eq!(manager.live["terminal"].process_count(), 1);
    // The session keeps the only lifecycle fact this contract reports: the provider did start a
    // program. Nothing in execution contract 1.1 reports a later exit back to a consumer, so the
    // host does not invent a stopped state from the absence of a process.
    assert_eq!(
        manager.execution(session.id()).unwrap().state(),
        ExecutionState::Running
    );
    // Repeating the stop is answered by the provider, which has nothing left to stop.
    manager.stop_execution(session.id()).unwrap();
    // An unknown session is refused instead of stopping an unrelated program.
    assert!(manager.stop_execution(session.id() + 1000).is_err());
}
