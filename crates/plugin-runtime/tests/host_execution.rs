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
use plugin_runtime::{ExecutionState, Manager, Package, RunRequest};
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
        })
        .unwrap_err()
        .to_string();
    assert!(missing.contains(CONTRACT), "{missing}");
}
