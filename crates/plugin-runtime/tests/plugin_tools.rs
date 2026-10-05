//! Actual public-SDK packages exercise tool authority and workspace/file-type preference persistence.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, api, ui},
};
use std::{
    io::{Cursor, Write},
    path::Path,
};

/// Admission variants pass through the archive validator and execute the unchanged independent guest.
fn package(variant: &str) -> Package {
    let mut files = Package::read(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-layout-test/layout-example.zip"),
    )
    .expect("build-layout-example.ps1 first")
    .files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    match variant {
        "unnegotiated" => {
            manifest["api"]["required"]
                .as_object_mut()
                .unwrap()
                .remove("ui.tools");
        }
        "no-storage" => {
            manifest["permissions"] = serde_json::json!(["editor.read", "workspace.read"]);
        }
        "missing-icon" => {
            files.remove("icons/row.svg");
        }
        "foreign-icon" => {
            files.insert("icons/row.svg".into(), br#"<svg xmlns="http://www.w3.org/2000/svg"><path fill="url(file:///outside)" d="M0 0"/></svg>"#.to_vec());
        }
        _ => {}
    }
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&zip.finish().unwrap().into_inner()).unwrap()
}

fn environment(root: &Path) -> Environment {
    Environment {
        workspace: root.display().to_string(),
        ..Default::default()
    }
}
fn file(id: &str, name: &str) -> api::FileContext {
    api::FileContext {
        version: api::FileVersion {
            id: id.into(),
            path: name.into(),
            revision: 1,
        },
        file_type: "layout".into(),
        text: Some(api::DocumentVersion {
            id: format!("text-{id}"),
            path: name.into(),
            revision: 1,
        }),
    }
}
fn preview(manager: &mut Manager, file: Option<api::FileContext>) -> anyhow::Result<()> {
    manager.event(
        "layout-example",
        Some("layout".into()),
        api::Notification::FilePreview { file },
    )
}
fn select(manager: &mut Manager, id: &str) -> api::Notification {
    let doc = &manager.live["layout-example"].views["layout"];
    let tool = doc.tools.iter().find(|tool| tool.id == id).unwrap();
    api::Notification::Tool(ui::ToolEvent {
        revision: doc.revision,
        tool: id.into(),
        target: tool.target.clone(),
    })
}
fn selected(manager: &Manager) -> String {
    manager.live["layout-example"].views["layout"]
        .tools
        .iter()
        .find(|tool| tool.selected)
        .unwrap()
        .id
        .clone()
}

/// Missing capability, consent or owned artwork must fail before any invalid tool scene publishes.
#[test]
#[ignore = "build actual SDK fixture with scripts/build-layout-example.ps1 first"]
fn tools_require_negotiation_storage_consent_and_safe_owned_icons() {
    let workspace = tempfile::tempdir().unwrap();
    for (variant, code) in [
        ("unnegotiated", api::ErrorCode::CapabilityUnavailable),
        ("no-storage", api::ErrorCode::PermissionDenied),
        ("missing-icon", api::ErrorCode::InvalidPath),
        ("foreign-icon", api::ErrorCode::InvalidRequest),
    ] {
        let data = tempfile::tempdir().unwrap();
        let pkg = package(variant);
        let mut manager =
            Manager::open(data.path().to_path_buf(), environment(workspace.path())).unwrap();
        manager
            .install(&pkg, pkg.manifest.permissions.clone())
            .unwrap();
        let error = preview(&mut manager, Some(file("first", "first.layout"))).unwrap_err();
        assert_eq!(
            error.downcast_ref::<api::Failure>().unwrap().code,
            code,
            "{variant}: {error:?}"
        );
    }
}

/// Same-type files converge, other workspaces remain independent, and reopening restores guest intent.
#[test]
#[ignore = "build actual SDK fixture with scripts/build-layout-example.ps1 first"]
fn tools_persist_private_intent_and_reject_late_file_events() {
    let workspace = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let pkg = package("normal");
    let mut manager =
        Manager::open(data.path().to_path_buf(), environment(workspace.path())).unwrap();
    manager
        .install(&pkg, pkg.manifest.permissions.clone())
        .unwrap();
    preview(&mut manager, Some(file("first", "first.layout"))).unwrap();
    assert_eq!(selected(&manager), "tool-row");
    let old = select(&mut manager, "tool-content");
    manager
        .event("layout-example", Some("layout".into()), old.clone())
        .unwrap();
    assert_eq!(selected(&manager), "tool-content");
    preview(&mut manager, Some(file("second", "second.layout"))).unwrap();
    assert_eq!(selected(&manager), "tool-content");
    let error = manager
        .event("layout-example", Some("layout".into()), old)
        .unwrap_err();
    assert_eq!(
        error.downcast_ref::<api::Failure>().unwrap().code,
        api::ErrorCode::StaleRevision
    );
    assert!(manager.live.contains_key("layout-example"));
    // Withdrawal closes the preference watch and removes function contributions before reuse.
    assert_eq!(manager.live["layout-example"].resource_count(), 2);
    preview(&mut manager, None).unwrap();
    assert_eq!(manager.live["layout-example"].resource_count(), 1);
    assert!(
        manager.live["layout-example"].views["layout"]
            .tools
            .is_empty()
    );
    manager
        .switch_workspace(environment(other.path()), true)
        .unwrap();
    preview(&mut manager, Some(file("other", "other.layout"))).unwrap();
    assert_eq!(selected(&manager), "tool-row");
    manager
        .switch_workspace(environment(workspace.path()), true)
        .unwrap();
    preview(&mut manager, Some(file("third", "third.layout"))).unwrap();
    assert_eq!(selected(&manager), "tool-content");
    drop(manager);
    let mut manager =
        Manager::open(data.path().to_path_buf(), environment(workspace.path())).unwrap();
    preview(&mut manager, Some(file("restart", "first.layout"))).unwrap();
    assert_eq!(selected(&manager), "tool-content");
    manager.disable("layout-example").unwrap();
    assert_eq!(manager.resource_count(), 0);
}

/// A live scoped watch observes a newer record; corrupt external writes are preserved and terminate it.
#[test]
#[ignore = "build actual SDK fixture with scripts/build-layout-example.ps1 first"]
fn private_preference_watch_converges_and_reports_corruption() {
    let workspace = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let pkg = package("normal");
    let mut manager =
        Manager::open(data.path().to_path_buf(), environment(workspace.path())).unwrap();
    manager
        .install(&pkg, pkg.manifest.permissions.clone())
        .unwrap();
    preview(&mut manager, Some(file("first", "first.layout"))).unwrap();
    let event = select(&mut manager, "tool-column");
    manager
        .event("layout-example", Some("layout".into()), event)
        .unwrap();
    let scope = std::fs::read_dir(data.path().join("data/layout-example/workspaces"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let path = std::fs::read_dir(scope.join("files"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(&path, br#"{"revision":2,"data":2}"#).unwrap();
    manager.poll();
    assert_eq!(selected(&manager), "tool-content");
    std::fs::write(&path, b"corrupt").unwrap();
    manager.poll();
    assert_eq!(std::fs::read(&path).unwrap(), b"corrupt");
    assert_eq!(manager.live["layout-example"].resource_count(), 1);
}
