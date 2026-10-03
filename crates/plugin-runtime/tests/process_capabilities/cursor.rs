//! An independent guest owns the cursor query; unresponsive consumers must not strand native work.
use super::*;
use std::time::{Duration, Instant};

/// Exercise actual ConPTY through a separately built guest, including incomplete startup teardown.
#[test]
#[ignore = "build with scripts/build-capability-example.ps1 first"]
fn cursor_handshake_is_public_and_unanswered_queries_release_native_resources() {
    // Bound the entire fixture even if an OS call accidentally blocks the manager thread again.
    let (done, result) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        run();
        done.send(()).unwrap();
    });
    result
        .recv_timeout(Duration::from_secs(180))
        .expect("cursor lifecycle blocked");
    worker.join().unwrap();
}

/// Query process handle counts after warm-up to detect leaked ConPTY workers/pipe ownership.
fn handles() -> u32 {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};
    let mut count = 0;
    assert_ne!(
        unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) },
        0
    );
    count
}

/// Await native teardown rather than considering a removed manager entry proof of OS cleanup.
fn wait_for_handles(baseline: u32) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while handles() > baseline + 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        handles() <= baseline + 2,
        "native handles leaked: before {baseline}, after {}",
        handles()
    );
}

/// Ordinary guest-published process events expose bytes without accessing host transport internals.
fn bytes(manager: &mut plugin_runtime::Manager, id: &str, offset: usize) -> Vec<u8> {
    process_events(manager, id)
        .into_iter()
        .skip(offset)
        .filter_map(|event| {
            event
                .get("Output")
                .and_then(|output| serde_json::from_value::<Vec<u8>>(output["bytes"].clone()).ok())
        })
        .flatten()
        .collect()
}

/// Verify responsive and nonresponsive consumers using the same transport flag and resource API.
fn run() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("cursor fixture.exe");
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/echo_service.rs");
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
    // The first native creation error initializes OS/error-reporting handles. Warm that path
    // before measuring repeated teardown; one-time loader state is not a per-console leak.
    let bad = dir.path().join("invalid.exe");
    std::fs::write(&bad, b"not an executable").unwrap();
    assert!(probe(&mut manager, id, json!({"kind":"execute", "program":bad, "args":[], "transport":{"kind":"pty", "columns":80, "rows":24, "inherit_cursor":true}})).get("Err").is_some());
    std::thread::sleep(Duration::from_millis(200));
    // Every round shares this baseline, so a small per-cycle leak cannot reset its own allowance.
    let baseline = handles();
    for round in 0..4 {
        let offset = process_events(&mut manager, id).len();
        let started = probe(
            &mut manager,
            id,
            json!({
                "kind":"execute", "program":exe, "args":["interactive", "cursor-client"],
                "transport":{"kind":"pty", "columns":80, "rows":24, "inherit_cursor":true}
            }),
        );
        assert!(started.get("Ok").is_some(), "{started}");
        let handle = &started["Ok"]["Resource"];
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            manager.poll();
            if bytes(&mut manager, id, offset)
                .windows(4)
                .any(|part| part == b"\x1b[6n")
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "guest did not receive cursor query"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        if round == 0 {
            // Any SDK guest can answer the query; the host neither fabricates nor consumes it.
            assert_eq!(
                probe(
                    &mut manager,
                    id,
                    json!({"kind":"write", "handle":handle, "bytes":b"\x1b[3;1Rhello cursor\r"})
                )["Ok"],
                "Unit"
            );
            while manager.live[id].process_count() > 0 {
                manager.poll();
                assert!(
                    Instant::now() < deadline,
                    "responsive cursor client did not exit"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(
                String::from_utf8_lossy(&bytes(&mut manager, id, offset))
                    .contains("received:hello cursor")
            );
        } else {
            assert_eq!(
                probe(
                    &mut manager,
                    id,
                    json!({"kind":"resize", "handle":handle, "columns":120, "rows":36})
                )["Ok"],
                "Unit"
            );
            // Dispatch resize while the query remains unanswered, then retain responsive ownership calls.
            std::thread::sleep(Duration::from_millis(200));
            let start = Instant::now();
            manager.poll();
            match round {
                1 => assert_eq!(
                    probe(
                        &mut manager,
                        id,
                        json!({"kind":"terminate", "handle":handle})
                    )["Ok"]["Process"],
                    "Terminated"
                ),
                2 => manager.disable(id).unwrap(),
                _ => {
                    // External exit must release an unanswered query without waiting for output EOF.
                    use windows_sys::Win32::{Foundation::CloseHandle, System::Threading::*};
                    for pid in manager.live[id].process_ids() {
                        let child = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
                        assert!(!child.is_null());
                        assert_ne!(unsafe { TerminateProcess(child, 0) }, 0);
                        unsafe {
                            CloseHandle(child);
                        }
                    }
                    while manager.live[id].process_count() > 0 {
                        manager.poll();
                        assert!(
                            start.elapsed() < Duration::from_secs(5),
                            "unanswered query prevented exit completion"
                        );
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
            }
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "query blocked resize or termination"
            );
        }
        wait_for_handles(baseline);
        if round == 2 {
            manager.enable(id).unwrap();
        }
        // Process creation can fail after ConPTY has started its query; pipe unwinding must finish.
        assert!(probe(&mut manager, id, json!({"kind":"execute", "program":bad, "args":[], "transport":{"kind":"pty", "columns":80, "rows":24, "inherit_cursor":true}})).get("Err").is_some());
        wait_for_handles(baseline);
    }
    manager.disable(id).unwrap();
    assert!(!manager.live.contains_key(id));
}

use std::path::Path;
