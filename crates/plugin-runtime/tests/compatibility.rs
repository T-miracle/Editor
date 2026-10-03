//! Public package-manager admission retains legacy preferences without touching retired executable formats.
use plugin_runtime::{Manager, plugin_protocol::Environment};
use serde_json::json;

/// Missing component bytes cannot hide the earlier compatibility decision or change persistent user choices.
#[test]
fn old_registry_reports_update_requirement_before_accessing_component() {
    let root = tempfile::tempdir().unwrap();
    let record = json!({"legacy": {
        "manifest": {"id":"legacy","name":"Legacy","version":"1.0.0","protocol":6,
            "component":"missing.wasm","permissions":["storage"],"storage_limit":1024},
        "digest":"a".repeat(64),"grants":["storage"],"enabled":true,
        "project_enabled":["retained-project"]
    }});
    std::fs::write(
        root.path().join("registry.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    let mut manager = Manager::open(root.path().into(), Environment::default()).unwrap();
    assert!(
        manager.installed["legacy"]
            .error
            .as_ref()
            .is_some_and(|error| error.contains("不兼容"))
    );
    assert!(manager.installed["legacy"].enabled);
    assert!(manager.live.is_empty());
    assert!(!manager.published_entries()[0].enabled);
    assert!(
        manager
            .enable("legacy")
            .unwrap_err()
            .to_string()
            .contains("不兼容")
    );
    let saved = Manager::read_registry(root.path()).unwrap();
    assert!(
        saved["legacy"].enabled && saved["legacy"].project_enabled.contains("retained-project")
    );
    assert!(saved["legacy"].grants.contains("storage"));
}

/// Historical aliases carry host settings and an immutable backup, while current package identities stay literal.
#[test]
fn legacy_alias_import_preserves_settings_and_original_bytes() {
    let root = tempfile::tempdir().unwrap();
    let record = json!({"me.example": {
        "manifest": {"id":"me.example","name":"Example","version":"1.0.0","protocol":6,
            "component":"missing.wasm","permissions":["storage"],"storage_limit":1024},
        "digest":"a".repeat(64),"grants":["storage"],"enabled":false,"project_enabled":["project-a"]
    }});
    let original = serde_json::to_vec(&record).unwrap();
    std::fs::write(root.path().join("registry.json"), &original).unwrap();
    std::fs::create_dir_all(root.path().join("settings")).unwrap();
    std::fs::create_dir_all(root.path().join("data/me.example")).unwrap();
    let settings = br#"{"user":{"label":"retained"},"projects":{}}"#;
    std::fs::write(root.path().join("settings/me.example.json"), settings).unwrap();
    std::fs::write(
        root.path().join("data/me.example/settings.json"),
        b"opaque private preferences",
    )
    .unwrap();
    let imported = Manager::read_registry(root.path()).unwrap();
    assert!(!imported["example"].enabled);
    assert!(imported["example"].project_enabled.contains("project-a"));
    assert_eq!(
        std::fs::read(root.path().join("settings/example.json")).unwrap(),
        settings
    );
    assert_eq!(
        std::fs::read(root.path().join("legacy-backup/v7/registry.json")).unwrap(),
        original
    );
    assert_eq!(
        std::fs::read(
            root.path()
                .join("legacy-backup/v7/plugins/example/data/settings.json")
        )
        .unwrap(),
        b"opaque private preferences"
    );
    Manager::read_registry(root.path()).unwrap();
    assert_eq!(
        std::fs::read(root.path().join("legacy-backup/v7/registry.json")).unwrap(),
        original
    );
    assert_eq!(
        std::fs::read(root.path().join("data/me.example/settings.json")).unwrap(),
        b"opaque private preferences"
    );
}

/// Build a current resource-only package whose author-selected ID remains literal at the public ZIP boundary.
fn literal_resource_package(id: &str) -> plugin_runtime::Package {
    use std::io::{Cursor, Write};
    let manifest = json!({"id":id,"name":"Literal resource","version":"1.0.0","protocol":7,
        "api":{"base":"^1"},"contributions":"plugin.toml","storage_limit":1024});
    let declaration = format!(
        "[plugin]\nid = \"{id}\"\nname = \"Literal resource\"\nversion = \"1.0.0\"\nhost_version = \">=0.1.0\"\n"
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
        ("plugin.toml", declaration.into_bytes()),
        ("README.md", b"# Literal resource".to_vec()),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    plugin_runtime::Package::from_bytes(&zip.finish().unwrap().into_inner()).unwrap()
}

/// A collision backup is recovery evidence for the old canonical owner, never data owned by a new literal alias.
#[test]
fn current_literal_alias_uninstalls_without_claiming_a_collision_backup() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(root.path().into(), Environment::default()).unwrap();
    let canonical = literal_resource_package("example");
    manager.install(&canonical, Default::default()).unwrap();
    drop(manager);
    let registry_path = root.path().join("registry.json");
    let mut registry: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&registry_path).unwrap()).unwrap();
    registry["me.example"] = json!({"manifest":{"id":"me.example","name":"Historical alias",
        "version":"0.0.6","protocol":6,"component":"missing.wasm","permissions":["storage"],"storage_limit":1024},
        "digest":"b".repeat(64),"grants":["storage"],"enabled":false,"project_enabled":["retained-project"]});
    std::fs::write(&registry_path, serde_json::to_vec(&registry).unwrap()).unwrap();
    let historical = root.path().join("data/me.example");
    std::fs::create_dir_all(&historical).unwrap();
    std::fs::write(
        historical.join("settings.json"),
        b"opaque historical settings",
    )
    .unwrap();
    let mut manager = Manager::open(root.path().into(), Environment::default()).unwrap();
    assert_eq!(manager.installed["example"].digest, canonical.digest);
    assert!(!manager.installed.contains_key("me.example"));
    let backup = root.path().join("legacy-backup/v7/plugins/me.example");
    let record = std::fs::read(backup.join("record.json")).unwrap();
    let metadata: serde_json::Value = serde_json::from_slice(&record).unwrap();
    assert_eq!(metadata["project_enabled"], json!(["retained-project"]));
    assert_eq!(metadata["grants"], json!(["storage"]));
    let literal = literal_resource_package("me.example");
    manager.install(&literal, Default::default()).unwrap();
    assert_eq!(manager.installed["me.example"].manifest.id, "me.example");
    manager
        .uninstall("me.example", true)
        .expect("deleting a current literal ID must not claim a historical collision backup");
    assert!(manager.installed.contains_key("example"));
    assert!(!manager.installed.contains_key("me.example"));
    assert_eq!(std::fs::read(backup.join("record.json")).unwrap(), record);
    assert_eq!(
        std::fs::read(backup.join("data/settings.json")).unwrap(),
        b"opaque historical settings"
    );
}
