//! Real package boundaries pin service declarations before any native program can run.
use plugin_runtime::Package;
use serde_json::json;
use std::io::{Cursor, Write};

/// Repackage the independently built SDK guest with a fixed test tool; no implementation mocks.
fn executable_package(program: &str, exec: bool) -> Package {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/plugin-api-test/capability-example.zip");
    let original = Package::read(&path).unwrap();
    let mut manifest = serde_json::to_value(&original.manifest).unwrap();
    manifest["api"]["required"]["process"] = json!("^1");
    manifest["services"] = json!({"echo":{"program":program,"args":[]}});
    manifest["permissions"]
        .as_array_mut()
        .unwrap()
        .push(json!("process.service.echo"));
    if exec {
        manifest["permissions"]
            .as_array_mut()
            .unwrap()
            .push(json!("process.exec"));
    }
    let mut files = original.files;
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        archive
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&archive.finish().unwrap().into_inner()).unwrap()
}

/// Observe only the native text published by the real guest, through the manager command seam.
fn probe(
    manager: &mut plugin_runtime::Manager,
    id: &str,
    operation: serde_json::Value,
) -> serde_json::Value {
    manager
        .invoke_command(
            id,
            "scope-probe",
            json!({"method":"process","operation":operation}),
        )
        .unwrap();
    let scene = manager.live[id].scene.as_ref().unwrap();
    let plugin_runtime::plugin_protocol::ui::Kind::Text { text } =
        &scene.ui.as_ref().unwrap().root.kind
    else {
        panic!("text expected")
    };
    serde_json::from_str(text).unwrap()
}

