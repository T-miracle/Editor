//! Real PTYs are requested by an independent SDK consumer across the public service boundary.
#![cfg(windows)]

/// The public input method reaches a retained PTY, and ordered output is readable without panel internals.
#[test]
#[ignore = "build terminal and capability-example through the current public SDK first"]
fn execution_input_and_incremental_output_are_public_and_source_owned() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    install(&mut manager, &terminal());
    install(&mut manager, &fixture("execution-client", false, true));
    assert_eq!(open(&mut manager, "execution-client"), "Service opened");
    execute(
        &mut manager,
        "execution-client",
        json!({
            "program":"powershell.exe", "args":["-NoProfile","-Command","Write-Output 'WAIT_FOR_INPUT'; $line=[Console]::ReadLine(); Write-Output ('ECHO:'+$line); Start-Sleep -Seconds 60"], "name":"public input"
        }),
    );
    wait(&mut manager, |manager| {
        text(manager, "execution-client").contains("started")
    });
    let update: api::RequestUpdate<Value> =
        serde_json::from_str(&text(&manager, "execution-client")).unwrap();
    let api::RequestUpdate::Completed {
        result: Ok(receipt),
    } = update
    else {
        panic!("creation receipt expected");
    };
    let session = receipt["session"].as_str().unwrap();
    command(
        &mut manager,
        "execution-client",
        "service-call",
        json!({"method":"input","value":{"session":session,"bytes":"中文 INPUT\r\n".as_bytes().to_vec()}}),
    );
    manager.poll();
    let answer = text(&manager, "execution-client");
    assert!(
        !answer.contains("unsupported_operation"),
        "the declared public method must actually write input: {answer}"
    );
    let mut cursor = 0u64;
    let mut bytes = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        command(
            &mut manager,
            "execution-client",
            "service-call",
            json!({"method":"events","value":{"session":session,"after":cursor,"limit":16}}),
        );
        manager.poll();
        let update: api::RequestUpdate<Value> =
            serde_json::from_str(&text(&manager, "execution-client")).unwrap();
        if let api::RequestUpdate::Completed { result: Ok(value) } = update {
            let batch: plugin_runtime::plugin_protocol::execution::Batch =
                serde_json::from_value(value).unwrap();
            assert!(!batch.gap);
            cursor = batch.cursor;
            for event in batch.events {
                if let Some(chunk) = event.bytes {
                    bytes.extend(chunk);
                }
            }
            if String::from_utf8_lossy(&bytes).contains("ECHO:中文 INPUT") {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        String::from_utf8_lossy(&bytes).contains("ECHO:中文 INPUT"),
        "actual interleaved output/input expected: {}",
        String::from_utf8_lossy(&bytes)
    );
    manager.shutdown();
}
#[path = "support/interactive_packages.rs"]
mod packages;
use packages::{CONTRACT, fixture, session_consumer};
use plugin_runtime::{
    ExecutionState, Manager, Package,
    plugin_protocol::{Environment, api, ui::Kind},
};
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn install(manager: &mut Manager, package: &Package) {
    manager
        .install(package, package.manifest.permissions.clone())
        .unwrap();
}
fn terminal() -> Package {
    Package::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/terminal.zip"))
        .unwrap()
}
fn text(manager: &Manager, id: &str) -> String {
    let Kind::Text { text } = &manager.live[id].views["welcome"].as_ref().root.kind else {
        panic!("native status expected")
    };
    text.clone()
}
/// Grid paint uses individual cells; concatenate displayed glyphs instead of searching serialized JSON.
fn output(manager: &Manager) -> String {
    let mut result = String::new();
    manager.live["terminal"].views["terminal"]
        .as_ref()
        .root
        .visit(&mut |node| {
            if let Kind::Canvas(canvas) = &node.kind {
                for paint in &canvas.paint {
                    if let plugin_runtime::plugin_protocol::Paint::Text { text, .. } = paint {
                        result.push_str(text);
                    }
                }
            }
        });
    result
}
fn command(manager: &mut Manager, id: &str, command: &str, args: Value) -> String {
    manager.invoke_command(id, command, args).unwrap();
    text(manager, id)
}
fn open(manager: &mut Manager, id: &str) -> String {
    command(manager, id, "service-open", json!(CONTRACT))
}
fn execute(manager: &mut Manager, id: &str, args: Value) -> String {
    command(
        manager,
        id,
        "service-call",
        json!({"method":"execute","value":args}),
    )
}
/// Polling drives actual process/service completion, bounded independently of guest execution fuel.

