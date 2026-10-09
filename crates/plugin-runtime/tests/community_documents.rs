//! Real SDK packages enter document capabilities through the public package manager.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, api, ui},
};
use serde_json::json;

/// Read the public diagnostic surface produced by the independent history guest.
fn result(manager: &Manager) -> Result<api::Value, api::Failure> {
    let scene = &manager.live["history-preview"].views["history"];
    let ui::Kind::Text { text } = &scene.root.kind else {
        panic!("history result expected")
    };
    serde_json::from_str(text).unwrap()
}

/// The fixture exposes its bounded diagnostic event log through an ordinary manifest command.
fn events(manager: &mut Manager) -> Vec<api::Notification> {
    manager
        .invoke_command("history-preview", "events", json!(null))
        .unwrap();
    let ui::Kind::Text { text } = &manager.live["history-preview"].views["history"].root.kind
    else {
        panic!("event log expected")
    };
    serde_json::from_str(text).unwrap()
}

/// A document reader can enumerate the live editor without receiving write permission.
#[test]
#[ignore = "package plugins/history-preview with the current host --plugin-package first"]
fn document_reader_can_request_live_document_enumeration() {
    let root = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let package = Package::read(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/community-api/history-preview-0.1.2.zip"),
    )
    .unwrap();
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
            "history-preview",
            "probe",
            json!({
                "method": "editor", "operation": { "kind": "list_documents" }, "timeout_ms": 30000
            }),
        )
        .unwrap();
    assert!(
        matches!(result(&manager), Ok(api::Value::Accepted(_))),
        "enumeration must be admitted: {:?}",
        result(&manager)
    );
}

