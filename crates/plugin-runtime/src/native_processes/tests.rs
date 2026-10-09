//! Saturated data queues must never obstruct authority revocation or confirmed cleanup.

use super::*;

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
    let deadline = std::time::Instant::now() + Duration::from_millis(250);
    let mut started = false;
    while std::time::Instant::now() < deadline {
        for event in group.poll() {
            match event {
                NativeProcessEvent::Started { session: 2 } => started = true,
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