/// Read an independent consumer's actual terminal service update, including asynchronous host forwards.
fn session_call(manager: &mut Manager, client: &str, method: &str, arguments: Value) -> Value {
    session_result(manager, client, method, arguments).unwrap()
}
/// Preserve typed failures so quota and hostile-handle tests observe the actual public answer.
fn session_result(
    manager: &mut Manager,
    client: &str,
    method: &str,
    arguments: Value,
) -> Result<Value, api::Failure> {
    assert_eq!(
        command(
            manager,
            client,
            "service-call-contract",
            json!({"contract":"session.host","method":method,"value":arguments})
        ),
        "Accepted"
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        manager.poll();
        if let Ok(update) =
            serde_json::from_str::<api::RequestUpdate<Value>>(&text(manager, client))
        {
            match update {
                api::RequestUpdate::Completed { result } => return result,
                api::RequestUpdate::Cancelled { reason, .. } => {
                    panic!("consumer request cancelled: {reason:?}")
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("consumer did not receive its actual provider result");
}

/// Two separately installed real WASM callers share the native default without sharing authority.
#[test]
#[ignore = "package capability-example with the current host SDK into target/plugin-api-test first"]
fn native_execution_two_consumers_have_isolated_input_and_retirement() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    for id in ["native-client-a", "native-client-b"] {
        install(&mut manager, &session_consumer(id));
    }
    assert!(!manager.installed.contains_key("terminal"));
    let mut sessions = Vec::new();
    for client in ["native-client-a", "native-client-b"] {
        let created = session_call(
            &mut manager,
            client,
            "start",
            json!({"program":"powershell.exe","args":["-NoProfile","-Command", "[Console]::InputEncoding=[Text.UTF8Encoding]::new(); [Console]::OutputEncoding=[Text.UTF8Encoding]::new(); Write-Output 'NATIVE_READY'; $line=[Console]::ReadLine(); Write-Output ('NATIVE_ANSWER:'+$line); Start-Sleep -Seconds 60"]}),
        );
        sessions.push(created["session"].as_str().unwrap().to_owned());
        // session.host acknowledges a queued launch before native creation. Only a public status
        // receipt permits subsequent stdin/locate operations; Starting is not a running process.
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let state = session_call(
                &mut manager,
                client,
                "status",
                json!({"session":sessions.last().unwrap()}),
            );
            if state["state"] == "running" {
                break;
            }
            assert_eq!(
                state["state"], "starting",
                "unexpected native creation result: {state}"
            );
            assert!(Instant::now() < deadline, "native creation did not finish");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    assert_ne!(sessions[0], sessions[1]);
    let error = session_result(
        &mut manager,
        "native-client-b",
        "input",
        json!({"session":sessions[0],"bytes":b"FOREIGN\r\n".to_vec()}),
    )
    .unwrap_err();
    assert_eq!(error.code, api::ErrorCode::InvalidHandle);
    let subscribed = session_call(
        &mut manager,
        "native-client-a",
        "subscribe",
        json!({"session":sessions[0]}),
    );
    let subscription = subscribed["subscription"].as_str().unwrap();
    session_call(
        &mut manager,
        "native-client-a",
        "locate",
        json!({"session":sessions[0]}),
    );
    session_call(
        &mut manager,
        "native-client-a",
        "input",
        json!({"session":sessions[0],"bytes":"中文 NATIVE\r\n".as_bytes().to_vec()}),
    );
    let mut bytes = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        let next = session_call(
            &mut manager,
            "native-client-a",
            "next",
            json!({"subscription":subscription,"limit":16}),
        );
        for event in next["events"].as_array().unwrap() {
            if let Some(chunk) = event.get("bytes") {
                bytes.extend(serde_json::from_value::<Vec<u8>>(chunk.clone()).unwrap());
            }
        }
        if String::from_utf8_lossy(&bytes).contains("NATIVE_ANSWER:中文 NATIVE") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        String::from_utf8_lossy(&bytes).contains("NATIVE_ANSWER:中文 NATIVE"),
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    session_call(
        &mut manager,
        "native-client-a",
        "unsubscribe",
        json!({"subscription":subscription}),
    );
    session_call(
        &mut manager,
        "native-client-a",
        "stop",
        json!({"session":sessions[0],"mode":"force"}),
    );
    manager.disable("native-client-b").unwrap();
    wait(&mut manager, |manager| {
        manager
            .executions()
            .iter()
            .all(|execution| !execution.snapshot().state.is_active())
    });
    manager.shutdown();
    assert_eq!(manager.resource_count(), 0);
}

/// One consumer uses input, output/state subscription, locate and unsubscribe with two independent providers.
#[test]
#[ignore = "build terminal and capability-example through the current public SDK first"]
fn a_consumer_subscribes_to_output_and_state_without_borrowing_provider_resources() {
    for package in [terminal(), packages::provider("independent-executor")] {
        let root = tempfile::tempdir().unwrap();
        let mut manager = Manager::open(
            root.path().join("plugins"),
            Environment {
                workspace: root.path().display().to_string(),
                os: "windows".into(),
                ..Default::default()
            },
        )
        .unwrap();
        install(&mut manager, &package);
        install(&mut manager, &session_consumer("session-client"));
        let created = session_call(
            &mut manager,
            "session-client",
            "start",
            json!({
                "program":"powershell.exe",
                "args":["-NoProfile","-Command","[Console]::InputEncoding=[Text.UTF8Encoding]::new(); [Console]::OutputEncoding=[Text.UTF8Encoding]::new(); Write-Output 'SUBSCRIBED_READY'; $line=[Console]::ReadLine(); Write-Output ('FROM_INPUT:'+$line); Start-Sleep -Seconds 60"]
            }),
        );
        let session = created["session"].as_str().unwrap();
        let id = session.parse::<u64>().unwrap();
        wait(&mut manager, |manager| {
            manager.execution(id).unwrap().state() == ExecutionState::Running
        });
        let subscribed = session_call(
            &mut manager,
            "session-client",
            "subscribe",
            json!({"session":session}),
        );
        let subscription = subscribed["subscription"].as_str().unwrap();
        let located = session_call(
            &mut manager,
            "session-client",
            "locate",
            json!({"session":session}),
        );
        assert_eq!(located["session"], session);
        session_call(
            &mut manager,
            "session-client",
            "input",
            json!({"session":session,"bytes":"中文 SUBSCRIPTION\r\n".as_bytes().to_vec()}),
        );
        let mut output = Vec::new();
        let mut last = 0u64;
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            let observed = session_call(
                &mut manager,
                "session-client",
                "next",
                json!({"subscription":subscription,"limit":16}),
            );
            assert_eq!(observed["subscription"], subscription);
            assert_eq!(observed["session"], session);
            assert!(!observed["gap"].as_bool().unwrap());
            for event in observed["events"].as_array().unwrap() {
                let sequence = event["sequence"].as_u64().unwrap();
                assert!(
                    sequence > last,
                    "each observation is delivered once in source order"
                );
                last = sequence;
                if let Some(bytes) = event.get("bytes") {
                    output.extend(serde_json::from_value::<Vec<u8>>(bytes.clone()).unwrap());
                }
            }
            assert_eq!(observed["cursor"].as_u64(), Some(last));
            if String::from_utf8_lossy(&output).contains("FROM_INPUT:中文 SUBSCRIPTION") {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            String::from_utf8_lossy(&output).contains("FROM_INPUT:中文 SUBSCRIPTION"),
            "provider {} output: {}",
            package.manifest.id,
            String::from_utf8_lossy(&output)
        );
        // Stop admission is separate from final state; the subscription stays live through actual exit.
        session_call(
            &mut manager,
            "session-client",
            "stop",
            json!({"session":session,"mode":"force"}),
        );
        wait(&mut manager, |manager| {
            manager.execution(id).unwrap().state() == ExecutionState::Exited
        });
        let ended = session_call(
            &mut manager,
            "session-client",
            "next",
            json!({"subscription":subscription,"limit":16}),
        );
        assert_eq!(ended["state"], "exited");
        assert!(
            ended["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| event["state"] == "terminated")
        );
        let released = session_call(
            &mut manager,
            "session-client",
            "unsubscribe",
            json!({"subscription":subscription}),
        );
        assert_eq!(released["subscription"], subscription);
        assert_eq!(
            manager.execution(id).unwrap().state(),
            ExecutionState::Exited
        );
        manager.shutdown();
    }
}
/// Polling drives actual process/service completion, bounded independently of guest execution fuel.

/// Configuration identity keeps equal commands independent and locates an edited active configuration.
#[test]
#[ignore = "build terminal and capability-example through the current public SDK first"]
fn configuration_identity_controls_deduplication_across_the_public_host_gateway() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    install(&mut manager, &terminal());
    install(&mut manager, &session_consumer("session-client"));
    let mut request = json!({"configuration":"first", "program":"powershell.exe","args":["-NoProfile","-Command","Start-Sleep -Seconds 60"]});
    let first = session_call(&mut manager, "session-client", "start", request.clone());
    request["configuration"] = json!("second");
    let second = session_call(&mut manager, "session-client", "start", request.clone());
    assert_ne!(
        first["session"], second["session"],
        "two saved configurations own independent sessions even with equal argv"
    );
    request["configuration"] = json!("first");
    request["program"] = json!("a-changed-program-that-must-not-launch.exe");
    let repeated = session_call(&mut manager, "session-client", "start", request);
    assert_eq!(first["session"], repeated["session"]);
    assert_eq!(repeated["located"], true);
    assert_eq!(manager.executions().len(), 2);
    manager.shutdown();
}

/// Cancelling the forwarded wait before the provider tick must not deliver queued input.
#[test]
#[ignore = "build terminal and capability-example through the current public SDK first"]
fn cancelled_forwarded_input_never_reaches_the_program() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("input.txt");
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    install(&mut manager, &packages::provider("independent-executor"));
    install(&mut manager, &session_consumer("session-client"));
    let created = session_call(
        &mut manager,
        "session-client",
        "start",
        json!({
            "program":"powershell.exe", "args":["-NoProfile","-Command",
                "Write-Output 'READY'; $line=[Console]::ReadLine(); [IO.File]::WriteAllText($env:INPUT_MARKER,$line); Start-Sleep -Seconds 60"],
            "env":[{"name":"INPUT_MARKER","value":marker.display().to_string()}]
        }),
    );
    let session = created["session"].as_str().unwrap();
    let id = session.parse().unwrap();
    eprintln!(
        "created forwarded-input session: {:?}",
        manager.execution(id).unwrap().snapshot()
    );
    wait(&mut manager, |manager| {
        manager.execution(id).unwrap().state() == ExecutionState::Running
    });
    assert_eq!(
        command(
            &mut manager,
            "session-client",
            "service-call-contract",
            json!({
                "contract":"session.host","method":"input","value":{"session":session,"bytes":b"CANCELLED\r\n".to_vec()}
            })
        ),
        "Accepted"
    );
    // The first tick admits the host forward; the next provider callback has not run yet.
    manager.poll();
    assert_eq!(
        command(
            &mut manager,
            "session-client",
            "service-cancel",
            Value::Null
        ),
        "WaitingStopped"
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        manager.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !marker.exists(),
        "cancelled input escaped its original request gate"
    );
    assert_eq!(
        manager.execution(id).unwrap().state(),
        ExecutionState::Running
    );
    // A paused actor must not admit an operation whose outer deadline passed between ticks.
    assert_eq!(
        command(
            &mut manager,
            "session-client",
            "service-call-contract",
            json!({
                "contract":"session.host","method":"input","timeout_ms":300,
                "value":{"session":session,"bytes":b"EXPIRED\r\n".to_vec()}
            })
        ),
        "Accepted"
    );
    manager.poll();
    std::thread::sleep(Duration::from_millis(400));
    manager.poll();
    std::thread::sleep(Duration::from_millis(200));
    manager.poll();
    assert!(
        !marker.exists(),
        "expired input escaped the parent invocation deadline"
    );
    // Cancelling an input wait does not retire the launch's independent program ownership.
    session_call(
        &mut manager,
        "session-client",
        "input",
        json!({"session":session,"bytes":b"VALID\r\n".to_vec()}),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !marker.exists() && Instant::now() < deadline {
        manager.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        marker.exists(),
        "valid input must reach the still-owned program: {:?}",
        manager.execution(id).unwrap().snapshot()
    );
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "VALID");
    manager.shutdown();
}
/// Polling drives actual process/service completion, bounded independently of guest execution fuel.
#[track_caller]
fn wait(manager: &mut Manager, ready: impl Fn(&Manager) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready(manager) && Instant::now() < deadline {
        manager.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(ready(manager), "expected service/process completion");
}

/// Started means a real program was acquired; output appears in its ordinary declared native panel.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn consumer_executes_argv_in_a_visible_terminal_session() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    install(&mut manager, &terminal());
    install(&mut manager, &fixture("execution-client", false, true));
    assert_eq!(open(&mut manager, "execution-client"), "Service opened");
    assert_eq!(
        execute(
            &mut manager,
            "execution-client",
            json!({
            "program":"powershell.exe", "args":["-NoProfile","-Command","[Console]::Write('SERVICE_ARGV_OK'); Start-Sleep -Seconds 60"],
                    "cwd":root.path(),"name":"构建输出"
                })
        ),
        "Accepted"
    );
    manager.poll();
    assert!(
        text(&manager, "execution-client").contains("started"),
        "{}",
        text(&manager, "execution-client")
    );
    let requests = manager
        .live
        .get_mut("terminal")
        .unwrap()
        .take_editor_requests();
    assert_eq!(requests.len(), 1);
    assert!(
        matches!(requests[0].operation(), api::EditorOperation::SetPanelVisibility { panel, visible:true } if panel == "terminal")
    );
    for request in requests {
        assert!(request.begin());
        request.finish(Ok(api::EditorValue::PanelVisibility {
            panel: "terminal".into(),
            visible: true,
        }));
    }
    wait(&mut manager, |manager| {
        output(manager).contains("SERVICE_ARGV_OK")
    });
    assert!(
        serde_json::to_string(
            &manager.live["terminal"]
                .views
                .values()
                .next()
                .map(|document| document.as_ref())
        )
        .unwrap()
        .contains("构建输出")
    );
    assert_eq!(manager.live["terminal"].process_count(), 2);
    // Source retirement must terminate delegated work while retaining the terminal's private shell.
    manager.disable("execution-client").unwrap();
    manager.poll();
    assert_eq!(manager.live["terminal"].process_count(), 1);
    let mut exited = false;
    manager.live["terminal"].views["terminal"]
        .as_ref()
        .root
        .visit(&mut |node| {
            if let Kind::SideTabs(tabs) = &node.kind {
                exited = tabs
                    .items
                    .iter()
                    .any(|tab| tab.label == "构建输出" && tab.status.as_deref() == Some("已退出"));
            }
        });
    assert!(
        exited,
        "source retirement must update the visible session lifecycle"
    );
}

/// Selection changes package ownership without changing consumer code or the native host's business logic.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn execution_contract_switches_to_an_independent_provider() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    install(&mut manager, &fixture("execution-client", false, true));
    assert!(open(&mut manager, "execution-client").contains("CapabilityUnavailable"));
    install(&mut manager, &terminal());
    assert_eq!(open(&mut manager, "execution-client"), "Service opened");
    install(&mut manager, &fixture("alternative-executor", true, true));
    assert!(open(&mut manager, "execution-client").contains("Conflict"));
    manager
        .set_service_provider(
            api::InstanceScope::Workspace,
            plugin_runtime::plugin_protocol::settings::Scope::User,
            CONTRACT,
            Some("alternative-executor"),
        )
        .unwrap();
    assert_eq!(open(&mut manager, "execution-client"), "Service opened");
    assert_eq!(
        execute(
            &mut manager,
            "execution-client",
            json!({
                "program":"powershell.exe", "args":["-NoProfile","-Command","Write-Output 'ALTERNATE_OK'; Start-Sleep -Seconds 60"],
                "name":"Alternate execution"
            })
        ),
        "Accepted"
    );
    manager.poll();
    assert!(
        text(&manager, "execution-client").contains("started"),
        "{}",
        text(&manager, "execution-client")
    );
    assert_eq!(manager.live["alternative-executor"].process_count(), 1);
    assert_eq!(manager.live["terminal"].process_count(), 1);
    assert!(text(&manager, "alternative-executor").contains("started"));
    let requests = manager
        .live
        .get_mut("alternative-executor")
        .unwrap()
        .take_editor_requests();
    assert!(
        matches!(requests[0].operation(), api::EditorOperation::SetPanelVisibility{panel,visible:true} if panel == "welcome")
    );
    manager.disable("execution-client").unwrap();
    manager.poll();
    assert_eq!(manager.live["alternative-executor"].process_count(), 0);
    assert!(
        text(&manager, "alternative-executor")
            .contains("Resource revoked: Err(Failure { code: InvalidHandle")
    );
    manager.enable("execution-client").unwrap();
    assert_eq!(open(&mut manager, "execution-client"), "Service opened");
    // Retiring a provider fails its outstanding request and cannot resurrect a former reference.
    execute(
        &mut manager,
        "execution-client",
        json!({"program":"cmd.exe","args":["/c","echo pending"]}),
    );
    manager.disable("alternative-executor").unwrap();
    manager.poll();
    assert!(text(&manager, "execution-client").contains("invalid_handle"));
    assert!(
        execute(
            &mut manager,
            "execution-client",
            json!({"program":"cmd.exe","args":[]})
        )
        .contains("InvalidHandle")
    );
}

/// Grants, cancellation and replacement constrain actual side effects rather than only method names.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn execution_authority_and_hot_update_never_replay_delegated_programs() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let package = terminal();
    install(&mut manager, &package);
    let consumer = fixture("execution-client", false, true);
    let mut denied = consumer.manifest.permissions.clone();
    denied.remove("process.exec");
    assert!(manager.install(&consumer, denied).is_err());
    assert!(!manager.live.contains_key("execution-client"));
    install(&mut manager, &consumer);
    install(&mut manager, &fixture("unprivileged-client", false, false));
    let args = json!({"program":"powershell.exe","args":["-NoProfile","-Command",
        "[IO.File]::AppendAllText('runs.txt', 'once'); [Console]::Write('STARTED_ONCE'); Start-Sleep -Seconds 60"],
        "cwd":root.path()});
    assert!(open(&mut manager, "unprivileged-client").contains("CapabilityUnavailable"));
    assert_eq!(open(&mut manager, "execution-client"), "Service opened");
    let mut invalid = args.clone();
    invalid["profile"] = json!(0);
    assert!(execute(&mut manager, "execution-client", invalid).contains("InvalidRequest"));
    assert!(
        execute(
            &mut manager,
            "execution-client",
            json!({"program":"cmd.exe", "args":vec!["x";129]})
        )
        .contains("InvalidRequest")
    );
    execute(&mut manager, "execution-client", args.clone());
    assert!(
        command(
            &mut manager,
            "execution-client",
            "service-cancel",
            Value::Null
        )
        .contains("NotExecuted")
    );
    manager.poll();
    assert_eq!(manager.live["terminal"].process_count(), 1);
    assert!(!root.path().join("runs.txt").exists());
    assert!(text(&manager, "execution-client").contains("not_executed"));

    execute(
        &mut manager,
        "execution-client",
        json!({"program":"does-not-exist-application.exe","args":[]}),
    );
    manager.poll();
    assert!(text(&manager, "execution-client").contains("operation_failed"));
    assert_eq!(manager.live["terminal"].process_count(), 1);
    execute(&mut manager, "execution-client", args.clone());
    wait(&mut manager, |manager| {
        output(manager).contains("STARTED_ONCE")
    });
    assert_eq!(
        std::fs::read_to_string(root.path().join("runs.txt")).unwrap(),
        "once"
    );
    // Completed creation cannot be cancelled retroactively and does not mean the native program exited.
    assert!(
        command(
            &mut manager,
            "execution-client",
            "service-cancel",
            Value::Null
        )
        .contains("InvalidHandle")
    );
    assert_eq!(manager.live["terminal"].process_count(), 2);
    execute(&mut manager, "execution-client", args);
    let mut manifest: Value = serde_json::from_slice(&package.files["manifest.json"]).unwrap();
    // Advance from the built package so this remains a real update after SDK releases.
    let mut candidate_version =
        semver::Version::parse(manifest["version"].as_str().unwrap()).unwrap();
    candidate_version.patch += 1;
    manifest["version"] = json!(candidate_version.to_string());
    install(&mut manager, &packages::archive(package.files, manifest));
    manager.poll();
    assert!(text(&manager, "execution-client").contains("invalid_handle"));
    assert_eq!(
        manager.live["terminal"].process_count(),
        1,
        "only the private shell may restart"
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("runs.txt")).unwrap(),
        "once"
    );
    assert!(output(&manager).contains("STARTED_ONCE"));
    assert!(
        execute(
            &mut manager,
            "execution-client",
            json!({"program":"cmd.exe","args":[]})
        )
        .contains("InvalidHandle")
    );
}

