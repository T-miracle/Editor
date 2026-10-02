//! Settings are observed through package installation and the public manager, not a parallel resolver.
use plugin_runtime::{Manager, Package, plugin_protocol::Environment};

/// Malformed third-party forms are rejected at inspection, before any settings UI or WASM is created.
#[test]
fn settings_declarations_reject_invalid_defaults_and_unbounded_controls() {
    use std::io::{Cursor, Write};
    for (definition, valid) in [
        (
            serde_json::json!({"title":"Enabled","value_type":{"kind":"boolean"},"default":true}),
            true,
        ),
        (
            serde_json::json!({"title":"Enabled","value_type":{"kind":"boolean"},"default":"yes"}),
            false,
        ),
        (
            serde_json::json!({"title":"Name","value_type":{"kind":"string","max_length":50000},"default":""}),
            false,
        ),
        (
            serde_json::json!({"title":"Mode","value_type":{"kind":"enum","choices":["a","a"]},"default":"a"}),
            false,
        ),
    ] {
        let manifest = serde_json::json!({"id":"settings-fixture","name":"Settings fixture","version":"1.0.0","protocol":7,
            "api":{"base":"^1","required":{"configuration":"^1"}},"component":"guest.wasm","storage_limit":1024,
            "settings":{"option":definition}});
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in [
            ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
            ("guest.wasm", b"\0asm\x0d\0\x01\0".to_vec()),
        ] {
            archive
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(&bytes).unwrap();
        }
        assert_eq!(
            Package::from_bytes(&archive.finish().unwrap().into_inner()).is_ok(),
            valid
        );
    }
}

/// A newly installed independent package exposes its declared defaults with truthful provenance.
#[test]
#[ignore = "build with scripts/build-capability-example.ps1 first"]
fn installed_settings_have_declared_defaults_and_sources() {
    let root = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let package = Package::read(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
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
    let settings = manager.effective_settings(&package.manifest.id).unwrap();
    assert_eq!(settings["enabled"].value, serde_json::json!(true));
    assert_eq!(
        serde_json::to_value(settings["enabled"].source).unwrap(),
        "default"
    );
    assert_eq!(
        settings["label"].value,
        serde_json::json!("Discovered label")
    );
    // Explicit project values outrank user values and survive reopening; invalid updates remain atomic.
    use plugin_runtime::plugin_protocol::settings::{Scope, Source};
    let id = &package.manifest.id;
    manager
        .update_setting(id, Scope::User, "enabled", Some(serde_json::json!(false)))
        .unwrap();
    let plugin_runtime::plugin_protocol::ui::Kind::Text { text } = &manager.live[id]
        .scene
        .as_ref()
        .unwrap()
        .ui
        .as_ref()
        .unwrap()
        .root
        .kind
    else {
        panic!("text expected")
    };
    assert!(
        text.contains("enabled=false"),
        "settings must reach the running guest without host restart"
    );
    manager
        .update_setting(
            id,
            Scope::User,
            "label",
            Some(serde_json::json!("Chosen label")),
        )
        .unwrap();
    assert!(
        manager
            .update_setting(
                id,
                Scope::Project,
                "label",
                Some(serde_json::json!("invalid"))
            )
            .is_err()
    );
    assert_eq!(
        manager.effective_settings(id).unwrap()["label"].value,
        serde_json::json!("Chosen label")
    );
    manager
        .update_setting(id, Scope::Project, "enabled", Some(serde_json::json!(true)))
        .unwrap();
    assert_eq!(
        manager.effective_settings(id).unwrap()["enabled"].source,
        Source::Project
    );
    assert!(
        manager
            .update_setting(
                id,
                Scope::Project,
                "enabled",
                Some(serde_json::json!("invalid"))
            )
            .is_err()
    );
    assert!(
        manager
            .update_setting(id, Scope::Project, "count", Some(serde_json::json!(4)))
            .is_err()
    );
    assert!(
        manager
            .update_setting(
                id,
                Scope::Project,
                "workspace.trusted",
                Some(serde_json::json!(true))
            )
            .is_err()
    );
    drop(manager);
    let mut manager = Manager::open(
        root.path().into(),
        Environment {
            workspace: workspace.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        manager.effective_settings(id).unwrap()["enabled"].value,
        serde_json::json!(true)
    );
    manager
        .update_setting(id, Scope::Project, "enabled", None)
        .unwrap();
    assert_eq!(
        manager.effective_settings(id).unwrap()["enabled"].source,
        Source::User
    );
    assert_eq!(
        manager.effective_settings(id).unwrap()["enabled"].value,
        serde_json::json!(false)
    );
    manager.set_workspace_trust(false).unwrap();
    assert!(
        manager
            .update_setting(id, Scope::Project, "enabled", Some(serde_json::json!(true)))
            .is_err()
    );
}

/// Global updates reach parked owners, while confirmed project values and failed hooks retain their boundaries.
#[test]
#[ignore = "build with scripts/build-capability-example.ps1 first"]
fn configuration_changes_are_isolated_and_disabled_guests_stay_disabled() {
    use plugin_runtime::plugin_protocol::settings::{Scope, Source};
    let root = tempfile::tempdir().unwrap();
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let env = |path: &std::path::Path| Environment {
        workspace: path.display().to_string(),
        ..Default::default()
    };
    let package = Package::read(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
    )
    .unwrap();
    let id = &package.manifest.id;
    let mut manager = Manager::open(root.path().into(), env(a.path())).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    manager
        .update_setting(id, Scope::Project, "enabled", Some(serde_json::json!(true)))
        .unwrap();
    manager.switch_workspace(env(b.path()), true).unwrap();
    manager
        .update_setting(id, Scope::User, "enabled", Some(serde_json::json!(false)))
        .unwrap();
    assert_eq!(
        manager.effective_settings(id).unwrap()["enabled"].source,
        Source::User
    );
    manager.switch_workspace(env(a.path()), true).unwrap();
    assert_eq!(
        manager.effective_settings(id).unwrap()["enabled"].value,
        serde_json::json!(true)
    );
    assert_eq!(
        manager.effective_settings(id).unwrap()["enabled"].source,
        Source::Project
    );
    manager
        .update_setting(id, Scope::Project, "enabled", None)
        .unwrap();
    assert_eq!(
        manager.effective_settings(id).unwrap()["enabled"].value,
        serde_json::json!(false)
    );
    manager.disable(id).unwrap();
    manager
        .update_setting(
            id,
            Scope::User,
            "label",
            Some(serde_json::json!("While disabled")),
        )
        .unwrap();
    assert!(manager.live.is_empty());
    assert_eq!(manager.resource_count(), 0);
    manager.enable(id).unwrap();
    assert_eq!(
        manager.effective_settings(id).unwrap()["label"].value,
        serde_json::json!("While disabled")
    );
    // Preserving data preserves settings; explicit delete-data also removes the host-owned namespace.
    manager.uninstall(id, false).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    assert_eq!(
        manager.effective_settings(id).unwrap()["label"].value,
        serde_json::json!("While disabled")
    );
    manager.uninstall(id, true).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    assert_eq!(
        manager.effective_settings(id).unwrap()["label"].source,
        Source::Discovered
    );
}
