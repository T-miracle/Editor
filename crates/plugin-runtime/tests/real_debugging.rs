//! Actual independently built WASM -> public Manager -> privately installed CodeLLDB -> Rust/PDB.
#![cfg(windows)]
#[path = "support/debugger_packages.rs"]
mod packages;
use plugin_runtime::{
    DebugRequest, DebugState, Manager,
    plugin_protocol::{Environment, api::RequestUpdate},
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// Every wait uses the public request gate and native observed state, never an assumed delay result.
fn answer(manager: &mut Manager, request: DebugRequest) -> Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match request.status() {
            RequestUpdate::Completed { result } => {
                return result
                    .unwrap_or_else(|error| panic!("{}: {}", request.session(), error.message));
            }
            RequestUpdate::Cancelled { reason, .. } => panic!("request cancelled: {reason:?}"),
            _ => {
                assert!(
                    Instant::now() < deadline,
                    "debug exchange exceeded acceptance deadline"
                );
                manager.poll();
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}
fn call(manager: &mut Manager, method: &str, mut arguments: Value) -> Value {
    // Controls carry the epoch the user observed; never rewrite an explicitly stale epoch.
    if matches!(method, "step" | "resume") && arguments.get("pause").is_none() {
        let state = manager
            .debug_status(arguments["session"].as_str().unwrap())
            .unwrap();
        arguments["pause"] = json!(state.pause.unwrap());
    }
    let request = manager.begin_debug_call(method, arguments).unwrap();
    answer(manager, request)
}
fn paused(manager: &mut Manager, session: &str) -> plugin_runtime::DebugSession {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let state = manager.debug_status(session).unwrap();
        if state.state == DebugState::Paused && state.source.is_some() {
            return state;
        }
        assert!(
            Instant::now() < deadline,
            "real target never paused: {state:?}; views: {:?}",
            manager
                .live
                .iter()
                .map(|(id, instance)| (id, &instance.views))
                .collect::<Vec<_>>()
        );
        manager.poll();
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A genuine target blocks on PTY stdin, then hits a PDB breakpoint; there is no ordinary-run copy.
#[test]
#[ignore = "build the current rust-debugger package and prepare the pinned official CodeLLDB VSIX"]
fn real_debugging_stdin_runs_one_target_and_preserves_inspection() {
    let root = tempfile::tempdir().unwrap();
    let (source, binary) = packages::interactive_program(root.path());
    let package = packages::debugger("independent-interactive-debugger");
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let start = manager.begin_configured_debug_call(Some("input-config"), "start",
        json!({"program":binary.display().to_string(),"args":[],"cwd":root.path().display().to_string(),"name":"Interactive",
            "breakpoints":[{"source":source.display().to_string(),"line":4}]})).unwrap();
    let receipt = answer(&mut manager, start);
    let session = receipt["session"].as_str().unwrap().to_owned();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut terminal = None;
    let mut transcript = Vec::new();
    while !String::from_utf8_lossy(&transcript).contains("WAITING_FOR_INPUT") {
        manager.poll();
        for message in manager.take_terminal_presentations() {
            let owner = message
                .owner
                .as_ref()
                .expect("debug invocation authenticates placement");
            assert_eq!(owner.configuration.as_deref(), Some("input-config"));
            assert_eq!(owner.invocation, session);
            terminal = Some(message.handle);
            for update in message.updates {
                if let plugin_runtime::plugin_protocol::process::Update::Output { bytes, .. } =
                    update
                {
                    transcript.extend(bytes);
                }
            }
        }
        assert!(
            Instant::now() < deadline,
            "real debug target has no presented stdin/output: {}",
            String::from_utf8_lossy(&transcript)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let terminal = terminal.unwrap();
    let mut foreign = terminal.clone();
    foreign.instance.push_str("-foreign");
    assert!(manager.terminal_input(&foreign, b"99\r").is_err());
    manager.terminal_resize(&terminal, 100, 20).unwrap();
    manager.terminal_input(&terminal, b"7\r").unwrap();
    let stopped = paused(&mut manager, &session);
    assert_eq!(stopped.line, Some(4));
    let frames = manager
        .debug_frames(&session, stopped.pause.unwrap())
        .unwrap();
    let frame = frames
        .iter()
        .find(|frame| frame.name.contains("calculate"))
        .unwrap();
    let variables = manager
        .debug_variables(&session, stopped.pause.unwrap(), frame.id)
        .unwrap();
    assert!(
        variables
            .iter()
            .any(|value| value.name == "value" && value.value == "7"),
        "{variables:?}"
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("starts.txt"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    call(&mut manager, "resume", json!({"session":session}));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !String::from_utf8_lossy(&transcript).contains("INPUT_RESULT:15") {
        manager.poll();
        for message in manager.take_terminal_presentations() {
            for update in message.updates {
                if let plugin_runtime::plugin_protocol::process::Update::Output { bytes, .. } =
                    update
                {
                    transcript.extend(bytes);
                }
            }
        }
        assert!(Instant::now() < deadline, "input result missing");
        std::thread::sleep(Duration::from_millis(10));
    }
    call(&mut manager, "stop", json!({"session":session}));
    assert_eq!(
        manager.live["independent-interactive-debugger"].process_count(),
        0
    );
    assert!(manager.terminal_input(&terminal, b"late\r").is_err());
    manager.shutdown();
}
/// Breakpoints and locals must come from the actual MSVC PDB, through a source-owned deferred call.
#[test]
#[ignore = "build rust-debugger through the current host SDK and download the pinned CodeLLDB VSIX first"]
fn real_rust_pdb_breakpoints_locals_and_step_controls_are_public() {
    let root = tempfile::tempdir().unwrap();
    let (source, binary) = packages::program(root.path());
    let package = packages::debugger("alternative-rust-debug");
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    assert_eq!(
        manager.live["alternative-rust-debug"].process_count(),
        0,
        "preparing/installing must not launch an adapter"
    );
    let receipt = call(
        &mut manager,
        "start",
        json!({"program":binary.display().to_string(),"args":[],"cwd":root.path().display().to_string(),"breakpoints":[{"source":source.display().to_string(),"line":8}]}),
    );
    let session = receipt["session"].as_str().unwrap().to_owned();
    let stopped = paused(&mut manager, &session);
    eprintln!("breakpoint: {stopped:?}");
    assert_eq!(stopped.line, Some(8));
    // Invalid input must leave the real pause inspectable; a rejected command never moved it.
    let invalid = manager
        .begin_debug_call(
            "step",
            json!({"session":session,"pause":stopped.pause.unwrap(),"kind":"not-a-step"}),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        manager.poll();
        if let RequestUpdate::Completed { result } = invalid.status() {
            assert!(result.is_err());
            break;
        }
        assert!(Instant::now() < deadline, "invalid step must fail promptly");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        manager.debug_status(&session).unwrap().state,
        DebugState::Paused
    );
    assert_eq!(manager.debug_status(&session).unwrap().pause, stopped.pause);
    let frames = manager
        .debug_frames(&session, stopped.pause.unwrap())
        .unwrap();
    assert!(
        frames.iter().any(|frame| frame.name.contains("main")),
        "actual Rust stack expected: {frames:?}"
    );
    call(&mut manager, "step", json!({"session":session,"kind":"in"}));
    let in_pause = paused(&mut manager, &session);
    eprintln!("step in paused");
    let frames = manager
        .debug_frames(&session, in_pause.pause.unwrap())
        .unwrap();
    let frame = frames
        .iter()
        .find(|frame| frame.name.contains("calculate"))
        .expect("step-in must enter the real Rust function");
    let frame_id = frame.id;
    let variables = manager
        .debug_variables(&session, in_pause.pause.unwrap(), frame_id)
        .unwrap();
    assert!(
        variables
            .iter()
            .any(|variable| variable.name == "value" && variable.value == "5"),
        "real local expected: {variables:?}"
    );
    call(
        &mut manager,
        "step",
        json!({"session":session,"kind":"over"}),
    );
    let over_pause = paused(&mut manager, &session);
    assert!(over_pause.pause.unwrap() > in_pause.pause.unwrap());
    // DAP may reuse a numeric frame ID. An old pause must still be rejected at the public boundary.
    assert!(
        manager
            .debug_variables(&session, in_pause.pause.unwrap(), frame_id)
            .is_err()
    );
    assert!(
        manager
            .debug_frames(&session, in_pause.pause.unwrap())
            .is_err()
    );
    eprintln!("step over paused");
    call(
        &mut manager,
        "step",
        json!({"session":session,"kind":"out"}),
    );
    let out_pause = paused(&mut manager, &session);
    eprintln!("step out paused");
    assert!(
        manager
            .debug_frames(&session, out_pause.pause.unwrap())
            .unwrap()[0]
            .name
            .contains("main")
    );
    call(&mut manager, "resume", json!({"session":session}));
    assert_eq!(
        manager.debug_status(&session).unwrap().state,
        DebugState::Running
    );
    call(&mut manager, "pause", json!({"session":session}));
    paused(&mut manager, &session);
    let result = call(&mut manager, "stop", json!({"session":session}));
    assert_eq!(result["state"], "exited");
    assert_eq!(
        manager.live["alternative-rust-debug"].process_count(),
        0,
        "actual adapter tree must be released"
    );
    manager.shutdown();
}

/// The public consumer receives a bounded failure, while adapter refusal leaves its pause usable.
#[test]
#[ignore = "build rust-debugger through the current public SDK first"]
fn adapter_refusal_and_oversized_inspection_finish_without_poisoning_the_pause() {
    let root = tempfile::tempdir().unwrap();
    let package = packages::fault_adapter("debug-fault-instrument", root.path());
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let receipt = call(
        &mut manager,
        "start",
        json!({"program":"instrument-only","args":[]}),
    );
    let session = receipt["session"].as_str().unwrap().to_owned();
    let stop = paused(&mut manager, &session);
    let request = manager
        .begin_debug_call(
            "step",
            json!({"session":session,"pause":stop.pause.unwrap(),"kind":"over"}),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        manager.poll();
        if let RequestUpdate::Completed { result } = request.status() {
            assert!(
                result
                    .unwrap_err()
                    .message
                    .contains("STEP_REFUSED_BY_INSTRUMENT")
            );
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let state = manager.debug_status(&session).unwrap();
    assert_eq!(state.state, DebugState::Paused);
    assert_eq!(state.pause, stop.pause);
    let frames = manager.debug_frames(&session, stop.pause.unwrap()).unwrap();
    let request = manager
        .begin_debug_call(
            "variables",
            json!({"session":session,"pause":stop.pause.unwrap(),"frame":frames[0].id}),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        manager.poll();
        if let RequestUpdate::Completed { result } = request.status() {
            let error = result.unwrap_err();
            assert_eq!(
                error.code,
                plugin_runtime::plugin_protocol::api::ErrorCode::LimitExceeded
            );
            assert!(error.message.contains("byte budget"));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "oversized inspection must return an error, never time out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        manager.debug_status(&session).unwrap().state,
        DebugState::Paused
    );
    call(&mut manager, "stop", json!({"session":session}));
    assert_eq!(manager.live["debug-fault-instrument"].process_count(), 0);
    manager.shutdown();
}

/// Native PIDs, rather than guest process counts, verify tree retirement and prevent overlapping reruns.
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
        queried && code == 259
    }
}
fn actual_tree(manager: &mut Manager, directory: &std::path::Path) -> Vec<u32> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        manager.poll();
        let pids = ["root", "child", "grandchild"]
            .into_iter()
            .filter_map(|extension| {
                std::fs::read_to_string(directory.join("adapter-pids").with_extension(extension))
                    .ok()?
                    .parse::<u32>()
                    .ok()
            })
            .collect::<Vec<_>>();
        if pids.len() == 3 && pids.iter().all(|pid| process_alive(*pid)) {
            return pids;
        }
        assert!(
            Instant::now() < deadline,
            "native tree never became observable"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
/// Ignoring DAP disconnect cannot retain targets forever; Force and normal Stop share the EOF barrier.
#[test]
#[ignore = "build rust-debugger through the current public SDK first"]
fn stop_timeout_and_force_wait_for_actual_tree_before_rerun() {
    let root = tempfile::tempdir().unwrap();
    let package = packages::stubborn_adapter("debug-stop-instrument", root.path());
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    for force in [false, true] {
        let start = manager
            .begin_configured_debug_call(
                Some("stable"),
                "start",
                json!({"program":"instrument-only","args":[]}),
            )
            .unwrap();
        let session = answer(&mut manager, start)["session"]
            .as_str()
            .unwrap()
            .to_owned();
        let pids = actual_tree(&mut manager, root.path());
        let since = Instant::now();
        let request = if force {
            manager.force_debug_session(&session).unwrap()
        } else {
            manager
                .begin_debug_call("stop", json!({"session":session}))
                .unwrap()
        };
        if !force {
            assert!(!request.status().is_terminal());
            assert!(
                manager
                    .begin_configured_debug_call(
                        Some("stable"),
                        "start",
                        json!({"program":"instrument-only","args":[]})
                    )
                    .is_err(),
                "normal disconnect still owns the old target"
            );
        }
        // Admission can finish very quickly on Windows; whenever it is still pending, rerun is refused.
        if !request.status().is_terminal() {
            assert!(
                manager
                    .begin_configured_debug_call(
                        Some("stable"),
                        "start",
                        json!({"program":"instrument-only","args":[]})
                    )
                    .is_err()
            );
        }
        let value = answer(&mut manager, request);
        assert_eq!(value["state"], "exited");
        if !force {
            assert!(since.elapsed() >= Duration::from_secs(3));
        }
        assert!(since.elapsed() < Duration::from_secs(15));
        assert!(
            pids.iter().all(|pid| !process_alive(*pid)),
            "exit receipt preceded native tree retirement: {pids:?}"
        );
        assert_eq!(
            manager.debug_status(&session).unwrap().state,
            DebugState::Exited
        );
        // Remove only this fixture's stale markers, so the next generation must report fresh PIDs.
        for extension in ["root", "child", "grandchild"] {
            std::fs::remove_file(root.path().join("adapter-pids").with_extension(extension))
                .unwrap();
        }
    }
    manager.shutdown();
}

#[test]
#[ignore = "build rust-debugger through the current public SDK first"]
/// Five independent protocol consumers publish decoded text and diagnostics without leaking DAP.
fn debug_output_choices_preserve_every_active_and_ended_session() {
    let root = tempfile::tempdir().unwrap();
    let id = "debug-output-instrument";
    let package = packages::fault_adapter(id, root.path());
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let mut sessions = Vec::new();
    for i in 0..5 {
        let marker = format!("OUTPUT_MARKER_{i}");
        let receipt = call(
            &mut manager,
            "start",
            json!({"program":marker,"args":[],"name":format!("Output {i}")}),
        );
        let host = receipt["session"].as_str().unwrap().to_owned();
        paused(&mut manager, &host);
        sessions.push((host, marker));
    }
    call(&mut manager, "stop", json!({"session":sessions[0].0}));
    assert!(!manager.live[id].views.contains_key("debug-output"));
    let mut histories = std::collections::BTreeMap::<String, String>::new();
    for view in manager.take_terminal_presentations() {
        assert!(
            !view.interactive,
            "decoded adapter output cannot accept target input"
        );
        let text = histories.entry(view.title).or_default();
        for update in view.updates {
            if let plugin_runtime::plugin_protocol::process::Update::Output { bytes, .. } = update {
                text.push_str(&String::from_utf8_lossy(&bytes));
            }
        }
    }
    assert_eq!(histories.len(), 5);
    for (index, (_, marker)) in sessions.iter().enumerate() {
        let text = &histories[&format!("Output {index}")];
        assert!(
            text.contains(marker),
            "decoded target output was lost: {marker}"
        );
        assert!(
            text.contains(&"d".repeat(100)),
            "adapter stderr must remain visible"
        );
        assert!(
            !text.contains("Content-Length"),
            "raw DAP must not enter the terminal"
        );
        assert!(text.len() < 32768);
    }
    manager.shutdown();
}
#[test]
#[ignore = "build rust-debugger through the current public SDK first"]
/// Sealing allocations must still allow the approved disconnect write to the existing adapter.
fn cooperative_disconnect_exits_before_force_grace() {
    let root = tempfile::tempdir().unwrap();
    let id = "cooperative-debug-instrument";
    let package = packages::fault_adapter(id, root.path());
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let receipt = call(
        &mut manager,
        "start",
        json!({"program":"COOPERATIVE_TARGET","args":[]}),
    );
    let session = receipt["session"].as_str().unwrap().to_owned();
    paused(&mut manager, &session);
    let pid = std::fs::read_to_string(root.path().join("adapter-pids.root"))
        .unwrap()
        .parse::<u32>()
        .unwrap();
    assert!(process_alive(pid));
    let since = Instant::now();
    let receipt = call(&mut manager, "stop", json!({"session":session}));
    assert!(
        since.elapsed() < Duration::from_secs(3),
        "cooperative disconnect incorrectly fell back to force"
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("adapter-pids.disconnect")).unwrap(),
        "received"
    );
    assert_eq!(receipt["state"], "exited");
    assert!(!process_alive(pid));
    manager.shutdown();
}

/// An actual initialization refusal retires all three native PIDs and retains its original cause.
#[test]
#[ignore = "build rust-debugger through the current public SDK first"]
fn failed_adapter_initialization_retires_its_actual_native_tree() {
    let root = tempfile::tempdir().unwrap();
    let id = "initialization-failure-instrument";
    let package = packages::initialization_failure_adapter(id, root.path());
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let request = manager
        .begin_debug_call("start", json!({"program":"instrument-only","args":[]}))
        .unwrap();
    let session = request.session().to_owned();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        manager.poll();
        if let RequestUpdate::Completed { result } = request.status() {
            assert!(
                result
                    .unwrap_err()
                    .message
                    .contains("INITIALIZE_REFUSED_BY_INSTRUMENT")
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "failed initialization never answered its caller"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    loop {
        manager.poll();
        let state = manager.debug_status(&session).unwrap();
        if state.state == DebugState::Failed {
            assert!(
                state
                    .reason
                    .as_deref()
                    .is_some_and(|cause| cause.contains("INITIALIZE_REFUSED_BY_INSTRUMENT")),
                "{state:?}"
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "initialization failure never settled: {state:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let pids = ["root", "child", "grandchild"].map(|level| {
        std::fs::read_to_string(root.path().join("adapter-pids").with_extension(level))
            .unwrap()
            .parse::<u32>()
            .unwrap()
    });
    assert!(
        pids.iter().all(|pid| !process_alive(*pid)),
        "failed initialization retained its native tree: {pids:?}"
    );
    assert_eq!(manager.live[id].process_count(), 0);
    manager.shutdown();
}