/// An independent consumer's session, started through the host, is the session the title bar sees.
///
/// This is the end-to-end form of ticket 08's third criterion, and it enters where a user does: a
/// real package built from the public SDK declares the host's session contract, opens it by name and
/// starts a program through it. The assertion is not about a second table — it is that
/// `Manager::executions`, which is what the title bar renders, knows the session the consumer
/// created, and that repeating the same launch locates that session instead of adding another.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn a_consumer_session_started_through_the_host_is_the_one_the_title_bar_sees() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    install(&mut manager, &terminal());
    install(&mut manager, &session_consumer("session-client"));
    let program = json!({
        "program": "powershell.exe",
        "args": ["-NoProfile", "-Command", "Start-Sleep -Seconds 60"],
        "name": "consumer session",
    });

    // The consumer opens the host's own contract by name and starts a program through it.
    let accepted = command(
        &mut manager,
        "session-client",
        "service-call-contract",
        json!({"contract":"session.host","method":"start","value":program}),
    );
    assert_eq!(accepted, "Accepted", "the consumer's call was accepted");
    let answered = answered_sessions(&mut manager);
    assert!(
        answered.contains("\"located\":false"),
        "the first start created a session: {answered}"
    );
    // The host's own table — what the title bar renders — has the session the consumer created.
    let sessions = manager.executions();
    assert_eq!(
        sessions.len(),
        1,
        "the consumer's session is in the host's table: {sessions:?}"
    );
    assert_eq!(sessions[0].snapshot().plugin, "terminal");
    let first = sessions[0].id();

    // The same launch, asked again by the consumer, locates that session instead of starting another.
    let accepted = command(
        &mut manager,
        "session-client",
        "service-call-contract",
        json!({"contract":"session.host","method":"start","value":program}),
    );
    assert_eq!(accepted, "Accepted");
    let answered = answered_sessions(&mut manager);
    assert!(
        answered.contains("\"located\":true"),
        "the repeat located the existing session: {answered}"
    );
    let sessions = manager.executions();
    assert_eq!(
        sessions.len(),
        1,
        "the consumer and the title bar are looking at one session, not two: {sessions:?}"
    );
    assert_eq!(sessions[0].id(), first);
    // It really is the host's session, not a name that happens to agree: the host can act on it, and
    // it is the provider's own session identity that makes it addressable.
    let session = manager.execution(first).unwrap();
    assert!(session.stoppable());
    assert!(
        session.snapshot().provider_session.is_some(),
        "the provider's own identity is what the host would address a stop to"
    );
    manager.stop_execution(first).unwrap();
}

