//! Real permission-checked services verify runtime logs independently of document diagnostics.
//! Each fixture owns a temporary installation and reuses the existing native stdio server.

use super::*;
use plugin_runtime::{LogLevel, LogRecord, Manager, Package, RuntimeLogs};
use std::{collections::BTreeMap, io::Write};

const OWNER: &str = "observed-lsp";
const SERVICE: &str = "observed-lsp/analysis";

/// A resource-only ZIP declares a standard service without borrowing any guest or host source.
fn fixture_package(root: &Path, crash_marker: Option<&Path>) -> Package {
    fixture_package_with_readiness(
        root,
        crash_marker,
        json!({
            "notification": "fixture/status", "pointer": "/state", "expected": "ready", "timeout_ms": 5000
        }),
    )
}

/// Vary only the public readiness declaration; the native executable and permission path stay real.
fn fixture_package_with_readiness(
    root: &Path,
    crash_marker: Option<&Path>,
    readiness: Value,
) -> Package {
    // Resolve beside this test binary so an isolated Cargo target cannot launch a stale shared fixture.
    let executable = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples/lsp_fixture.exe")
        .canonicalize()
        .expect("build plugin-runtime --example lsp_fixture first");
    let mut arguments = vec![root.join("wire.jsonl").display().to_string()];
    if let Some(marker) = crash_marker {
        arguments.extend([
            "--crash-marker".into(),
            marker.display().to_string(),
            "--crash-stderr".into(),
            "native startup crash: 崩溃尾部".into(),
        ]);
    }
    let manifest = json!({
        "id": OWNER, "name": "Observed Language Service", "version": "1.0.0",
        "protocol": 7, "api": {"base": "^1", "required": {"process": "^1", "language.lsp": "^1"}},
        "contributions": "plugin.toml", "storage_limit": 1024,
        "permissions": ["process.service.analysis"],
        "services": {"analysis": {"program": executable, "args": arguments}},
        "language_servers": [{"id": "analysis", "language": "unfamiliar", "service": "analysis",
            "readiness": readiness}]
    });
    let declaration = format!(
        "[plugin]\nid = \"{OWNER}\"\nname = \"Observed Language Service\"\nversion = \"1.0.0\"\nhost_version = \">=0.1.0\"\n"
    );
    let files = BTreeMap::from([
        (
            "manifest.json".into(),
            serde_json::to_vec(&manifest).unwrap(),
        ),
        ("plugin.toml".into(), declaration.into_bytes()),
    ]);
    resource_package(files)
}

/// Alternate public declarations reuse the same ZIP path without widening another module's visibility.
fn resource_package(files: BTreeMap<String, Vec<u8>>) -> Package {
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (path, bytes) in files {
        archive
            .start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&archive.finish().unwrap().into_inner()).unwrap()
}

/// Installation and service preparation use the public runtime authority and the real local executable.
fn installed_fixture(root: &Path, crash_marker: Option<&Path>) -> (Manager, LanguageServer) {
    let package = fixture_package(root, crash_marker);
    installed_package(root, &package)
}

