//! Real SDK guests exercise bounded faults and manual recovery through the public package manager.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, ui::Kind},
};
use serde_json::json;
#[path = "support/service_packages.rs"]
mod service_packages;

/// Recover one faulting instance without restarting the manager or disturbing a healthy installed peer.
#[test]
#[ignore = "build the SDK capability-example first"]
fn wasm_fault_is_scoped_visible_and_independently_restartable() {
    let directory = tempfile::tempdir().unwrap();
    let package = Package::read(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
    )
    .unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::write(workspace.join("source.txt"), "workspace").unwrap();
    let root = directory.path().join("plugins");
    let mut manager = Manager::open(
        root.clone(),
        Environment {
            workspace: workspace.display().to_string(),
            ..Environment::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    manager
        .invoke_command(
            "capability-example",
            "scope-write",
            json!({"text":"recoverable"}),
        )
        .unwrap();
    let peer = service_packages::package("healthy-peer", true, true, "1.0.0");
    manager
        .install(&peer, peer.manifest.permissions.clone())
        .unwrap();
    let healthy_view = manager.live["healthy-peer"].views["welcome"].clone();
    manager
        .invoke_command("capability-example", "active-directory", json!(null))
        .unwrap();
    let pending = manager
        .live
        .get_mut("capability-example")
        .unwrap()
        .take_editor_requests()
        .pop()
        .unwrap();
    assert!(pending.begin());
    let started = std::time::Instant::now();
    assert!(
        manager
            .invoke_command("capability-example", "fault-spin", json!(null))
            .is_err()
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    let report = manager.diagnostics("capability-example");
    assert!(
        report
            .iter()
            .any(|report| report.operation.contains("fault-spin")
                && report.message.contains("budget"))
    );
    assert!(manager.live["capability-example"].views.is_empty());
    assert!(!pending.enter_side_effect());
    assert!(std::sync::Arc::ptr_eq(
        &healthy_view,
        manager.live["healthy-peer"].views.values().next().unwrap()
    ));
    assert_eq!(manager.live["capability-example"].resource_count(), 0);
    manager.restart_plugin("capability-example").unwrap();
    assert!(
        manager
            .invoke_command("capability-example", "fault-memory", json!(null))
            .is_err()
    );
    assert!(
        manager
            .diagnostics("capability-example")
            .iter()
            .any(|report| report.operation.contains("fault-memory"))
    );
    manager.restart_plugin("capability-example").unwrap();
    manager
        .invoke_command("capability-example", "active-directory", json!(null))
        .unwrap();
    let document = manager.live["capability-example"]
        .views
        .values()
        .next()
        .unwrap()
        .as_ref();
    let Kind::Text { text } = &document.root.kind else {
        panic!("native text expected")
    };
    assert!(text.contains("Accepted"));
    // A missing component is a real restart failure, never a reason to discard the committed data.
    let component = root
        .join("packages")
        .join("capability-example")
        .join(&manager.installed["capability-example"].digest)
        .join("capability-example.wasm");
    let bytes = std::fs::read(&component).unwrap();
    std::fs::remove_file(&component).unwrap();
    assert!(manager.restart_plugin("capability-example").is_err());
    assert!(
        manager.installed["capability-example"]
            .error
            .as_ref()
            .unwrap()
            .contains("Restart failed")
    );
    assert_eq!(
        std::fs::read_to_string(
            manager
                .data_directory("capability-example")
                .join("value.txt")
        )
        .unwrap(),
        "recoverable"
    );
    std::fs::write(&component, bytes).unwrap();
    manager.restart_plugin("capability-example").unwrap();
    manager
        .invoke_command("capability-example", "scope-read", json!(null))
        .unwrap();
    let Kind::Text { text } = &manager.live["capability-example"]
        .views
        .values()
        .next()
        .unwrap()
        .as_ref()
        .root
        .kind
    else {
        panic!("recovered text expected")
    };
    assert_eq!(text, "workspace|recoverable");
}
