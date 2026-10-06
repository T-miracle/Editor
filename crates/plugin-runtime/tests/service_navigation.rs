//! Real SDK guests cannot borrow navigation authority from a stronger service provider.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{
        Environment,
        api::{DocumentVersion, EditorOperation, NavigationTarget, Operation},
        ui::Kind,
    },
};
use serde_json::{Value, json};
use std::io::{Cursor, Write};

#[path = "support/service_packages.rs"]
mod service_packages;

/// Change only public declarations; the actual independent component performs the nested request.
fn package(id: &str, provider: bool, delegated: &[&str]) -> Package {
    let mut files =
        service_packages::package(id, provider, false, if provider { "1.0.0" } else { "^1" }).files;
    let mut manifest: Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["permissions"] = if provider {
        json!([
            "assets.read",
            "editor.read",
            "workspace.read",
            "navigation.external",
            "services.call"
        ])
    } else if delegated.contains(&"workspace.read") {
        json!([
            "assets.read",
            "editor.read",
            "workspace.read",
            "services.call"
        ])
    } else {
        json!(["assets.read", "editor.read", "services.call"])
    };
    // Publish only the probe contract so unrelated fixture methods cannot demand broader grants.
    let methods = json!({"probe": {
        "parameters": {"type": "string", "max_bytes": 2048},
        "result": {"type": "string", "max_bytes": 4096},
        "permissions": delegated
    }});
    if provider {
        manifest["api"]["required"]["editor.navigation"] = json!("^1");
        manifest["plugin_services"]["provides"]["example.echo"]["methods"] = methods.clone();
    }
    manifest["plugin_services"]["requires"]["example.echo"]["methods"] = methods;
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
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

/// Observe the real consumer's public native status after its provider probes an editor operation.
fn probe(manager: &mut Manager, target: NavigationTarget) -> String {
    let operation = Operation::Editor {
        operation: EditorOperation::NavigateDocument {
            document: DocumentVersion {
                id: "source-document".into(),
                path: "source.md".into(),
                revision: 1,
            },
            target,
        },
        timeout_ms: 30_000,
    };
    manager.invoke_command("service-consumer", "service-call", json!({
        "method": "probe", "value": serde_json::to_string(&operation).unwrap(), "timeout_ms": 30_000
    })).unwrap();
    manager.poll();
    let Kind::Text { text } = &manager.live["service-consumer"].views["welcome"].root.kind else {
        panic!("Expected the real SDK consumer status");
    };
    text.clone()
}

/// Reject both target privileges before an unauthorized request reaches the native editor queue.
#[test]
#[ignore = "build capability-example through the public SDK first"]
fn delegated_navigation_cannot_borrow_provider_file_or_browser_permissions() {
    let private = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        private.path().join("plugins"),
        Environment {
            workspace: workspace.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    for (id, provider) in [("navigation-provider", true), ("service-consumer", false)] {
        let package = package(id, provider, &["editor.read"]);
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
    }
    manager
        .invoke_command("service-consumer", "service-open", json!("example.echo"))
        .unwrap();
    for target in [
        NavigationTarget::RelativeDocument {
            path: "target.md".into(),
        },
        NavigationTarget::ExternalUrl {
            url: "https://example.com/help".into(),
        },
    ] {
        let result = probe(&mut manager, target);
        assert!(
            result.contains("permission_denied"),
            "Borrowed provider navigation authority: {result}"
        );
        assert!(
            manager
                .live
                .get_mut("navigation-provider")
                .unwrap()
                .take_editor_requests()
                .is_empty(),
            "A refused service navigation must never enter the native work queue"
        );
    }
}

/// Explicitly delegated workspace navigation remains available after tightening weaker calls.
#[test]
#[ignore = "build capability-example through the public SDK first"]
fn delegated_relative_navigation_accepts_both_required_grants() {
    let private = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        private.path().join("plugins"),
        Environment {
            workspace: workspace.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    for (id, provider) in [("navigation-provider", true), ("service-consumer", false)] {
        let package = package(id, provider, &["editor.read", "workspace.read"]);
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
    }
    manager
        .invoke_command("service-consumer", "service-open", json!("example.echo"))
        .unwrap();
    let result = probe(
        &mut manager,
        NavigationTarget::RelativeDocument {
            path: "target.md".into(),
        },
    );
    assert!(
        result.contains("Accepted"),
        "Explicit target authority was rejected: {result}"
    );
    assert_eq!(
        manager
            .live
            .get_mut("navigation-provider")
            .unwrap()
            .take_editor_requests()
            .len(),
        1
    );
}