/// Detailed notifications are explicitly requested instead of changing existing subscribers.
#[test]
#[ignore = "package plugins/history-preview with the current host --plugin-package first"]
fn document_reader_can_opt_in_to_ordered_events() {
    let root = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let package = Package::read(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/community-api/history-preview-0.1.2.zip"),
    )
    .unwrap();
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
            "history-preview",
            "probe",
            json!({"method":"subscribe_document_events"}),
        )
        .unwrap();
    let api::Value::Resource(subscription) =
        result(&manager).expect("explicit ordered subscription must be admitted")
    else {
        panic!("subscription expected")
    };
    manager
        .invoke_command(
            "history-preview",
            "probe",
            json!({"method":"subscribe_documents"}),
        )
        .unwrap();
    let api::Value::Resource(legacy) = result(&manager).unwrap() else {
        panic!("legacy subscription expected")
    };
    let document = api::DocumentVersion {
        id: "native-session".into(),
        path: "local.txt".into(),
        revision: 1,
    };
    manager.document_changed(api::DocumentChange {
        document: document.clone(),
        closed: false,
    });
    for sequence in [7, 8] {
        manager.document_event(api::DocumentEvent {
            sequence,
            resource: Some(api::ResourceIdentity::Local {
                path: document.path.clone(),
            }),
            kind: api::DocumentEventKind::WillSave(document.clone()),
        });
    }
    manager.poll();
    let log = events(&mut manager);
    assert!(log.iter().any(|event| matches!(event, api::Notification::Document { subscription, change } if subscription == &legacy && change.document == document)));
    let sequences = log
        .iter()
        .filter_map(|event| match event {
            api::Notification::DocumentEvent {
                subscription: owner,
                event,
            } if owner == &subscription => Some(event.sequence),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(sequences, [7, 8]);
    assert!(!log.iter().any(|event| matches!(event, api::Notification::DocumentEvent { subscription, .. } if subscription == &legacy)), "legacy handles never receive rich variants");
    manager
        .invoke_command(
            "history-preview",
            "probe",
            json!({"method":"close_resource", "handle":subscription}),
        )
        .unwrap();
    assert!(matches!(result(&manager), Ok(api::Value::Unit)));
    manager.document_event(api::DocumentEvent {
        sequence: 9,
        resource: None,
        kind: api::DocumentEventKind::ActiveChanged(None),
    });
    manager.poll();
    assert!(!events(&mut manager).iter().any(|event| matches!(event, api::Notification::DocumentEvent { subscription: owner, event } if owner == &subscription && event.sequence == 9)));
    manager
        .invoke_command(
            "history-preview",
            "probe",
            json!({"method":"subscribe_document_events"}),
        )
        .unwrap();
    let api::Value::Resource(overflow) = result(&manager).unwrap() else {
        panic!("subscription expected")
    };
    for sequence in 10..140 {
        manager.document_event(api::DocumentEvent {
            sequence,
            resource: None,
            kind: api::DocumentEventKind::ActiveChanged(None),
        });
    }
    manager.poll();
    assert!(events(&mut manager).iter().any(|event| matches!(event, api::Notification::SubscriptionFailed { subscription, error } if subscription == &overflow && error.code == api::ErrorCode::LimitExceeded)));
    // Recovery re-enters enumeration and a fresh handle rather than trusting a partial event history.
    manager
        .invoke_command(
            "history-preview",
            "probe",
            json!({"method":"subscribe_document_events"}),
        )
        .unwrap();
    assert!(matches!(result(&manager), Ok(api::Value::Resource(_))));
    manager
        .invoke_command(
            "history-preview",
            "probe",
            json!({"method":"editor", "operation":{"kind":"list_documents"}, "timeout_ms":30000}),
        )
        .unwrap();
    assert!(matches!(result(&manager), Ok(api::Value::Accepted(_))));
}

/// Current SDK callers cannot turn a named resource or negotiated capability into read authority.
#[test]
#[ignore = "package plugins/history-preview with the current host --plugin-package first"]
fn document_reader_requires_permission_and_workspace_scope() {
    let package = Package::read(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/community-api/history-preview-0.1.2.zip"),
    )
    .unwrap();
    for application in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let mut files = package.files.clone();
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&files["manifest.json"]).unwrap();
        if application {
            manifest["scope"] = json!("application");
        } else {
            manifest["permissions"] = json!(["ui.panels"]);
        }
        files.insert(
            "manifest.json".into(),
            serde_json::to_vec(&manifest).unwrap(),
        );
        let scoped = Package::from_files(files).unwrap();
        let mut manager = Manager::open(
            root.path().into(),
            Environment {
                workspace: workspace.path().display().to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        manager
            .install(&scoped, scoped.manifest.permissions.clone())
            .unwrap();
        for operation in [
            json!({"method":"subscribe_document_events"}),
            json!({"method":"editor", "operation":{"kind":"list_documents"}, "timeout_ms":30000}),
            json!({"method":"editor", "operation":{"kind":"open_virtual_document", "title":"forged", "text":"no authority"}, "timeout_ms":30000}),
        ] {
            manager
                .invoke_command("history-preview", "probe", operation)
                .unwrap();
            assert_eq!(
                result(&manager).unwrap_err().code,
                api::ErrorCode::PermissionDenied
            );
        }
    }
}

/// A package built by the baseline host keeps the old SDK's strict notification decoder working.
#[test]
#[ignore = "build capability-example with the pre-1.1 host into target/community-legacy first"]
fn old_sdk_local_document_subscriptions_continue_on_documents_1_1() {
    let package = Package::read(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/community-legacy/capability-example-0.17.0.zip"),
    )
    .unwrap();
    let component = &package.files["capability-example.wasm"];
    assert!(
        !component
            .windows(b"subscribe_document_events".len())
            .any(|bytes| bytes == b"subscribe_document_events"),
        "fixture must actually use the previous SDK decoder"
    );
    let root = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
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
            "capability-example",
            "scope-probe",
            json!({"method":"subscribe_documents"}),
        )
        .unwrap();
    let read = |manager: &Manager| {
        let ui::Kind::Text { text } = &manager.live["capability-example"].views["welcome"]
            .root
            .kind
        else {
            panic!("old guest text expected")
        };
        text.clone()
    };
    let subscription: Result<api::Value, api::Failure> =
        serde_json::from_str(&read(&manager)).unwrap();
    let api::Value::Resource(subscription) = subscription.unwrap() else {
        panic!("old subscription expected")
    };
    let document = api::DocumentVersion {
        id: "old-native-session".into(),
        path: "local.txt".into(),
        revision: 4,
    };
    manager.document_changed(api::DocumentChange {
        document: document.clone(),
        closed: false,
    });
    manager.document_event(api::DocumentEvent {
        sequence: 1,
        resource: Some(api::ResourceIdentity::Local {
            path: document.path.clone(),
        }),
        kind: api::DocumentEventKind::WillSave(document),
    });
    manager.poll();
    assert!(
        read(&manager).contains("old-native-session"),
        "old notification must reach its old consumer"
    );
    let prior = read(&manager);
    manager.document_changed(api::DocumentChange {
        document: api::DocumentVersion {
            id: "virtual-hidden".into(),
            path: "nanobug-virtual://1/1".into(),
            revision: 0,
        },
        closed: false,
    });
    manager.poll();
    assert_eq!(
        read(&manager),
        prior,
        "legacy subscriptions never receive virtual URI paths"
    );
    manager
        .invoke_command(
            "capability-example",
            "scope-probe",
            json!({"method":"close_resource", "handle":subscription}),
        )
        .unwrap();
    let result: Result<api::Value, api::Failure> = serde_json::from_str(&read(&manager)).unwrap();
    assert!(
        matches!(result, Ok(api::Value::Unit)),
        "old SDK still decodes and dispatches after subscription callbacks"
    );
}
