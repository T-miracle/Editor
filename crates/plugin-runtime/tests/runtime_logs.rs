//! Real SDK packages exercise both WASI output streams, fault attribution and failed-candidate retention.
use plugin_runtime::{InstallControl, LogLevel, Manager, Package, plugin_protocol::Environment};
use serde_json::{Value, json};
use std::{
    io::{Cursor, Write},
    sync::Arc,
};

#[path = "support/service_packages.rs"]
mod service_packages;

/// Produce a current-contract ZIP with no required WIT exports, so preparation fails before cutover.
fn incompatible_candidate(package: &Package) -> Package {
    let mut files = package.files.clone();
    let mut manifest: Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["version"] = json!("99.0.0");
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    files.insert(
        package.manifest.component.clone().unwrap(),
        b"\0asm\x0d\0\x01\0".to_vec(),
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

/// Actual guest panic output and host trap diagnostics stay attributed after the origin is disabled.
#[test]
#[ignore = "build capability-example through the public SDK first"]
fn real_wasm_output_and_faults_survive_instance_retirement() {
    let directory = tempfile::tempdir().unwrap();
    let mut manager =
        Manager::open(directory.path().join("plugins"), Environment::default()).unwrap();
    for package in [
        service_packages::package("log-provider", true, false, "1.0.0"),
        service_packages::package("log-consumer", false, false, "^1"),
    ] {
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
    }
    let logs = manager.runtime_logs();
    manager
        .invoke_command("log-consumer", "service-open", json!("example.echo"))
        .unwrap();
    manager
        .invoke_command(
            "log-consumer",
            "service-call",
            json!({"method":"trap", "value":"panic", "timeout_ms":1000}),
        )
        .unwrap();
    manager.poll();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        let records = logs.records("log-provider");
        // The two output readers may publish in either order; wait for both real guest sources.
        if records.iter().any(|record| {
            record.source == "wasi/stdout" && record.message == "service fixture stdout before trap"
        }) && records.iter().any(|record| {
            record.source == "wasi/stderr" && record.message.contains("service fixture trap")
        }) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let records = logs.records("log-provider");
    assert!(records.iter().any(|record| {
        record.plugin == "log-provider"
            && record.level == LogLevel::Info
            && record.source == "wasi/stdout"
            && record.message == "service fixture stdout before trap"
    }));
    assert!(
        records
            .iter()
            .any(|record| record.level == LogLevel::Warning
                && record.source == "wasi/stderr"
                && record.message.contains("service fixture trap"))
    );
    assert!(
        records
            .iter()
            .any(|record| record.level == LogLevel::Error && record.source.starts_with("wasm/"))
    );
    assert_eq!(logs.unread_severity("log-provider"), Some(LogLevel::Error));
    assert!(
        logs.records("log-consumer")
            .iter()
            .all(|record| record.level != LogLevel::Error)
    );
    manager.disable("log-provider").unwrap();
    assert!(
        logs.records("log-provider")
            .iter()
            .any(|record| record.level == LogLevel::Error)
    );
}

/// A failed actual component candidate keeps the active version and records into the manager's shared sink.
#[test]
#[ignore = "build capability-example through the public SDK first"]
fn failed_preparation_is_visible_without_replacing_the_live_instance() {
    let directory = tempfile::tempdir().unwrap();
    let mut manager =
        Manager::open(directory.path().join("plugins"), Environment::default()).unwrap();
    let original = service_packages::package("log-provider", true, false, "1.0.0");
    manager
        .install(&original, original.manifest.permissions.clone())
        .unwrap();
    let active = manager.live["log-provider"].views["welcome"].clone();
    let candidate = incompatible_candidate(&original);
    assert!(
        manager
            .prepare_installation(
                &candidate,
                candidate.manifest.permissions.clone(),
                &InstallControl::default()
            )
            .is_err()
    );
    assert!(Arc::ptr_eq(
        &active,
        &manager.live["log-provider"].views["welcome"]
    ));
    assert_eq!(manager.installed["log-provider"].digest, original.digest);
    assert!(
        manager
            .runtime_logs()
            .records("log-provider")
            .iter()
            .any(|record| record.level == LogLevel::Error && record.source.starts_with("wasm/"))
    );
}
