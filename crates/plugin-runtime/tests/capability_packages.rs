//! Package-boundary checks for capability negotiation before any guest can run.
use plugin_runtime::Package;
use serde_json::json;
use std::io::{Cursor, Write};

/// A minimal component header suffices here: negotiation precedes WASM instantiation.
fn package(api: serde_json::Value) -> anyhow::Result<Package> {
    let manifest = json!({
        "id": "independent-fixture", "name": "Independent fixture", "version": "3.2.1",
        "protocol": 7, "api": api, "component": "guest.wasm", "storage_limit": 1024
    });
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("manifest.json", serde_json::to_vec(&manifest)?),
        ("guest.wasm", b"\0asm\x0d\0\x01\0".to_vec()),
    ] {
        archive.start_file(name, zip::write::SimpleFileOptions::default())?;
        archive.write_all(&bytes)?;
    }
    Package::from_bytes(&archive.finish()?.into_inner())
}

/// An unrelated unavailable optional interface must not block a supported package.
#[test]
fn package_negotiates_required_interfaces_and_ignores_unavailable_optional_ones() {
    let result = package(json!({
        "base": "^1", "required": {"package.assets": ">=1.0, <2", "ui.native": "^1"},
        "optional": {"example.future": "^9"}
    }));
    assert!(
        result.is_ok(),
        "supported capability package rejected: {:?}",
        result.err()
    );
}

/// Compatibility is determined per interface, not by the unrelated package version.
#[test]
fn incompatible_required_capabilities_and_base_versions_are_rejected_at_inspection() {
    for (api, explanation) in [
        (
            json!({"base":"^1", "required":{"example.future":"^9"}}),
            "Required capability unavailable",
        ),
        (
            json!({"base":"^1", "required":{"ui.native":"^2"}}),
            "Required capability unavailable",
        ),
        (json!({"base":"^2"}), "Unsupported base API"),
        (
            json!({"base":"^1", "required":{"ui.native":"^1"}, "optional":{"ui.native":"^1"}}),
            "both required and optional",
        ),
    ] {
        let error = match package(api) {
            Ok(_) => panic!("incompatible package accepted"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains(explanation),
            "unexpected error: {error}"
        );
    }
}

/// Run the independently compiled component through the real package manager boundary.
#[test]
#[ignore = "build with scripts/build-capability-example.ps1 first"]
fn capability_guest_installs_reads_assets_displays_fallback_and_uninstalls() {
    use plugin_runtime::{Manager, plugin_protocol::Environment};
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/plugin-api-test/capability-example.zip");
    let package = Package::read(&path).expect("read independently built package");
    let directory = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(directory.path().into(), Environment::default()).unwrap();
    assert!(manager.install(&package, Default::default()).is_err());
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let scene = manager.live[&package.manifest.id]
        .views
        .values()
        .next()
        .unwrap();
    let document = scene.as_ref();
    let plugin_runtime::plugin_protocol::ui::Kind::Text { text } = &document.root.kind else {
        panic!("expected native text");
    };
    assert!(text.contains("Hello from a versioned package asset."));
    assert!(text.contains("Optional feature unavailable; native fallback active."));
    manager
        .invoke_command(
            &package.manifest.id,
            "check-errors",
            serde_json::Value::Null,
        )
        .unwrap();
    let scene = manager.live[&package.manifest.id]
        .views
        .values()
        .next()
        .unwrap();
    let plugin_runtime::plugin_protocol::ui::Kind::Text { text } = &scene.as_ref().root.kind else {
        panic!("expected diagnostic text");
    };
    assert_eq!(text, "Typed errors and request IDs verified.");
    manager.uninstall(&package.manifest.id, true).unwrap();
    assert!(!manager.installed.contains_key(&package.manifest.id));
    assert!(!manager.live.contains_key(&package.manifest.id));
}

/// Repack the same real guest: an undeclared permission or interface must fail at its actual call.
#[test]
#[ignore = "build with scripts/build-capability-example.ps1 first"]
fn capability_guest_cannot_borrow_undeclared_permissions_or_interfaces() {
    use plugin_runtime::{Manager, plugin_protocol::Environment};
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/plugin-api-test/capability-example.zip");
    let original = Package::read(&path).unwrap();
    for (remove_permission, expected) in [
        (
            true,
            plugin_runtime::plugin_protocol::api::ErrorCode::PermissionDenied,
        ),
        (
            false,
            plugin_runtime::plugin_protocol::api::ErrorCode::CapabilityUnavailable,
        ),
    ] {
        let mut manifest = original.manifest.clone();
        if remove_permission {
            manifest.permissions.clear();
        } else {
            manifest
                .api
                .as_mut()
                .unwrap()
                .required
                .remove("package.assets");
        }
        let mut files = original.files.clone();
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
        let package = Package::from_bytes(&archive.finish().unwrap().into_inner()).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let mut manager = Manager::open(directory.path().into(), Environment::default()).unwrap();
        let error = manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap_err();
        assert!(
            // Structured failure survives contextual plugin/scope diagnostics.
            error
                .downcast_ref::<plugin_runtime::plugin_protocol::api::Failure>()
                .is_some_and(|failure| failure.code == expected),
            "unexpected rejection: {error:#}"
        );
        assert!(manager.installed.is_empty());
        assert!(manager.live.is_empty());
    }
}