/// A consumer can list, inspect and stop through the boundary — not only start.
///
/// Ticket 08's third criterion names four things a consumer does through the versioned session
/// operations: create a visible session, query it, locate and show it, and stop it. The check above
/// covers creating one and locating it on a repeat; this one covers the other two, so the criterion is
/// read rather than assumed to follow from `start`.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn a_consumer_queries_and_stops_its_session_through_the_host() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    install(&mut manager, &terminal());
    install(&mut manager, &session_consumer("session-client"));
    // A program that stays up, so there is a session to query and to stop.
    let program = json!({
        "program": "powershell.exe",
        "args": ["-NoProfile", "-Command", "Start-Sleep -Seconds 60"],
        "name": "consumer session",
    });

    let accepted = command(
        &mut manager,
        "session-client",
        "service-call-contract",
        json!({"contract":"session.host","method":"start","value":program}),
    );
    assert_eq!(accepted, "Accepted");
    let created = answered_sessions(&mut manager);
    assert!(created.contains("\"located\":false"), "{created}");
    let sessions = manager.executions();
    assert_eq!(sessions.len(), 1, "one session exists: {sessions:?}");
    let session = sessions[0].id();
    // Whether the provider has confirmed the program yet is not what this check is about, and this
    // harness does not drive a real PTY far enough to guarantee it. What matters is that the consumer
    // can address the session it created, which the two calls below do by naming its identity.

    // The consumer asks the host what it has, and reads back its own session. The wait is on the
    // session appearing in the consumer's own panel rather than on a word: a list answer carries no
    // `located` field, and waiting for one would time out on a call that in fact succeeded.
    let answered = command(
        &mut manager,
        "session-client",
        "service-call-contract",
        json!({"contract":"session.host","method":"list","value":{}}),
    );
    assert_eq!(answered, "Accepted");
    let wanted = session.to_string();
    let listed = |manager: &mut Manager| {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut text = String::new();
        while Instant::now() < deadline {
            manager.poll();
            text = text_of(manager, "session-client");
            if text.contains(&wanted) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        text
    };
    let listed = listed(&mut manager);
    assert!(
        listed.contains(&wanted),
        "the consumer's own session is in the host's list: {listed}"
    );

    // And it stops it through the same boundary, rather than the host reaching in on its behalf.
    let answered = command(
        &mut manager,
        "session-client",
        "service-call-contract",
        json!({"contract":"session.host","method":"stop","value":{"session":session.to_string()}}),
    );
    assert_eq!(answered, "Accepted");
    // The answer is the session's own identity and state, so the stop was addressed to it rather than
    // silently accepted; a refusal would have been reported as a failure instead of accepted.
    let stopped = answered_sessions(&mut manager);
    assert!(
        stopped.contains(&session.to_string()),
        "the stop answer names the session the consumer asked about: {stopped}"
    );
}

