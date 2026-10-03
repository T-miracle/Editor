//! Real guest requests cross the same asynchronous host boundary used by the editor worker.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, api, ui::Kind},
};
use serde_json::json;

/// Observe the independently compiled guest's native output, without reading its internal state.
fn text(manager: &Manager, id: &str) -> String {
    let Kind::Text { text } = &manager.live[id]
        .views
        .values()
        .next()
        .unwrap()
        .as_ref()
        .root
        .kind
    else {
        panic!("native text expected")
    };
    text.clone()
}

/// Acceptance must precede execution and a correlated final result must return to the guest.
#[test]
#[ignore = "build with scripts/build-capability-example.ps1 first"]
fn editor_request_is_accepted_then_completed_through_the_host() {
    let root = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let package = Package::read(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
    )
    .unwrap();
    let id = &package.manifest.id;
    let mut manager = Manager::open(
        root.path().into(),
        Environment {
            workspace: workspace.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    manager
        .invoke_command(
            id,
            "scope-probe",
            json!({"method":"editor", "operation":{"kind":"active_directory"}, "timeout_ms":30000}),
        )
        .unwrap();
    let accepted: Result<api::Value, api::Failure> =
        serde_json::from_str(&text(&manager, id)).unwrap();
    let api::Value::Accepted(handle) = accepted.unwrap() else {
        panic!("acceptance expected")
    };
    let request = manager
        .live
        .get_mut(id)
        .unwrap()
        .take_editor_requests()
        .pop()
        .unwrap();
    assert_eq!(request.handle(), &handle);
    assert!(request.begin());
    manager.poll();
    assert!(text(&manager, id).contains("Progress"));
    request.finish(Ok(api::EditorValue::Directory { path: "src".into() }));
    manager.poll();
    assert!(text(&manager, id).contains("src"));
    assert!(text(&manager, id).contains("Completed"));
    // Replacing a view intent rejects an older completion even when that request was not cancelled.
    for _ in 0..2 {
        manager
            .invoke_command(id, "active-directory", json!(null))
            .unwrap();
    }
    let requests = manager.live.get_mut(id).unwrap().take_editor_requests();
    for request in &requests {
        assert!(request.begin());
    }
    requests[1].finish(Ok(api::EditorValue::Directory { path: "NEW".into() }));
    manager.poll();
    requests[0].finish(Ok(api::EditorValue::Directory { path: "OLD".into() }));
    manager.poll();
    assert!(text(&manager, id).contains("NEW"));
    assert!(!text(&manager, id).contains("OLD"));
    // Panel identity is local to the requesting package, never a cross-plugin target string.
    manager.invoke_command(id, "scope-probe", json!({"method":"editor", "operation":{"kind":"set_panel_visibility","panel":"other/welcome","visible":false},"timeout_ms":30000})).unwrap();
    assert!(text(&manager, id).contains("permission_denied"));
    for _ in 0..32 {
        manager
            .invoke_command(id, "active-directory", json!(null))
            .unwrap();
    }
    manager
        .invoke_command(id, "active-directory", json!(null))
        .unwrap();
    assert!(text(&manager, id).contains("LimitExceeded"));
    let failure = text(&manager, id);
    for pending in manager.live.get_mut(id).unwrap().take_editor_requests() {
        pending.finish(Ok(api::EditorValue::Directory {
            path: "obsolete".into(),
        }));
    }
    manager.poll();
    assert_eq!(
        text(&manager, id),
        failure,
        "older work cannot overwrite the latest rejected intent"
    );
    manager.disable(id).unwrap();
    assert_eq!(manager.resource_count(), 0);
}

/// Cancellation, deadlines and retirement seal queued callbacks before they can execute.
#[test]
#[ignore = "build with scripts/build-capability-example.ps1 first"]
fn cancelled_expired_and_retired_requests_cannot_publish_late_results() {
    let root = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let package = Package::read(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
    )
    .unwrap();
    let id = &package.manifest.id;
    let mut manager = Manager::open(
        root.path().into(),
        Environment {
            workspace: workspace.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    for mode in ["cancel", "timeout", "retire", "running"] {
        if mode == "running" {
            manager.enable(id).unwrap();
        }
        manager.invoke_command(id, "scope-probe", json!({"method":"editor", "operation":{"kind":"active_directory"}, "timeout_ms":30000})).unwrap();
        let request = manager
            .live
            .get_mut(id)
            .unwrap()
            .take_editor_requests()
            .pop()
            .unwrap();
        match mode {
            "cancel" | "running" => {
                if mode == "running" {
                    assert!(request.begin());
                    assert!(request.enter_side_effect());
                }
                manager.invoke_command(id, "scope-probe", json!({"method":"cancel_request", "handle":request.handle(), "mode":"try_terminate"})).unwrap();
                assert!(text(&manager, id).contains(if mode == "running" {
                    "waiting_stopped"
                } else {
                    "not_executed"
                }));
            }
            "timeout" => {
                request.expire(std::time::Instant::now() + std::time::Duration::from_secs(31))
            }
            _ => manager.disable(id).unwrap(),
        }
        assert!(!request.begin());
        request.finish(Ok(api::EditorValue::Directory {
            path: "LATE".into(),
        }));
        assert!(!format!("{:?}", request.status()).contains("LATE"));
        if mode != "retire" {
            manager.poll();
            assert!(text(&manager, id).contains("Cancelled"));
        }
    }
}

/// Document notifications coalesce by identity, retain the newest revision and fail explicitly on overflow.
#[test]
#[ignore = "build with scripts/build-capability-example.ps1 first"]
fn document_subscriptions_are_bounded_versioned_and_disposable() {
    let root = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let package = Package::read(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
    )
    .unwrap();
    let id = &package.manifest.id;
    let mut manager = Manager::open(
        root.path().into(),
        Environment {
            workspace: workspace.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    manager
        .invoke_command(id, "scope-probe", json!({"method":"subscribe_documents"}))
        .unwrap();
    let result: Result<api::Value, api::Failure> =
        serde_json::from_str(&text(&manager, id)).unwrap();
    let api::Value::Resource(subscription) = result.unwrap() else {
        panic!("subscription expected")
    };
    let change = |id: &str, revision| api::DocumentChange {
        document: api::DocumentVersion {
            id: id.into(),
            path: "sample.txt".into(),
            revision,
        },
        closed: false,
    };
    for revision in 0..1000 {
        manager.document_changed(change("open-1", revision));
    }
    manager.document_changed(change("open-1", 2));
    manager.poll();
    assert!(text(&manager, id).contains("revision: 999"));
    let latest = text(&manager, id);
    manager.document_changed(change("open-1", 20));
    manager.poll();
    assert_eq!(
        text(&manager, id),
        latest,
        "late revisions must be rejected across delivery batches too"
    );
    manager
        .invoke_command(
            id,
            "scope-probe",
            json!({"method":"close_resource", "handle":subscription}),
        )
        .unwrap();
    let released = text(&manager, id);
    manager.document_changed(change("open-1", 1000));
    manager.poll();
    assert_eq!(text(&manager, id), released);
    manager
        .invoke_command(id, "scope-probe", json!({"method":"subscribe_documents"}))
        .unwrap();
    for index in 0..65 {
        manager.document_changed(change(&format!("open-{index}"), 1));
    }
    manager.poll();
    assert!(text(&manager, id).contains("SubscriptionFailed"));
    assert!(text(&manager, id).contains("LimitExceeded"));
    // A user-created subscription can recover with one click after a terminal overflow.
    manager
        .invoke_command(id, "subscribe-documents", json!(null))
        .unwrap();
    for index in 0..65 {
        manager.document_changed(change(&format!("overflow-{index}"), 1));
    }
    manager.poll();
    assert!(text(&manager, id).contains("SubscriptionFailed"));
    manager
        .invoke_command(id, "subscribe-documents", json!(null))
        .unwrap();
    manager.document_changed(change("recovered", 1));
    manager.poll();
    assert!(text(&manager, id).contains("recovered"));
    manager.disable(id).unwrap();
    assert_eq!(manager.resource_count(), 0);
}
