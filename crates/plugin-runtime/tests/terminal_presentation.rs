//! Independent SDK consumers exercise native presentation, bounded stop and original source scope.
#![cfg(windows)]
#[path = "support/debugger_packages.rs"]
mod packages;
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{self as protocol, api, process},
};
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{Duration, Instant},
};

/// Repackaging changes only public manifest identity and capability declarations, never native code paths.
fn consumer(id: &str) -> Package {
    let base = Package::read(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
    )
    .unwrap();
    let mut manifest: Value = serde_json::from_slice(&base.files["manifest.json"]).unwrap();
    manifest["id"] = json!(id);
    manifest["api"]["required"]["process"] = json!(">=1.7,<2");
    manifest["permissions"]
        .as_array_mut()
        .unwrap()
        .push(json!("process.exec"));
    // A global consumer's processes must still belong to the workspace where they were created.
    if id == "presentation-b" {
        manifest["scope"] = json!("application");
    }
    packages::package(base.files, manifest)
}
/// A native child deliberately ignores CTRL+C; only real tree termination can complete the stop.
fn stubborn_program(directory: &Path) -> std::path::PathBuf {
    let source = directory.join("stubborn.rs");
    let binary = directory.join("stubborn.exe");
    std::fs::write(&source,r#"
//! A real PTY child that acknowledges interrupts without exiting.
#[link(name="kernel32")] unsafe extern "system" {fn SetConsoleCtrlHandler(f:Option<unsafe extern "system" fn(u32)->i32>,add:i32)->i32;}
/// Claim CTRL+C while leaving other operating system events to their normal handler.
unsafe extern "system" fn interrupt(kind:u32)->i32 {i32::from(kind==0)}
fn main(){assert_ne!(unsafe{SetConsoleCtrlHandler(Some(interrupt),1)},0); std::fs::write(std::env::args().nth(1).unwrap(),std::process::id().to_string()).unwrap(); loop{std::thread::sleep(std::time::Duration::from_millis(10));}}
"#).unwrap();
    let output = std::process::Command::new("rustc")
        .args(["--edition", "2024", "-o"])
        .arg(&binary)
        .arg(source)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    binary
}
/// Typed failures are returned by the public guest request, not manufactured by the test.
fn request(
    manager: &mut Manager,
    id: &str,
    operation: process::Operation,
) -> Result<api::Value, api::Failure> {
    manager
        .invoke_command(
            id,
            "process-request",
            serde_json::to_value(operation).unwrap(),
        )
        .unwrap();
    let protocol::ui::Kind::Text { text } = &manager.live[id].views["welcome"].root.kind else {
        panic!("guest result expected")
    };
    serde_json::from_str(text).unwrap()
}
fn launch(manager: &mut Manager, id: &str, pty: bool) -> api::ResourceHandle {
    let api::Value::Resource(handle)=request(manager,id,process::Operation::Execute{
        program:"powershell.exe".into(),args:vec!["-NoProfile".into(),"-Command".into(),"Write-Output 'READY'; $x=[Console]::ReadLine(); Write-Output ('ANSWER:'+$x); Start-Sleep -Seconds 60".into()],
        cwd:None,env:Default::default(),transport:if pty {process::Transport::Pty{columns:80,rows:20,inherit_cursor:false}} else {process::Transport::Stdio},
    }).unwrap() else {panic!("owned process handle expected")};
    handle
}
/// Foreign/stale and pipe handles fail; retiring one consumer keeps the peer's input and output alive.
#[test]
#[ignore = "build capability-example through the current embedded SDK first"]
fn public_terminal_presentations_validate_ownership_transport_and_retirement() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        root.path().join("plugins"),
        protocol::Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    for id in ["presentation-a", "presentation-b"] {
        let package = consumer(id);
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
    }
    let a = launch(&mut manager, "presentation-a", true);
    let b = launch(&mut manager, "presentation-b", true);
    for (id, handle) in [("presentation-a", &a), ("presentation-b", &b)] {
        request(
            &mut manager,
            id,
            process::Operation::PresentTerminal {
                handle: handle.clone(),
                title: id.into(),
            },
        )
        .unwrap();
    }
    assert_eq!(
        request(
            &mut manager,
            "presentation-b",
            process::Operation::PresentTerminal {
                handle: a.clone(),
                title: "foreign".into()
            }
        )
        .unwrap_err()
        .code,
        api::ErrorCode::InvalidHandle
    );
    let pipe = launch(&mut manager, "presentation-b", false);
    assert_eq!(
        request(
            &mut manager,
            "presentation-b",
            process::Operation::PresentTerminal {
                handle: pipe.clone(),
                title: "pipe".into()
            }
        )
        .unwrap_err()
        .code,
        api::ErrorCode::InvalidRequest
    );
    manager.terminal_input(&b, b"peer\r").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut peer = Vec::new();
    loop {
        manager.poll();
        for view in manager.take_terminal_presentations() {
            if view.handle == b {
                for update in view.updates {
                    if let process::Update::Output { bytes, .. } = update {
                        peer.extend(bytes);
                    }
                }
            }
        }
        if String::from_utf8_lossy(&peer).contains("ANSWER:peer") {
            break;
        }
        assert!(Instant::now() < deadline, "peer input missing");
        std::thread::sleep(Duration::from_millis(10));
    }
    manager.disable("presentation-a").unwrap();
    assert!(manager.terminal_input(&a, b"retired\r").is_err());
    manager.terminal_resize(&b, 100, 25).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut ended = false;
    while !ended {
        manager.poll();
        ended = manager.take_terminal_presentations().iter().any(|view| {
            view.handle == a
                && view.updates.iter().any(|update| {
                    matches!(
                        update,
                        process::Update::Exited { .. } | process::Update::Terminated
                    )
                })
        });
        assert!(
            Instant::now() < deadline,
            "retired presentation has no exit receipt"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(manager.live["presentation-b"].process_count(), 2);
    // A normal native close escalates after its grace period, retaining EOF rather than a fake exit.
    let program = stubborn_program(root.path());
    let ready = root.path().join("stubborn-ready");
    let api::Value::Resource(stubborn) = request(
        &mut manager,
        "presentation-b",
        process::Operation::Execute {
            program: program.display().to_string(),
            args: vec![ready.display().to_string()],
            cwd: None,
            env: Default::default(),
            transport: process::Transport::Pty {
                columns: 80,
                rows: 20,
                inherit_cursor: false,
            },
        },
    )
    .unwrap() else {
        panic!("resource")
    };
    request(
        &mut manager,
        "presentation-b",
        process::Operation::PresentTerminal {
            handle: stubborn.clone(),
            title: "Stubborn program".into(),
        },
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready.exists() {
        manager.poll();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let stopped = Instant::now();
    manager
        .terminal_exit(&stubborn, process::ExitMode::Graceful)
        .unwrap();
    loop {
        manager.poll();
        if manager.take_terminal_presentations().iter().any(|view| {
            view.handle == stubborn
                && view.updates.iter().any(|update| {
                    matches!(
                        update,
                        process::Update::Exited { .. } | process::Update::Terminated
                    )
                })
        }) {
            break;
        }
        assert!(
            stopped.elapsed() < Duration::from_secs(10),
            "normal close did not converge"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        stopped.elapsed() >= Duration::from_millis(2800),
        "ignored interrupt must await the grace barrier"
    );
    manager.terminal_resize(&b, 90, 24).unwrap();
    // Read-only PTY projections cannot acquire input authority merely from their transport type.
    let readonly = launch(&mut manager, "presentation-b", true);
    request(
        &mut manager,
        "presentation-b",
        process::Operation::TerminalOutput {
            handle: readonly.clone(),
            title: "Read-only PTY".into(),
            bytes: b"decoded".to_vec(),
        },
    )
    .unwrap();
    assert_eq!(
        request(
            &mut manager,
            "presentation-b",
            process::Operation::PresentTerminal {
                handle: readonly.clone(),
                title: "Cannot change mode".into()
            }
        )
        .unwrap_err()
        .code,
        api::ErrorCode::InvalidState
    );
    assert!(manager.terminal_input(&readonly, b"forbidden").is_err());
    assert!(manager.terminal_resize(&readonly, 90, 25).is_err());
    assert_eq!(
        request(
            &mut manager,
            "presentation-b",
            process::Operation::TerminalOutput {
                handle: b.clone(),
                title: "Interactive mode conflict".into(),
                bytes: b"text".to_vec()
            }
        )
        .unwrap_err()
        .code,
        api::ErrorCode::InvalidState
    );
    // Pipe stdout is protocol data. Only the consumer's explicitly decoded text becomes visible.
    request(
        &mut manager,
        "presentation-b",
        process::Operation::TerminalOutput {
            handle: pipe.clone(),
            title: "Decoded protocol".into(),
            bytes: b"diagnostic only".to_vec(),
        },
    )
    .unwrap();
    manager.poll();
    let diagnostic = manager
        .take_terminal_presentations()
        .into_iter()
        .find(|view| view.handle == pipe)
        .unwrap();
    assert!(!diagnostic.interactive);
    let text: Vec<_> = diagnostic
        .updates
        .into_iter()
        .filter_map(|update| {
            if let process::Update::Output { bytes, .. } = update {
                Some(bytes)
            } else {
                None
            }
        })
        .flatten()
        .collect();
    assert_eq!(text, b"diagnostic only");
    assert!(manager.terminal_input(&pipe, b"forbidden").is_err());
    assert!(manager.terminal_resize(&pipe, 80, 20).is_err());
    assert_eq!(
        request(
            &mut manager,
            "presentation-b",
            process::Operation::TerminalOutput {
                handle: pipe.clone(),
                title: "Quota".into(),
                bytes: vec![0; 16385],
            }
        )
        .unwrap_err()
        .code,
        api::ErrorCode::LimitExceeded
    );
    // Leaving a workspace closes its displayed resources; a parked owner's tail never enters B.
    let next = root.path().join("workspace-b");
    std::fs::create_dir(&next).unwrap();
    manager
        .switch_workspace(
            protocol::Environment {
                workspace: next.display().to_string(),
                os: "windows".into(),
                ..Default::default()
            },
            true,
        )
        .unwrap();
    for _ in 0..8 {
        manager.poll();
    }
    assert!(manager.take_terminal_presentations().is_empty());
    assert!(manager.terminal_input(&b, b"old workspace").is_err());
    assert!(
        manager.live.contains_key("presentation-b"),
        "application owner remains live"
    );
    manager
        .switch_workspace(
            protocol::Environment {
                workspace: root.path().display().to_string(),
                os: "windows".into(),
                ..Default::default()
            },
            true,
        )
        .unwrap();
    assert!(manager.terminal_input(&b, b"retired after switch").is_err());
    assert!(manager.take_terminal_presentations().is_empty());
    // Revoking trust must stop even a directly created application PTY without calling the guest.
    let global_ready = root.path().join("global-ready");
    let api::Value::Resource(global) = request(
        &mut manager,
        "presentation-b",
        process::Operation::Execute {
            program: program.display().to_string(),
            args: vec![global_ready.display().to_string()],
            cwd: None,
            env: Default::default(),
            transport: process::Transport::Pty {
                columns: 80,
                rows: 20,
                inherit_cursor: false,
            },
        },
    )
    .unwrap() else {
        panic!("global process")
    };
    request(
        &mut manager,
        "presentation-b",
        process::Operation::PresentTerminal {
            handle: global.clone(),
            title: "Global source".into(),
        },
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !global_ready.exists() {
        manager.poll();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let pid = std::fs::read_to_string(global_ready).unwrap();
    manager
        .terminal_exit(&global, process::ExitMode::Graceful)
        .unwrap();
    manager.set_workspace_trust(false).unwrap();
    assert!(manager.live.contains_key("presentation-b"));
    assert!(manager.terminal_input(&global, b"revoked").is_err());
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        manager.poll();
        let status = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-Command",
                &format!("if(Get-Process -Id {pid} -ErrorAction SilentlyContinue) {{exit 1}}"),
            ])
            .status()
            .unwrap();
        if status.success() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "revoked application process remains alive"
        );
    }
    assert!(manager.take_terminal_presentations().is_empty());
    manager.shutdown();
}
