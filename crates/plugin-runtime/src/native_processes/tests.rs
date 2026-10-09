//! Saturated data queues must never obstruct authority revocation or confirmed cleanup.

use super::*;

/// Retiring one origin must bypass diagnostic backpressure while another owned process keeps running.
#[test]
#[cfg(windows)]
fn single_session_retirement_bypasses_saturated_data_without_stopping_its_neighbor() {
    let directory = tempfile::tempdir().unwrap();
    let group = NativeProcessGroup::new(true);
    for id in [1, 2] {
        let marker = directory.path().join(format!("{id}.pid"));
        group
            .launch(
                id,
                NativeLaunch {
                    program: "powershell.exe".into(),
                    args: vec![
                        "-NoLogo".into(),
                        "-NoProfile".into(),
                        "-Command".into(),
                        format!(
                            "[IO.File]::WriteAllText('{}', $PID); Start-Sleep -Seconds 60",
                            marker.display().to_string().replace('\'', "''")
                        ),
                    ],
                    cwd: directory.path().display().to_string(),
                    env: BTreeMap::new(),
                    transport: Transport::Stdio,
                },
            )
            .unwrap();
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !directory.path().join("2.pid").exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "fixture never started"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let first = std::fs::read_to_string(directory.path().join("1.pid"))
        .unwrap()
        .parse()
        .unwrap();
    let second = std::fs::read_to_string(directory.path().join("2.pid"))
        .unwrap()
        .parse()
        .unwrap();
    // Keep feeding diagnostic-producing input until both retained output and input queues saturate.
    // No UI receiver drains events; the public close operation must not depend on that receiver.
    let fill_until = std::time::Instant::now() + Duration::from_millis(500);
    while std::time::Instant::now() < fill_until {
        if group.write(999, vec![b'x']).is_err() {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    group
        .close(1)
        .expect("retirement cannot share the saturated input queue");
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while native_pid_alive(first) {
        assert!(
            std::time::Instant::now() < deadline,
            "single-origin retirement is blocked by data"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        native_pid_alive(second),
        "retiring one origin must not revoke its neighbor"
    );
    group
        .shutdown()
        .unwrap()
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
}

/// Query native liveness independently from the supervisor's logical terminal events.
#[cfg(windows)]
fn native_pid_alive(pid: u32) -> bool {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return false;
        }
        let mut code = 0;
        let ok = GetExitCodeProcess(process, &mut code);
        CloseHandle(process);
        assert_ne!(ok, 0);
        code == 259
    }
}

/// Close must deliver queued output and confirm native exit; cleanup admission is not an exit event.
#[test]
#[cfg(windows)]
fn close_preserves_final_output_and_observes_real_exit() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("created.pid");
    let script = format!(
        "$info=New-Object Diagnostics.ProcessStartInfo; $info.FileName='powershell.exe'; $info.Arguments='-NoLogo -NoProfile -Command \"Start-Sleep -Seconds 60\"'; $info.UseShellExecute=$false; $info.CreateNoWindow=$true; $info.RedirectStandardOutput=$true; $info.RedirectStandardError=$true; $child=[Diagnostics.Process]::Start($info); [Console]::Write(('x' * (512 * 1024)) + 'FINAL_OUTPUT'); [IO.File]::WriteAllText('{}', ($PID.ToString()+' '+$child.Id)); Start-Sleep -Seconds 60",
        marker.display().to_string().replace('\'', "''")
    );
    let group = NativeProcessGroup::new(true);
    group
        .launch(
            1,
            NativeLaunch {
                program: "powershell.exe".into(),
                args: vec![
                    "-NoLogo".into(),
                    "-NoProfile".into(),
                    "-Command".into(),
                    script,
                ],
                cwd: directory.path().display().to_string(),
                env: BTreeMap::new(),
                transport: Transport::Stdio,
            },
        )
        .unwrap();
    // The marker proves all bytes were written before Close. Keep the payload below backpressure
    // capacity so a blocked Write cannot prevent the marker; saturation is tested separately.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !marker.exists() {
        if std::time::Instant::now() >= deadline {
            let bytes = group
                .poll()
                .into_iter()
                .filter_map(|event| match event {
                    NativeProcessEvent::Update {
                        update: Update::Output { bytes, .. },
                        ..
                    } => Some(bytes),
                    _ => None,
                })
                .flatten()
                .collect::<Vec<_>>();
            panic!(
                "native output did not reach the marker: {}",
                String::from_utf8_lossy(&bytes[..bytes.len().min(1800)])
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let pids = std::fs::read_to_string(marker)
        .unwrap()
        .split_whitespace()
        .map(|pid| pid.parse::<u32>().unwrap())
        .collect::<Vec<_>>();
    group.close(1).unwrap();
    let mut bytes = Vec::new();
    let mut ended = false;
    while !ended {
        for event in group.poll() {
            match event {
                NativeProcessEvent::Update {
                    update: Update::Output { bytes: chunk, .. },
                    ..
                } => bytes.extend(chunk),
                NativeProcessEvent::Update {
                    update: Update::Terminated,
                    ..
                } => ended = true,
                NativeProcessEvent::Failed { message, .. } => panic!("close failed: {message}"),
                _ => {}
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "native close has no exit observation"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        bytes.ends_with(b"FINAL_OUTPUT"),
        "Close must not discard buffered output"
    );
    // A logical Terminated event is insufficient: query the actual OS process state.
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    // The descendant redirects both streams, so EOF alone cannot prove its exit.
    for pid in pids {
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if !process.is_null() {
                let mut code = 0;
                assert_ne!(GetExitCodeProcess(process, &mut code), 0);
                CloseHandle(process);
                assert_ne!(
                    code, 259,
                    "a running native child cannot be reported terminated"
                );
            }
        }
    }
    group
        .shutdown()
        .unwrap()
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
}

/// A new grant can arrive while the supervisor is waiting for data; it must retain its new lease.
#[test]
fn renewed_authority_does_not_retire_a_new_launch() {
    let directory = tempfile::tempdir().unwrap();
    let launch = || {
        let (program, args) = if cfg!(windows) {
            (
                "powershell.exe",
                vec![
                    "-NoLogo",
                    "-NoProfile",
                    "-Command",
                    "Start-Sleep -Seconds 30",
                ],
            )
        } else {
            ("/bin/sleep", vec!["30"])
        };
        NativeLaunch {
            program: program.into(),
            args: args.into_iter().map(str::to_owned).collect(),
            cwd: directory.path().to_string_lossy().into_owned(),
            env: BTreeMap::new(),
            transport: Transport::Stdio,
        }
    };
    let group = NativeProcessGroup::new(true);
    group.launch(1, launch()).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !group
        .poll()
        .iter()
        .any(|event| matches!(event, NativeProcessEvent::Started { session: 1 }))
    {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    // Leave the worker in its ordinary receive window, then wake it with the new authorized command.
    std::thread::sleep(Duration::from_millis(2));
    group.set_trusted(false).unwrap();
    group.set_trusted(true).unwrap();
    group.launch(2, launch()).unwrap();
    // Actual process creation can take longer under the full workspace suite's parallel load.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut started = false;
    let mut observed_until = deadline;
    while std::time::Instant::now() < observed_until {
        for event in group.poll() {
            match event {
                NativeProcessEvent::Started { session: 2 } => {
                    started = true;
                    observed_until = std::time::Instant::now() + Duration::from_millis(250);
                }
                NativeProcessEvent::Update {
                    session: 2,
                    update: Update::Exited { .. } | Update::Terminated,
                } => panic!("new authority's child was retired by the old revocation"),
                NativeProcessEvent::Failed {
                    session: 2,
                    message,
                    ..
                } => panic!("new launch failed: {message}"),
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(started);
    group
        .shutdown()
        .unwrap()
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
}

/// Exercise only the public native group: no UI receiver drains its deliberately noisy errors.
#[test]
fn revocation_and_shutdown_bypass_saturated_data_queues() {
    let directory = tempfile::tempdir().unwrap();
    let group = NativeProcessGroup::new(true);
    let (program, args) = if cfg!(windows) {
        (
            "powershell.exe",
            vec![
                "-NoLogo",
                "-NoProfile",
                "-Command",
                "Start-Sleep -Seconds 30",
            ],
        )
    } else {
        ("/bin/sleep", vec!["30"])
    };
    group
        .launch(
            1,
            NativeLaunch {
                program: program.into(),
                args: args.into_iter().map(str::to_owned).collect(),
                cwd: directory.path().to_string_lossy().into_owned(),
                env: BTreeMap::new(),
                transport: Transport::Stdio,
            },
        )
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !group
        .poll()
        .iter()
        .any(|event| matches!(event, NativeProcessEvent::Started { session: 1 }))
    {
        assert!(
            std::time::Instant::now() < deadline,
            "real native child did not start"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    // Invalid owned IDs produce bounded diagnostic events and eventually apply backpressure.
    let mut busy = false;
    for _ in 0..100_000 {
        if group.write(999, vec![b'x']).is_err() {
            busy = true;
            break;
        }
    }
    assert!(busy, "the public input queue must remain bounded");
    group
        .set_trusted(false)
        .expect("revocation cannot be dropped behind ordinary input");
    assert!(
        group.write(1, vec![b'x']).is_err(),
        "revocation must also deny further input"
    );
    group
        .shutdown()
        .unwrap()
        .recv_timeout(Duration::from_secs(5))
        .expect("cleanup cannot depend on draining visible output");
}