/// Alternate declarations still enter through the actual public package manager and shared log sink.
fn installed_package(root: &Path, package: &Package) -> (Manager, LanguageServer) {
    let mut manager = Manager::open(
        root.join("installed"),
        plugin_runtime::plugin_protocol::Environment {
            workspace: root.display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(package, package.manifest.permissions.clone())
        .unwrap();
    let service = manager.language_services()[SERVICE]
        .as_ref()
        .unwrap()
        .clone();
    (manager, LanguageServer::from_service(service).unwrap())
}

/// Synchronize with host receipt without draining the request queue or relying on scheduler sleeps.
fn wait_for_log(logs: &RuntimeLogs, message: &str) -> Vec<LogRecord> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let records = logs.records(OWNER);
        if records.iter().any(|record| record.message == message) {
            return records;
        }
        assert!(
            Instant::now() < deadline,
            "missing runtime message: {message}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

// Notification admission and readiness have their own real-service regressions.
mod completion_hooks;
mod notifications;
mod snapshots;

/// More log pushes than the document queue can hold must still arrive while no document is open.
#[test]
#[ignore = "build cargo build -p plugin-runtime --example lsp_fixture first"]
fn native_lsp_logs_preserve_levels_and_leave_document_diagnostics_separate() {
    let directory = tempfile::tempdir().unwrap();
    let (_manager, server) = installed_fixture(directory.path(), None);
    let logs = server.service.runtime_logs();
    server.prepare_until_ready().unwrap();
    assert_eq!(logs.unread_severity(OWNER), None);
    let mut notifications = vec![
        json!({"jsonrpc":"2.0", "method":"window/logMessage", "params":{"type":1,"message":"runtime-error"}}),
        json!({"jsonrpc":"2.0", "method":"window/showMessage", "params":{"type":2,"message":"runtime-warning"}}),
        json!({"jsonrpc":"2.0", "method":"window/logMessage", "params":{"type":3,"message":"runtime-info"}}),
        json!({"jsonrpc":"2.0", "method":"window/logMessage", "params":{"type":4,"message":"runtime-trace"}}),
        json!({"jsonrpc":"2.0", "method":"textDocument/publishDiagnostics", "params":{
            "uri":file_uri(&directory.path().join("closed.unfamiliar")).unwrap(), "version":1,
            "diagnostics":[{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}},
                "severity":1,"message":"document-only-error"}]}}),
    ];
    notifications.extend((0..300).map(|index| {
        json!({"jsonrpc":"2.0", "method":"window/logMessage", "params":{
            "type":3,"message":format!("idle-log-{index}")}})
    }));
    server
        .connection
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .notify(
            "fixture/notifications",
            json!({"notifications":notifications}),
        )
        .unwrap();
    // No request or document poll runs here: the pipe reader itself owns log notification delivery.
    let records = wait_for_log(&logs, "idle-log-299");
    for (message, expected) in [
        ("runtime-error", LogLevel::Error),
        ("runtime-warning", LogLevel::Warning),
        ("runtime-info", LogLevel::Info),
        ("runtime-trace", LogLevel::Info),
    ] {
        let record = records
            .iter()
            .find(|record| record.message == message)
            .unwrap();
        assert_eq!(record.level, expected);
        assert_eq!(record.plugin, OWNER);
        assert_eq!(record.source, "lsp/analysis");
    }
    assert!(
        records
            .iter()
            .all(|record| record.message != "document-only-error")
    );
    assert_eq!(logs.unread_severity(OWNER), Some(LogLevel::Error));
    let mut connection = server.connection.lock().unwrap();
    connection
        .as_mut()
        .unwrap()
        .request("fixture/barrier", json!({}))
        .unwrap();
    assert!(
        std::fs::read_to_string(directory.path().join("wire.jsonl"))
            .unwrap()
            .contains("fixture/barrier")
    );
}

/// Deliberate retirement stays informational, preserves a peer's alert, and cannot revive old adapters.
#[test]
#[ignore = "build cargo build -p plugin-runtime --example lsp_fixture first"]
fn native_lsp_retirement_cannot_alert_a_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let (mut manager, server) = installed_fixture(directory.path(), None);
    let logs = server.service.runtime_logs();
    server.prepare_until_ready().unwrap();
    logs.append("peer-service", LogLevel::Error, "fixture", "peer-error");
    let running_lifecycle = logs
        .records(OWNER)
        .iter()
        .filter(|record| record.source == "lsp/analysis")
        .count();
    server.retire();
    assert_eq!(
        logs.records(OWNER)
            .iter()
            .filter(|record| record.source == "lsp/analysis")
            .count(),
        running_lifecycle + 1,
        "a controlled stop remains visible as one lifecycle record"
    );
    let retired_boundary = logs.records(OWNER).last().unwrap().id;
    assert!(server.prepare().is_err());
    assert_eq!(logs.unread_severity(OWNER), None);
    let replacement = LanguageServer::from_service(server.service.clone()).unwrap();
    replacement.prepare_until_ready().unwrap();
    assert!(server.prepare_until_ready().is_err());
    manager.disable(OWNER).unwrap();
    assert!(replacement.prepare().is_err());
    replacement.retire();
    assert!(
        logs.records(OWNER)
            .iter()
            .filter(|record| record.id > retired_boundary)
            .all(|record| record.level == LogLevel::Info),
        "controlled shutdown cannot turn pipe EOF into a runtime alert"
    );
    assert_eq!(logs.unread_severity("peer-service"), Some(LogLevel::Error));
    assert!(
        logs.records(OWNER)
            .iter()
            .filter(|record| record.source == "lsp/analysis")
            .all(|record| record.level == LogLevel::Info)
    );
}

/// Actual initialization failures emit bounded retry transitions; backoff rejection adds no extra attempt.
#[test]
#[ignore = "build cargo build -p plugin-runtime --example lsp_fixture first"]
fn native_lsp_retry_logs_stop_at_the_failure_budget() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("crash");
    std::fs::write(&marker, "crash on initialize").unwrap();
    let (_manager, server) = installed_fixture(directory.path(), Some(&marker));
    let logs = server.service.runtime_logs();
    for (now, expected) in [
        (Duration::ZERO, RecoveryState::WaitingRetry),
        (Duration::from_secs(2), RecoveryState::WaitingRetry),
        (Duration::from_secs(8), RecoveryState::Paused),
    ] {
        assert!(server.prepare_at(now).is_err());
        assert_eq!(server.recovery_state(), Some(expected));
        let before = logs.records(OWNER);
        assert!(server.prepare_at(now).is_err());
        assert_eq!(logs.records(OWNER), before);
    }
    let attempts: Vec<_> = logs
        .records(OWNER)
        .into_iter()
        .filter(|record| record.source == "lsp/analysis/initialize")
        .collect();
    assert_eq!(attempts.len(), 3);
    assert_eq!(
        attempts
            .iter()
            .map(|record| record.level)
            .collect::<Vec<_>>(),
        vec![LogLevel::Warning, LogLevel::Warning, LogLevel::Error]
    );
    let wire = std::fs::read_to_string(directory.path().join("wire.jsonl")).unwrap();
    assert_eq!(
        wire.lines()
            .filter(|line| line.contains("\"method\":\"initialize\""))
            .count(),
        3
    );
}