/// Read the consumer's own report of its session answer, waiting for one to arrive.
///
/// The answer is read from the consumer's panel because that is what it publishes, so the check sees
/// the same text a user would; the result is parsed from the SDK guest's serialized update.
fn answered_sessions(manager: &mut Manager) -> String {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut text = String::new();
    while Instant::now() < deadline {
        manager.poll();
        text = text_of(manager, "session-client");
        if text.contains("located") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    text
}

/// The panel text of one plugin, without asserting it is a status document.
fn text_of(manager: &Manager, id: &str) -> String {
    let Kind::Text { text } = &manager.live[id].views["welcome"].as_ref().root.kind else {
        return String::new();
    };
    text.clone()
}

/// A real SDK consumer cannot use the host gateway to borrow execution authority it never received.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn the_session_gateway_refuses_a_consumer_without_execution_permission() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    install(&mut manager, &terminal());
    let package = session_consumer("unprivileged-client");
    let mut manifest = serde_json::to_value(&package.manifest).unwrap();
    manifest["permissions"] = json!(["assets.read", "services.call", "ui.panels"]);
    // Admission already refuses a declared privileged method without its permission. This package
    // declares only reads, then attempts an undeclared start through the real guest call boundary.
    manifest["plugin_services"]["requires"]
        .as_object_mut()
        .unwrap()
        .remove(CONTRACT);
    let methods = manifest["plugin_services"]["requires"]["session.host"]["methods"]
        .as_object_mut()
        .unwrap();
    methods.remove("start");
    methods.remove("stop");
    methods.remove("input");
    install(&mut manager, &packages::archive(package.files, manifest));
    let before = manager.live["terminal"].process_count();
    let answer = command(
        &mut manager,
        "unprivileged-client",
        "service-call-contract",
        json!({
            "contract": "session.host", "method": "start", "value": {
                "program": "powershell.exe", "args": ["-NoProfile", "-Command", "Start-Sleep -Seconds 60"]
            }
        }),
    );
    assert!(answer.contains("UnsupportedOperation"), "{answer}");
    manager.poll();
    assert!(manager.executions().is_empty());
    assert_eq!(manager.live["terminal"].process_count(), before);
}

