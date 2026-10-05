//! Real execution lifecycles entered through an installed provider and the public host session API.
#![cfg(windows)]
use plugin_runtime::{ExecutionState, Manager, Package, RunRequest, plugin_protocol::Environment};
use std::{
    path::Path,
    time::{Duration, Instant},
};

/// This probe handles the console interrupt and writes cleanup evidence before returning normally.
const STOP_PROBE: &str = r#"
//! A real child program that makes graceful shutdown observable to the integration test.
use std::sync::atomic::{AtomicBool, Ordering};
static STOP: AtomicBool = AtomicBool::new(false);
static IGNORE: AtomicBool = AtomicBool::new(false);
#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetConsoleCtrlHandler(handler: Option<unsafe extern "system" fn(u32) -> i32>, add: i32) -> i32;
}
/// Report only CTRL+C as a supported interrupt; all other native events retain their default behavior.
unsafe extern "system" fn interrupt(kind: u32) -> i32 {
    if kind != 0 { return 0; }
    if !IGNORE.load(Ordering::Acquire) { STOP.store(true, Ordering::Release); }
    1
}
fn main() {
    let directory = std::env::args().nth(1).expect("evidence directory");
    let mode = std::env::args().nth(2).unwrap_or_default();
    let ignore = matches!(mode.as_str(), "ignore" | "tree" | "descendant");
    IGNORE.store(ignore, Ordering::Release);
    assert_ne!(unsafe { SetConsoleCtrlHandler(Some(interrupt), 1) }, 0);
    if mode == "tree" {
        // The descendant inherits the owned job and keeps running until the entire tree is stopped.
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([&directory, "descendant"]).spawn().unwrap();
        std::fs::write(format!("{directory}/child-pid"), child.id().to_string()).unwrap();
    }
    let marker = if mode == "descendant" { "child-ready" } else { "ready" };
    std::fs::write(format!("{directory}/{marker}"), std::process::id().to_string()).unwrap();
    while !STOP.load(Ordering::Acquire) {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    if mode == "delay" { std::thread::sleep(std::time::Duration::from_millis(800)); }
    std::fs::write(format!("{directory}/cleanup"), b"clean exit").unwrap();
}
"#;

/// Compile a self-contained probe using the existing compiler, failing explicitly if unavailable.
fn probe(directory: &Path) -> std::path::PathBuf {
    let source = directory.join("stop_probe.rs");
    let binary = directory.join("stop_probe.exe");
    std::fs::write(&source, STOP_PROBE).unwrap();
    let output = std::process::Command::new("rustc")
        .args(["--edition", "2024", "-o"])
        .arg(&binary)
        .arg(source)
        .output()
        .expect("existing Rust compiler required");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    binary
}

/// Drive the ordinary manager queue until a real externally observed predicate settles.
fn wait(manager: &mut Manager, done: impl Fn(&Manager) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !done(manager) && Instant::now() < deadline {
        manager.poll();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        done(manager),
        "observable lifecycle did not settle before the deadline"
    );
}

/// Stop must permit the program's cleanup instead of immediately killing its owned process tree.
#[test]
#[ignore = "build the terminal package with the current public SDK first"]
fn a_normal_stop_allows_the_program_to_finish_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let program = probe(root.path());
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let package = Package::read(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/terminal.zip"),
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let session = manager
        .start_execution(RunRequest {
            program: program.display().to_string(),
            args: vec![root.path().display().to_string()],
            cwd: Some(root.path().display().to_string()),
            name: Some("normal stop".into()),
            env: vec![],
        })
        .unwrap();
    wait(&mut manager, |_| root.path().join("ready").exists());
    manager.stop_execution(session.id()).unwrap();
    wait(&mut manager, |manager| {
        matches!(
            manager.execution(session.id()).unwrap().snapshot().state,
            ExecutionState::Exited | ExecutionState::Failed
        )
    });
    assert_eq!(
        std::fs::read(root.path().join("cleanup")).unwrap(),
        b"clean exit"
    );
    assert_eq!(session.snapshot().state, ExecutionState::Exited);
    let status = manager.query_execution(session.id()).unwrap();
    manager.poll_request(&status);
    let plugin_runtime::plugin_protocol::api::RequestUpdate::Completed { result: Ok(value) } =
        status.status()
    else {
        panic!("the provider must report the observed final exit code");
    };
    assert_eq!(value["code"], 0);
}

/// Ignoring a supported interrupt must eventually force termination instead of leaking the program.
#[test]
#[ignore = "build the terminal package with the current public SDK first"]
fn an_ignored_normal_stop_is_upgraded_to_forceful_termination() {
    let root = tempfile::tempdir().unwrap();
    let program = probe(root.path());
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let package = Package::read(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/terminal.zip"),
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let session = manager
        .start_execution(RunRequest {
            program: program.display().to_string(),
            args: vec![root.path().display().to_string(), "ignore".into()],
            cwd: Some(root.path().display().to_string()),
            name: None,
            env: vec![],
        })
        .unwrap();
    // The provider's own interactive shell is unrelated to this managed execution and must survive.
    let baseline = manager.live["terminal"].process_count();
    wait(&mut manager, |_| root.path().join("ready").exists());
    manager.stop_execution(session.id()).unwrap();
    wait(&mut manager, |manager| {
        manager.live["terminal"].process_count() == baseline
            && session.snapshot().state == ExecutionState::Exited
    });
    assert!(
        !root.path().join("cleanup").exists(),
        "the program ignored graceful exit"
    );
    assert_eq!(session.snapshot().state, ExecutionState::Exited);
    let status = manager.query_execution(session.id()).unwrap();
    manager.poll_request(&status);
    let plugin_runtime::plugin_protocol::api::RequestUpdate::Completed { result: Ok(value) } =
        status.status()
    else {
        panic!("the final forced outcome must remain observable");
    };
    assert_eq!(value["state"], "terminated");
}

/// Window shutdown must allow bounded cleanup before revoking the provider's native resources.
#[test]
#[ignore = "build the terminal package with the current public SDK first"]
fn leaving_waits_for_normal_cleanup_before_retiring_the_provider() {
    let root = tempfile::tempdir().unwrap();
    let program = probe(root.path());
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let package = Package::read(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/terminal.zip"),
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    manager
        .start_execution(RunRequest {
            program: program.display().to_string(),
            args: vec![root.path().display().to_string(), "delay".into()],
            cwd: Some(root.path().display().to_string()),
            name: None,
            env: vec![],
        })
        .unwrap();
    wait(&mut manager, |_| root.path().join("ready").exists());
    manager.shutdown();
    assert_eq!(
        std::fs::read(root.path().join("cleanup")).unwrap(),
        b"clean exit"
    );
    assert!(manager.live.is_empty());
}

/// Read a native PID without relying on the provider's logical process counter.
fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code = 0;
        let queried = GetExitCodeProcess(handle, &mut code) != 0;
        CloseHandle(handle);
        queried && code == 259 // STILL_ACTIVE is a native state, not a provider acknowledgement.
    }
}

/// Force bypasses the configured grace period and releases both the target and its descendant.
#[test]
#[ignore = "build the terminal package with the current public SDK first"]
fn immediate_termination_releases_the_actual_owned_process_tree() {
    let root = tempfile::tempdir().unwrap();
    let program = probe(root.path());
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let package = Package::read(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/terminal.zip"),
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let baseline = manager.live["terminal"].process_count();
    let session = manager
        .start_execution(RunRequest {
            program: program.display().to_string(),
            args: vec![root.path().display().to_string(), "tree".into()],
            cwd: Some(root.path().display().to_string()),
            name: None,
            env: vec![],
        })
        .unwrap();
    wait(&mut manager, |_| root.path().join("child-ready").exists());
    let parent = std::fs::read_to_string(root.path().join("ready"))
        .unwrap()
        .parse()
        .unwrap();
    let child = std::fs::read_to_string(root.path().join("child-pid"))
        .unwrap()
        .parse()
        .unwrap();
    assert!(process_alive(parent) && process_alive(child));
    let start = Instant::now();
    manager
        .stop_execution_with(
            session.id(),
            plugin_runtime::StopOptions {
                mode: plugin_runtime::plugin_protocol::process::ExitMode::Force,
                grace_ms: 60_000,
            },
        )
        .unwrap();
    wait(&mut manager, |manager| {
        !process_alive(parent)
            && !process_alive(child)
            && manager.live["terminal"].process_count() == baseline
            && session.state() == ExecutionState::Exited
    });
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "force must skip the 60-second grace period"
    );
    assert!(!root.path().join("cleanup").exists());
}

/// Stopping before creation revokes this launch only and prevents a delayed native start from escaping.
#[test]
#[ignore = "build the terminal package with the current public SDK first"]
fn a_starting_execution_can_be_stopped_without_leaving_a_late_program() {
    let root = tempfile::tempdir().unwrap();
    let program = probe(root.path());
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let package = Package::read(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/terminal.zip"),
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let baseline = manager.live["terminal"].process_count();
    let session = manager
        .start_execution(RunRequest {
            program: program.display().to_string(),
            args: vec![root.path().display().to_string()],
            cwd: Some(root.path().display().to_string()),
            name: None,
            env: vec![],
        })
        .unwrap();
    assert_eq!(session.state(), ExecutionState::Starting);
    manager
        .stop_execution_with(
            session.id(),
            plugin_runtime::StopOptions {
                mode: plugin_runtime::plugin_protocol::process::ExitMode::Force,
                ..Default::default()
            },
        )
        .unwrap();
    for _ in 0..100 {
        manager.poll();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(session.state(), ExecutionState::Failed);
    assert_eq!(manager.live["terminal"].process_count(), baseline);
    assert!(!root.path().join("ready").exists());
}

/// Changing projects completes old programs' cleanup before parking their plugin instances.
#[test]
#[ignore = "build the terminal package with the current public SDK first"]
fn switching_workspaces_cleans_the_old_program_before_leaving_its_scope() {
    let root = tempfile::tempdir().unwrap();
    let next = tempfile::tempdir().unwrap();
    let program = probe(root.path());
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let package = Package::read(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/terminal.zip"),
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let session = manager
        .start_execution(RunRequest {
            program: program.display().to_string(),
            args: vec![root.path().display().to_string(), "delay".into()],
            cwd: Some(root.path().display().to_string()),
            name: None,
            env: vec![],
        })
        .unwrap();
    wait(&mut manager, |_| root.path().join("ready").exists());
    manager
        .switch_workspace(
            Environment {
                workspace: next.path().display().to_string(),
                os: "windows".into(),
                ..Default::default()
            },
            true,
        )
        .unwrap();
    assert_eq!(
        std::fs::read(root.path().join("cleanup")).unwrap(),
        b"clean exit"
    );
    assert!(
        manager.execution(session.id()).is_none(),
        "the old scope is not visible in the new project"
    );
    manager.shutdown();
}
