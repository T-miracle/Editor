//! Real PTYs are requested by an independent SDK consumer across the public service boundary.
#![cfg(windows)]
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