/// A consumer's retirement revokes the real program delegated through the host gateway.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn retiring_a_session_consumer_releases_its_program() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    install(&mut manager, &terminal());
    install(&mut manager, &session_consumer("session-client"));
    let before = manager.live["terminal"].process_count();
    let answer = command(
        &mut manager,
        "session-client",
        "service-call-contract",
        json!({
            "contract": "session.host", "method": "start", "value": {
                "program": "powershell.exe", "args": ["-NoProfile", "-Command", "Start-Sleep -Seconds 60"]
            }
        }),
    );
    assert_eq!(answer, "Accepted");
    wait(&mut manager, |manager| {
        manager
            .executions()
            .first()
            .is_some_and(|entry| entry.snapshot().state == ExecutionState::Running)
    });
    let session = manager.executions()[0].id();
    assert!(manager.live["terminal"].process_count() > before);
    manager.disable("session-client").unwrap();
    manager.poll();
    assert_eq!(manager.live["terminal"].process_count(), before);
    assert_eq!(
        manager.execution(session).unwrap().snapshot().state,
        ExecutionState::Failed
    );
}

/// Natural exits are observed even when a consumer uses only the public session gateway.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn public_sessions_observe_exits_and_release_capacity() {
    for provider in [terminal(), packages::provider("independent-executor")] {
        let root = tempfile::tempdir().unwrap();
        let mut manager = Manager::open(
            root.path().join("plugins"),
            Environment {
                workspace: root.path().display().to_string(),
                os: "windows".into(),
                ..Default::default()
            },
        )
        .unwrap();
        install(&mut manager, &provider);
        install(&mut manager, &session_consumer("session-client"));
        for iteration in 0..65 {
            let previous = manager
                .executions()
                .last()
                .map(|entry| entry.id())
                .unwrap_or(0);
            assert_eq!(
                command(
                    &mut manager,
                    "session-client",
                    "service-call-contract",
                    json!({
                        "contract": "session.host", "method": "start", "value": {
                            "program": "cmd.exe", "args": ["/d", "/c", "exit", "0"]
                        }
                    })
                ),
                "Accepted"
            );
            wait(&mut manager, |manager| {
                manager
                    .executions()
                    .last()
                    .is_some_and(|entry| entry.id() > previous)
            });
            let session = manager.executions().last().unwrap().id();
            // This deliberately does not use the native query_execution API to drive lifecycle.
            let deadline = Instant::now() + Duration::from_secs(30);
            while manager.execution(session).unwrap().snapshot().state != ExecutionState::Exited
                && Instant::now() < deadline
            {
                manager.poll();
                std::thread::sleep(Duration::from_millis(20));
            }
            assert_eq!(
                manager.execution(session).unwrap().snapshot().state,
                ExecutionState::Exited,
                "iteration {iteration}: {:?}; consumer: {}",
                manager.execution(session).unwrap().snapshot(),
                text_of(&manager, "session-client")
            );
            // This runtime test has no window event loop. Complete ordinary panel requests through
            // their public host boundary, exactly as native UI integration does, instead of leaving
            // 32 unanswered display requests to exhaust a different resource budget.
            for instance in manager.live.values_mut() {
                for request in instance.take_editor_requests() {
                    let api::EditorOperation::SetPanelVisibility { panel, visible } =
                        request.operation().clone()
                    else {
                        panic!("unexpected editor operation in the execution fixture");
                    };
                    if request.begin() {
                        request.finish(Ok(api::EditorValue::PanelVisibility { panel, visible }));
                    }
                }
            }
            assert_eq!(
                command(
                    &mut manager,
                    "session-client",
                    "service-call-contract",
                    json!({
                        "contract": "session.host", "method": "status", "value": {"session":session.to_string()}
                    })
                ),
                "Accepted"
            );
            wait(&mut manager, |manager| {
                text_of(manager, "session-client").contains("\"state\":\"exited\"")
            });
            // A user need not manually close finished views to regain capacity. Both independent
            // providers evict completed history while preserving every active program.
        }
        assert_eq!(manager.executions().len(), 64);
        manager.shutdown();
    }
}