/// A native byte echo program verifies pipes, complete output, and independent stderr transport.
#[test]
#[ignore = "build with scripts/build-capability-example.ps1 first"]
fn approved_service_runs_over_stdio_and_cannot_execute_arbitrary_programs() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("echo-fixture.exe");
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/echo_service.rs");
    assert!(
        std::process::Command::new("rustc")
            .arg(source)
            .arg("-o")
            .arg(&exe)
            .status()
            .unwrap()
            .success()
    );
    let package = executable_package(exe.to_str().unwrap(), false);
    let id = &package.manifest.id;
    let mut manager = plugin_runtime::Manager::open(
        dir.path().join("plugins"),
        plugin_runtime::plugin_protocol::Environment {
            workspace: dir.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let start = probe(
        &mut manager,
        id,
        json!({"kind":"start_service","service":"echo"}),
    );
    assert!(start.get("Ok").is_some(), "service failed: {start}");
    let handle = &start["Ok"]["Resource"];
    let denied = probe(
        &mut manager,
        id,
        json!({"kind":"execute", "program":exe, "args":[], "transport":{"kind":"stdio"}}),
    );
    assert_eq!(denied["Err"]["code"], "permission_denied");
    assert_eq!(
        probe(
            &mut manager,
            id,
            json!({"kind":"write", "handle":handle, "bytes":b"hello space\nexit\n"})
        )["Ok"],
        "Unit"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while manager.live[id].process_count() != 0 && std::time::Instant::now() < deadline {
        manager.live.get_mut(id).unwrap().poll().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(
        manager.live[id].process_count(),
        0,
        "service did not finish"
    );
    let events = process_events(&mut manager, id);
    let mut out = Vec::new();
    let mut err = Vec::new();
    for event in &events {
        if let Some(output) = event.get("Output") {
            let bytes: Vec<u8> = serde_json::from_value(output["bytes"].clone()).unwrap();
            match output["stream"].as_str().unwrap() {
                "stdout" => out.extend(bytes),
                "stderr" => err.extend(bytes),
                other => panic!("unexpected stream {other}"),
            }
        }
    }
    assert!(out.starts_with(b"hello space\n"));
    assert_eq!(out.len(), 12 + 131072);
    assert_eq!(err, b"separate stderr\n");
    assert_eq!(events.last().unwrap()["Exited"]["code"], 0);
}

/// Consume bounded event pages from the guest's native view, never from runtime internals.
fn process_events(manager: &mut plugin_runtime::Manager, id: &str) -> Vec<serde_json::Value> {
    let mut events = Vec::new();
    for index in 0..256 {
        manager
            .invoke_command(id, "process-events", json!(index))
            .unwrap();
        let scene = manager.live[id].scene.as_ref().unwrap();
        let plugin_runtime::plugin_protocol::ui::Kind::Text { text } =
            &scene.ui.as_ref().unwrap().root.kind
        else {
            panic!("text expected")
        };
        let event: serde_json::Value = serde_json::from_str(text).unwrap();
        if event.is_null() {
            break;
        }
        events.push(event);
    }
    events
}

/// Refused starts have no native side effects; callback cancellation suppresses already queued output.
#[test]
#[ignore = "build with scripts/build-capability-example.ps1 first"]
fn process_quota_precedes_execution_and_closed_handles_receive_no_late_batch_events() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("lifetime-fixture.exe");
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/echo_service.rs");
    assert!(
        std::process::Command::new("rustc")
            .arg(source)
            .arg("-o")
            .arg(&exe)
            .status()
            .unwrap()
            .success()
    );
    let package = executable_package(exe.to_str().unwrap(), true);
    let id = &package.manifest.id;
    let mut manager =
        plugin_runtime::Manager::open(dir.path().join("plugins"), Default::default()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    for _ in 0..128 {
        manager
            .invoke_command(id, "scope-probe", json!({"method":"open_data"}))
            .unwrap();
    }
    let marker = dir.path().join("forbidden.txt");
    let result = probe(
        &mut manager,
        id,
        json!({"kind":"execute", "program":exe, "args":["marker",marker], "transport":{"kind":"stdio"}}),
    );
    assert_eq!(result["Err"]["code"], "limit_exceeded");
    assert!(!marker.exists(), "a rejected start executed native code");
    assert_eq!(manager.live[id].process_count(), 0);
    manager.disable(id).unwrap();
    manager.enable(id).unwrap();
    manager
        .invoke_command(id, "close-on-output", json!(null))
        .unwrap();
    let start = probe(
        &mut manager,
        id,
        json!({"kind":"start_service", "service":"echo"}),
    );
    assert!(start.get("Ok").is_some(), "service failed: {start}");
    probe(
        &mut manager,
        id,
        json!({"kind":"write", "handle":start["Ok"]["Resource"], "bytes":b"exit\n"}),
    );
    // Let more than one output chunk reach the queue before the first guest callback closes it.
    std::thread::sleep(std::time::Duration::from_millis(200));
    manager.live.get_mut(id).unwrap().poll().unwrap();
    assert_eq!(manager.live[id].process_count(), 0);
    assert_eq!(
        process_events(&mut manager, id).len(),
        1,
        "output arrived after callback closed its handle"
    );
    // A naturally exited child may already have been reaped when its first output callback runs.
    manager.disable(id).unwrap();
    manager.enable(id).unwrap();
    manager
        .invoke_command(id, "close-on-output", json!(null))
        .unwrap();
    let marker = dir.path().join("short-completed.txt");
    let result = probe(
        &mut manager,
        id,
        json!({"kind":"execute", "program":exe, "args":["short",marker], "transport":{"kind":"stdio"}}),
    );
    assert!(result.get("Ok").is_some(), "short service failed: {result}");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !marker.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(marker.exists());
    #[cfg(windows)]
    while manager.live[id].process_ids().into_iter().any(running)
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    std::thread::sleep(std::time::Duration::from_millis(50));
    manager.live.get_mut(id).unwrap().poll().unwrap();
    assert_eq!(
        process_events(&mut manager, id).len(),
        1,
        "natural exit bypassed callback cancellation"
    );
}

/// Real ConPTY preserves interactive spaces and argv boundaries while resize and normal exit work.
#[cfg(windows)]
#[test]
#[ignore = "build with scripts/build-capability-example.ps1 first"]
fn interactive_pty_accepts_spaces_and_preserves_quoted_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("interactive fixture.exe");
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/echo_service.rs");
    assert!(
        std::process::Command::new("rustc")
            .arg(source)
            .arg("-o")
            .arg(&exe)
            .status()
            .unwrap()
            .success()
    );
    let package = executable_package(exe.to_str().unwrap(), true);
    let id = &package.manifest.id;
    let mut manager =
        plugin_runtime::Manager::open(dir.path().join("plugins"), Default::default()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let argument = "space \"quote\" slash\\";
    let start = probe(
        &mut manager,
        id,
        json!({"kind":"execute","program":exe,"args":["interactive",argument],"transport":{"kind":"pty","columns":100,"rows":30}}),
    );
    assert!(start.get("Ok").is_some(), "PTY launch failed: {start}");
    let handle = &start["Ok"]["Resource"];
    assert_eq!(
        probe(
            &mut manager,
            id,
            json!({"kind":"resize","handle":handle,"columns":110,"rows":36})
        )["Ok"],
        "Unit"
    );
    std::thread::sleep(std::time::Duration::from_millis(200));
    manager.live.get_mut(id).unwrap().poll().unwrap();
    assert_eq!(
        probe(
            &mut manager,
            id,
            json!({"kind":"write","handle":handle,"bytes":b"hello with spaces\r"})
        )["Ok"],
        "Unit"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while manager.live[id].process_count() > 0 && std::time::Instant::now() < deadline {
        manager.live.get_mut(id).unwrap().poll().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(manager.live[id].process_count(), 0, "PTY did not exit");
    let events = process_events(&mut manager, id);
    let mut bytes = Vec::new();
    for event in &events {
        if let Some(output) = event.get("Output") {
            assert_eq!(output["stream"], "pty");
            bytes.extend(serde_json::from_value::<Vec<u8>>(output["bytes"].clone()).unwrap());
        }
    }
    let output = String::from_utf8_lossy(&bytes);
    assert!(
        output.contains(&format!("argument:{argument}")),
        "argv lost: {output:?}"
    );
    assert!(
        output.contains("received:hello with spaces"),
        "input lost: {output:?}"
    );
    assert_eq!(events.last().unwrap()["Exited"]["code"], 0);
    assert_eq!(
        probe(
            &mut manager,
            id,
            json!({"kind":"write","handle":handle,"bytes":[]})
        )["Err"]["code"],
        "invalid_handle"
    );
}

/// Build through the same ZIP inspector as installation, without host-private constructors.
fn package(service: serde_json::Value) -> anyhow::Result<Package> {
    let manifest = json!({
        "id":"process-fixture", "name":"Process fixture", "version":"1.0.0",
        "protocol":7, "api":{"base":"^1", "required":{"process":"^1"}},
        "component":"guest.wasm", "storage_limit":1024,
        "permissions":["process.service.echo"], "services":{"echo":service}
    });
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("manifest.json", serde_json::to_vec(&manifest)?),
        ("guest.wasm", b"\0asm\x0d\0\x01\0".to_vec()),
    ] {
        archive.start_file(name, zip::write::SimpleFileOptions::default())?;
        archive.write_all(&bytes)?;
    }
    Package::from_bytes(&archive.finish()?.into_inner())
}

/// Service authority is a fixed executable and argument list, never a caller-supplied template.
#[test]
fn declared_stdio_service_is_inspected_without_running_or_retargeting_it() {
    let valid = package(json!({"program":"echo-service", "args":["--stdio"]}));
    assert!(valid.is_ok(), "valid service rejected: {:?}", valid.err());
    for invalid in [
        json!({"program":"../workspace/service", "args":[]}),
        json!({"program":"echo-service", "args":[], "install":"execute anything"}),
        json!({"program":"echo-service", "args":[], "shell":true}),
    ] {
        assert!(
            package(invalid).is_err(),
            "unsafe service declaration accepted"
        );
    }
}

/// Both native transports own immediate descendants; grant rejection must leave the old instance usable.
#[cfg(windows)]
#[test]
#[ignore = "build with scripts/build-capability-example.ps1 first"]
fn native_trees_are_reclaimed_and_added_execution_grants_require_consent() {
    use plugin_runtime::{Manager, plugin_protocol::Environment};
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("tree fixture.exe");
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/echo_service.rs");
    assert!(
        std::process::Command::new("rustc")
            .arg(source)
            .arg("-o")
            .arg(&exe)
            .status()
            .unwrap()
            .success()
    );
    let original = executable_package(exe.to_str().unwrap(), false);
    let expanded = executable_package(exe.to_str().unwrap(), true);
    let id = &original.manifest.id;
    let mut manager = Manager::open(
        dir.path().join("plugins"),
        Environment {
            workspace: dir.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&original, original.manifest.permissions.clone())
        .unwrap();
    assert!(
        manager
            .install(&expanded, original.manifest.permissions.clone())
            .is_err()
    );
    assert_eq!(manager.installed[id].grants, original.manifest.permissions);
    assert!(
        probe(
            &mut manager,
            id,
            json!({"kind":"start_service", "service":"echo"})
        )
        .get("Ok")
        .is_some()
    );
    manager
        .install(&expanded, expanded.manifest.permissions.clone())
        .unwrap();
    for (index, transport) in [
        json!({"kind":"stdio"}),
        json!({"kind":"pty","columns":80,"rows":24}),
    ]
    .into_iter()
    .enumerate()
    {
        let marker = dir.path().join(format!("child {index}.txt"));
        let start = probe(
            &mut manager,
            id,
            json!({"kind":"execute","program":exe,"args":["tree",marker],"transport":transport}),
        );
        assert!(start.get("Ok").is_some(), "native launch failed: {start}");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !marker.exists() && std::time::Instant::now() < deadline {
            manager.live.get_mut(id).unwrap().poll().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let child: u32 = std::fs::read_to_string(marker).unwrap().parse().unwrap();
        assert!(running(child), "fixture descendant never started");
        let parents = manager.live[id].process_ids();
        if index == 0 {
            let result = probe(
                &mut manager,
                id,
                json!({"kind":"terminate","handle":start["Ok"]["Resource"]}),
            );
            assert_eq!(result["Ok"]["Process"], "Terminated");
        } else {
            assert_eq!(
                probe(
                    &mut manager,
                    id,
                    json!({"kind":"resize","handle":start["Ok"]["Resource"],"columns":110,"rows":36})
                )["Ok"],
                "Unit"
            );
            manager.disable(id).unwrap();
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while (running(child) || parents.iter().any(|pid| running(*pid)))
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!running(child), "descendant survived release");
        assert!(
            parents.into_iter().all(|pid| !running(pid)),
            "parent survived release"
        );
    }
    manager.set_workspace_trust(false).unwrap();
    assert!(
        manager
            .install(&expanded, expanded.manifest.permissions.clone())
            .is_err()
    );
    assert!(manager.enable(id).is_err());
    assert_eq!(manager.resource_count(), 0);
}

/// OS liveness is observable behavior; no runtime-private process table is inspected by cleanup tests.
#[cfg(windows)]
fn running(pid: u32) -> bool {
    use windows_sys::Win32::{Foundation::*, System::Threading::*};
    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            return false;
        }
        let live = WaitForSingleObject(handle, 0) == WAIT_TIMEOUT;
        CloseHandle(handle);
        live
    }
}