/// A healthy reconnection records recovery once; historical failure levels are retained after reading.
#[test]
#[ignore = "build cargo build -p plugin-runtime --example lsp_fixture first"]
fn native_lsp_recovery_records_the_transition_without_clearing_history() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("crash");
    std::fs::write(&marker, "crash on initialize").unwrap();
    let (_manager, server) = installed_fixture(directory.path(), Some(&marker));
    let logs = server.service.runtime_logs();
    assert!(server.prepare_at(Duration::ZERO).is_err());
    let failed = logs.records(OWNER);
    let viewed = failed.last().unwrap().id;
    logs.view_through(OWNER, viewed);
    std::fs::remove_file(marker).unwrap();
    server.prepare_at(Duration::from_secs(2)).unwrap();
    assert_eq!(server.recovery_state(), Some(RecoveryState::Recovered));
    let recovered = logs.records(OWNER);
    let transition: Vec<_> = recovered
        .iter()
        .filter(|record| record.source == "lsp/analysis/initialize" && record.id > viewed)
        .collect();
    assert_eq!(transition.len(), 1);
    assert_eq!(transition[0].level, LogLevel::Info);
    assert_eq!(logs.unread_severity(OWNER), None);
    for _ in 0..3 {
        server.prepare_at(Duration::from_secs(3)).unwrap();
    }
    assert_eq!(logs.records(OWNER), recovered);
    assert!(
        failed
            .iter()
            .all(|record| logs.records(OWNER).contains(record))
    );
}

/// Fault teardown drains the actual service's unterminated stderr before retiring its output reader.
#[test]
#[ignore = "build cargo build -p plugin-runtime --example lsp_fixture first"]
fn native_lsp_failure_tail_survives_startup_and_running_protocol_faults() {
    let startup = tempfile::tempdir().unwrap();
    let marker = startup.path().join("crash");
    std::fs::write(&marker, "crash during initialize").unwrap();
    let (_manager, server) = installed_fixture(startup.path(), Some(&marker));
    let logs = server.service.runtime_logs();
    assert!(server.prepare_at(Duration::ZERO).is_err());
    let startup_records = wait_for_log(&logs, "native startup crash: 崩溃尾部");
    assert!(startup_records.iter().any(|record| {
        record.message == "native startup crash: 崩溃尾部"
            && record.source == "lsp/analysis/stderr"
            && record.level == LogLevel::Warning
    }));

    let running = tempfile::tempdir().unwrap();
    let (_manager, server) = installed_fixture(running.path(), None);
    server.prepare_until_ready().unwrap();
    let logs = server.service.runtime_logs();
    let result = {
        let mut connection = server.connection.lock().unwrap();
        let result = connection.as_mut().unwrap().request(
            "fixture/fail-with-stderr",
            json!({"message":"native protocol fault: 运行尾部"}),
        );
        server.connection_failed("fixture/protocol-fault", &result, &mut connection);
        result
    };
    assert!(result.is_err());
    let records = wait_for_log(&logs, "native protocol fault: 运行尾部");
    assert!(records.iter().any(|record| {
        record.message == "native protocol fault: 运行尾部"
            && record.source == "lsp/analysis/stderr"
            && record.level == LogLevel::Warning
    }));
    assert_eq!(logs.unread_severity(OWNER), Some(LogLevel::Error));
}

/// A controlled stop discards an old pending line instead of raising an alert after replacement starts.
#[test]
#[ignore = "build cargo build -p plugin-runtime --example lsp_fixture first"]
fn native_lsp_controlled_stop_cannot_publish_a_pending_stderr_tail() {
    let directory = tempfile::tempdir().unwrap();
    let (_manager, server) = installed_fixture(directory.path(), None);
    server.prepare_until_ready().unwrap();
    let logs = server.service.runtime_logs();
    server
        .connection
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .request("fixture/stderr", json!({"message":"retired pending tail"}))
        .unwrap();
    server.retire();
    let replacement = LanguageServer::from_service(server.service.clone()).unwrap();
    replacement.prepare_until_ready().unwrap();
    // A new handshake is observable and old readers may drain, but their prior cutoff is irreversible.
    assert!(logs.records(OWNER).iter().all(|record| {
        record.message != "retired pending tail" && record.level == LogLevel::Info
    }));
    assert_eq!(logs.unread_severity(OWNER), None);
}