/// Hiding a managed tab preserves its program and its public identity until explicitly stopped.
#[test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn closing_a_managed_terminal_tab_keeps_its_session_and_locate_restores_it() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    install(&mut manager, &terminal());
    install(&mut manager, &session_consumer("session-client"));
    let start = json!({
        "contract": "session.host", "method": "start", "value": {
            "program": "powershell.exe", "args": ["-NoProfile", "-Command", "Start-Sleep -Seconds 60"]
        }
    });
    assert_eq!(
        command(
            &mut manager,
            "session-client",
            "service-call-contract",
            start.clone()
        ),
        "Accepted"
    );
    wait(&mut manager, |manager| {
        manager
            .executions()
            .last()
            .is_some_and(|entry| entry.snapshot().state == ExecutionState::Running)
    });
    let session = manager.executions().last().unwrap().id();
    manager
        .invoke_command("terminal", "terminal.close", Value::Null)
        .unwrap();
    manager.poll();
    assert_eq!(
        manager.execution(session).unwrap().state(),
        ExecutionState::Running
    );
    let receipt = manager
        .execution(session)
        .unwrap()
        .snapshot()
        .provider_session
        .unwrap();
    let selected = |manager: &Manager| {
        let mut selected = None;
        manager.live["terminal"].views["terminal"]
            .root
            .visit(&mut |node| {
                if let Kind::SideTabs(tabs) = &node.kind {
                    selected = tabs.selected.clone();
                }
            });
        selected
    };
    assert_ne!(selected(&manager).as_deref(), Some(receipt.as_str()));
    let location = manager.locate_execution(session).unwrap();
    manager.poll_request(&location);
    assert!(matches!(
        location.status(),
        api::RequestUpdate::Completed { result: Ok(_) }
    ));
    assert_eq!(selected(&manager).as_deref(), Some(receipt.as_str()));
    // Repeating the same configuration locates the retained program rather than creating a duplicate.
    assert_eq!(
        command(
            &mut manager,
            "session-client",
            "service-call-contract",
            start
        ),
        "Accepted"
    );
    manager.poll();
    assert_eq!(manager.executions().len(), 1);
    assert_eq!(manager.executions()[0].id(), session);
    manager.shutdown();
}
/// Public guest bytes cannot forge host authority, and subscription quotas reclaim only their owner.
#[test]
#[ignore = "build terminal and capability-example through the current public SDK first"]
fn forged_host_handles_and_bounded_subscriptions_are_refused_through_the_public_sdk() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    install(&mut manager, &terminal());
    let clients = (0..5)
        .map(|index| format!("bounded-client-{index}"))
        .collect::<Vec<_>>();
    for client in &clients {
        install(&mut manager, &session_consumer(client));
    }
    let before = manager.live["terminal"].process_count();
    let forged = command(
        &mut manager,
        &clients[0],
        "service-forge-host-reference",
        json!({}),
    );
    assert!(
        forged.contains("InvalidHandle"),
        "public handle forgery must be rejected: {forged}"
    );
    assert_eq!(manager.live["terminal"].process_count(), before);
    assert!(manager.executions().is_empty());
    let mut sessions = Vec::new();
    let mut subscriptions = Vec::new();
    for client in &clients {
        let receipt = session_call(
            &mut manager,
            client,
            "start",
            json!({"program":"powershell.exe","args":["-NoProfile","-Command","Start-Sleep -Seconds 60"],"configuration":client}),
        );
        sessions.push(receipt["session"].as_str().unwrap().to_owned());
    }
    for (index, client) in clients.iter().take(4).enumerate() {
        for _ in 0..32 {
            let receipt = session_call(
                &mut manager,
                client,
                "subscribe",
                json!({"session":sessions[index]}),
            );
            subscriptions.push((
                client.clone(),
                receipt["subscription"].as_str().unwrap().to_owned(),
            ));
        }
        let failure = session_result(
            &mut manager,
            client,
            "subscribe",
            json!({"session":sessions[index]}),
        )
        .unwrap_err();
        assert_eq!(
            failure.code,
            api::ErrorCode::LimitExceeded,
            "per-origin quota"
        );
    }
    assert_eq!(
        session_result(
            &mut manager,
            &clients[4],
            "subscribe",
            json!({"session":sessions[4]})
        )
        .unwrap_err()
        .code,
        api::ErrorCode::LimitExceeded,
        "host-wide quota"
    );
    let (owner, subscription) = &subscriptions[0];
    assert_eq!(
        session_result(
            &mut manager,
            &clients[4],
            "next",
            json!({"subscription":subscription,"limit":1})
        )
        .unwrap_err()
        .code,
        api::ErrorCode::InvalidHandle
    );
    session_call(
        &mut manager,
        owner,
        "unsubscribe",
        json!({"subscription":subscription}),
    );
    let replacement = session_call(
        &mut manager,
        &clients[4],
        "subscribe",
        json!({"session":sessions[4]}),
    );
    assert_ne!(
        replacement["subscription"].as_str(),
        Some(subscription.as_str()),
        "retired identity never regains authority"
    );
    assert_eq!(
        session_result(
            &mut manager,
            owner,
            "next",
            json!({"subscription":subscription,"limit":1})
        )
        .unwrap_err()
        .code,
        api::ErrorCode::InvalidHandle
    );
    manager.shutdown();
}
