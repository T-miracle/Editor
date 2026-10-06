//! Retired UI metadata imports preserve installation preferences without admitting executable old contracts.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, settings::Scope},
};
use serde_json::{Value, json};
use std::io::{Cursor, Write};

/// A current resource-only peer proves that one obsolete executable record cannot block unrelated packages.
fn resource_package(id: &str, version: &str) -> Package {
    let manifest = json!({
        "id":id, "name":id, "version":version, "protocol":7,
        "api":{"base":"^1"}, "contributions":"plugin.toml", "permissions":[],
        "settings":{"label":{"title":"Label", "value_type":{"kind":"string", "max_length":120},
            "default":"Default", "scope":"user", "apply":"restart_instance"}},
        "panels":[], "commands":[], "storage_limit":1024
    });
    let declaration =
        format!("[plugin]\nid='{id}'\nname='{id}'\nversion='{version}'\nhost_version='>=0.1.0'\n");
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
        ("plugin.toml", declaration.into_bytes()),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&zip.finish().unwrap().into_inner()).unwrap()
}

/// Old optional command fields were serialized even when null; component access must remain prohibited.
fn old_record(id: &str) -> Value {
    let mut manifest = serde_json::to_value(resource_package(id, "1.0.0").manifest).unwrap();
    manifest["contributions"] = Value::Null;
    manifest["component"] = json!("must-not-be-read.wasm");
    manifest["panels"] = json!([{"id":"panel", "title":"Panel", "position":"right"}]);
    manifest["commands"] = json!([{
        "id":"action", "title":"Action", "toolbar":null, "toolbar_icon":null
    }]);
    json!({"manifest":manifest, "digest":"a".repeat(64), "enabled":true,
        "grants":["storage"], "project_enabled":["other-workspace"]})
}

/// Startup, reopen and explicit enable keep old records unavailable until a current package replaces them.
#[test]
fn retired_ui_registry_preserves_records_data_and_current_peers() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let peer = resource_package("current-peer", "1.0.0");
    let mut manager = Manager::open(root.into(), Environment::default()).unwrap();
    manager.install(&peer, Default::default()).unwrap();
    drop(manager);
    let registry = root.join("registry.json");
    let mut records: Value = serde_json::from_slice(&std::fs::read(&registry).unwrap()).unwrap();
    records["old-command"] = old_record("old-command");
    let mut panel = old_record("old-panel");
    panel["enabled"] = json!(false);
    panel["manifest"]["panels"][0]["view_modes"] = json!({
        "source":"icons/source.svg", "split":"icons/split.svg", "preview":"icons/preview.svg"
    });
    records["old-panel"] = panel;
    let original = serde_json::to_vec_pretty(&records).unwrap();
    std::fs::write(&registry, &original).unwrap();
    let data = root.join("data/old-command/private-note.txt");
    let settings = root.join("settings/old-command.json");
    std::fs::create_dir_all(data.parent().unwrap()).unwrap();
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::fs::write(&data, b"opaque user data").unwrap();
    std::fs::write(&settings, br#"{"user":{"label":"saved"},"projects":{}}"#).unwrap();

    let mut manager = Manager::open(root.into(), Environment::default()).unwrap();
    assert!(
        manager.installed["current-peer"]
            .compatibility_error()
            .is_none()
    );
    assert!(manager.installed["old-command"].enabled);
    assert!(!manager.installed["old-panel"].enabled);
    for id in ["old-command", "old-panel"] {
        let entry = &manager.installed[id];
        assert!(entry.grants.contains("storage"));
        assert!(entry.project_enabled.contains("other-workspace"));
        assert!(entry.compatibility_error().unwrap().contains("更新插件"));
        assert!(manager.instance_id(id).is_none());
        assert!(
            manager
                .enable(id)
                .unwrap_err()
                .to_string()
                .contains("更新插件")
        );
        // Settings validation also prepares guests, including disabled ones, so it must honor the same gate.
        assert!(
            manager
                .update_setting(id, Scope::User, "label", Some(json!("attempted")))
                .unwrap_err()
                .to_string()
                .contains("更新插件")
        );
        assert!(manager.instance_id(id).is_none());
    }
    assert_eq!(std::fs::read(&data).unwrap(), b"opaque user data");
    assert_eq!(
        std::fs::read(&settings).unwrap(),
        br#"{"user":{"label":"saved"},"projects":{}}"#
    );
    assert_eq!(
        std::fs::read(root.join("registry.before-ui-contract.json")).unwrap(),
        original
    );
    drop(manager);
    let mut manager = Manager::open(root.into(), Environment::default()).unwrap();
    assert!(
        manager.installed["old-command"]
            .compatibility_error()
            .is_some()
    );
    manager
        .install(
            &resource_package("old-command", "2.0.0"),
            Default::default(),
        )
        .unwrap();
    assert!(
        manager.installed["old-command"]
            .compatibility_error()
            .is_none()
    );
    assert!(manager.installed["old-command"].enabled);
    assert!(
        manager.installed["old-command"]
            .project_enabled
            .contains("other-workspace")
    );
    assert_eq!(std::fs::read(&data).unwrap(), b"opaque user data");
    assert_eq!(
        std::fs::read(&settings).unwrap(),
        br#"{"user":{"label":"saved"},"projects":{}}"#
    );
}

/// Import is limited to known retired fields; an unrelated unknown public field remains invalid metadata.
#[test]
fn retired_ui_import_does_not_accept_unknown_current_fields() {
    let directory = tempfile::tempdir().unwrap();
    let mut record = old_record("unknown-field");
    record["manifest"]["commands"][0]
        .as_object_mut()
        .unwrap()
        .remove("toolbar");
    record["manifest"]["commands"][0]
        .as_object_mut()
        .unwrap()
        .remove("toolbar_icon");
    record["manifest"]["panels"][0]["unknown_layout"] = json!(true);
    std::fs::write(
        directory.path().join("registry.json"),
        serde_json::to_vec(&json!({"unknown-field":record})).unwrap(),
    )
    .unwrap();
    let error = Manager::read_registry(directory.path()).unwrap_err();
    assert!(error.to_string().contains("unknown_layout"));
}

/// A current independent component stays dormant behind imported metadata until a verified SDK package replaces it.
#[test]
#[ignore = "build current capability-example with scripts/verify-plugin-sdk.ps1 first"]
fn retired_ui_install_updates_through_sdk_and_preserves_private_data() {
    let package = Package::read(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-sdk-test/capability-example.zip"),
    )
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let id = &package.manifest.id;
    let mut manager = Manager::open(root.into(), Environment::default()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let private = manager.data_directory(id).join("upgrade-proof.txt");
    std::fs::write(&private, b"keep opaque content").unwrap();
    drop(manager);
    let settings = root.join("settings").join(format!("{id}.json"));
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    let saved = br#"{"user":{"count":8},"projects":{}}"#;
    std::fs::write(&settings, saved).unwrap();
    let registry = root.join("registry.json");
    let mut records: Value = serde_json::from_slice(&std::fs::read(&registry).unwrap()).unwrap();
    for command in records[id]["manifest"]["commands"].as_array_mut().unwrap() {
        command["toolbar"] = Value::Null;
        command["toolbar_icon"] = Value::Null;
    }
    std::fs::write(&registry, serde_json::to_vec(&records).unwrap()).unwrap();
    let mut manager = Manager::open(root.into(), Environment::default()).unwrap();
    assert!(manager.installed[id].enabled);
    assert!(
        manager.instance_id(id).is_none(),
        "import must never execute the old component"
    );
    assert!(manager.installed[id].compatibility_error().is_some());
    assert_eq!(manager.installed[id].grants, package.manifest.permissions);
    assert_eq!(std::fs::read(&private).unwrap(), b"keep opaque content");
    assert_eq!(std::fs::read(&settings).unwrap(), saved);
    // A real readable component must remain dormant even when configuration changes request preparation.
    assert!(
        manager
            .update_setting(id, Scope::User, "count", Some(json!(9)))
            .unwrap_err()
            .to_string()
            .contains("更新插件")
    );
    assert!(manager.instance_id(id).is_none());
    assert_eq!(std::fs::read(&private).unwrap(), b"keep opaque content");
    assert_eq!(std::fs::read(&settings).unwrap(), saved);
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    assert!(manager.instance_id(id).is_some());
    assert!(manager.installed[id].compatibility_error().is_none());
    assert_eq!(std::fs::read(&private).unwrap(), b"keep opaque content");
    assert_eq!(std::fs::read(&settings).unwrap(), saved);
    drop(manager);
    let manager = Manager::open(root.into(), Environment::default()).unwrap();
    assert!(manager.instance_id(id).is_some());
    assert!(manager.installed[id].compatibility_error().is_none());
}
