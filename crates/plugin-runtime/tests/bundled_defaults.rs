//! Public Manager lifecycle preserves first-use choices independently of removable plugin-owned data.
use plugin_runtime::{Manager, Package, plugin_protocol::Environment};
use serde_json::json;
use std::io::{Cursor, Write};

/// A real resource-only ZIP uses the ordinary package validator and installer without guest execution.
fn package() -> Package {
    package_version("1.0.0")
}

/// Distinct release digests keep the same real opaque package identity and normal contribution validation.
fn package_version(version: &str) -> Package {
    let manifest = json!({
        "id":"bundled-fixture","name":"Bundled fixture","version":version,"protocol":7,
        "api":{"base":"^1"},"contributions":"plugin.toml","storage_limit":1024
    });
    let contribution = format!(
        "[plugin]\nid='bundled-fixture'\nname='Bundled fixture'\nversion='{version}'\nhost_version='^0.1'\n"
    );
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
        ("plugin.toml", contribution.as_bytes().to_vec()),
    ] {
        archive
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&archive.finish().unwrap().into_inner()).unwrap()
}

/// Exposure is recoverable, while an explicit refusal survives restart and a future package release digest.
#[test]
fn offered_exposure_is_retryable_but_decline_survives_new_release_and_restart() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("private-runtime");
    let environment = Environment {
        workspace: directory.path().display().to_string(),
        ..Default::default()
    };
    let first = package();
    let replacement = package_version("2.0.0");
    assert_ne!(first.digest, replacement.digest);
    let mut manager = Manager::open(root.clone(), environment.clone()).unwrap();
    manager.record_bundle_offer(&first.manifest.id).unwrap();
    assert!(!manager.has_bundle_choice(&first.manifest.id).unwrap());
    assert!(
        manager
            .can_offer_bundle(&first.manifest.id, directory.path())
            .unwrap()
    );
    assert!(
        manager
            .can_install_bundle(&first.manifest.id, directory.path())
            .unwrap()
    );
    drop(manager);
    let mut reopened = Manager::open(root.clone(), environment.clone()).unwrap();
    assert!(
        reopened
            .can_offer_bundle(&replacement.manifest.id, directory.path())
            .unwrap()
    );
    reopened
        .record_bundle_offer(&replacement.manifest.id)
        .unwrap();
    reopened
        .record_bundle_decline(&replacement.manifest.id)
        .unwrap();
    assert!(
        reopened
            .record_bundle_offer(&replacement.manifest.id)
            .is_err(),
        "late exposure cannot overwrite an explicit user refusal"
    );
    drop(reopened);
    let reopened = Manager::open(root, environment).unwrap();
    assert!(reopened.has_bundle_choice(&first.manifest.id).unwrap());
    assert!(
        !reopened
            .can_offer_bundle(&replacement.manifest.id, directory.path())
            .unwrap()
    );
    assert!(
        !reopened
            .can_install_bundle(&replacement.manifest.id, directory.path())
            .unwrap()
    );
    assert!(
        reopened.installed.is_empty() && reopened.live.is_empty(),
        "discovery and refusal execute no guest or installation"
    );
}

/// Damaged host metadata cannot become an empty profile, or remove an existing plugin and its private data.
#[test]
fn malformed_choice_metadata_fails_closed_without_uninstalling_existing_package() {
    for malformed in [
        "{",
        r#"{"version":2,"entries":{}}"#,
        r#"{"version":1,"entries":{"BAD-ID":"offered"}}"#,
        r#"{"version":1,"entries":{"bundled-fixture":"unknown"}}"#,
    ] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("private-runtime");
        let environment = Environment {
            workspace: directory.path().display().to_string(),
            ..Default::default()
        };
        let package = package();
        let mut manager = Manager::open(root.clone(), environment).unwrap();
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
        let data = manager.data_directory(&package.manifest.id);
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("keep.txt"), "retained").unwrap();
        let metadata = root.join("bundle-choices.json");
        std::fs::write(&metadata, malformed).unwrap();
        assert!(manager.has_bundle_choice(&package.manifest.id).is_err());
        assert!(
            manager
                .can_offer_bundle(&package.manifest.id, directory.path())
                .is_err()
        );
        assert!(
            manager
                .can_install_bundle(&package.manifest.id, directory.path())
                .is_err()
        );
        assert!(manager.record_bundle_offer(&package.manifest.id).is_err());
        assert!(manager.uninstall(&package.manifest.id, true).is_err());
        assert!(manager.installed.contains_key(&package.manifest.id));
        assert!(data.join("keep.txt").exists());
        assert_eq!(std::fs::read_to_string(metadata).unwrap(), malformed);
    }
}

/// A restored disabled package cannot be auto-offered or enabled merely because a newer default ships.
#[test]
fn disabled_installation_remains_disabled_across_restart_and_explicit_update() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("private-runtime");
    let environment = Environment {
        workspace: directory.path().display().to_string(),
        ..Default::default()
    };
    let first = package();
    let replacement = package_version("2.0.0");
    let mut manager = Manager::open(root.clone(), environment.clone()).unwrap();
    manager
        .install(&first, first.manifest.permissions.clone())
        .unwrap();
    manager.disable(&first.manifest.id).unwrap();
    drop(manager);
    let mut reopened = Manager::open(root, environment).unwrap();
    assert!(!reopened.installed[&first.manifest.id].enabled);
    assert!(
        !reopened
            .can_offer_bundle(&replacement.manifest.id, directory.path())
            .unwrap()
    );
    assert!(
        !reopened
            .can_install_bundle(&replacement.manifest.id, directory.path())
            .unwrap()
    );
    reopened
        .install(&replacement, replacement.manifest.permissions.clone())
        .unwrap();
    assert_eq!(
        reopened.installed[&first.manifest.id].manifest.version,
        "2.0.0"
    );
    assert!(!reopened.installed[&first.manifest.id].enabled);
}

/// A matching identity cannot reuse a workspace offer after trust withdrawal or workspace replacement.
#[test]
fn pending_exposure_does_not_authorize_restricted_or_different_workspace() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    let other = directory.path().join("other");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    let environment = Environment {
        workspace: workspace.display().to_string(),
        ..Default::default()
    };
    let mut manager = Manager::open(directory.path().join("private-runtime"), environment).unwrap();
    manager.record_bundle_offer("bundled-fixture").unwrap();
    assert!(!manager.can_offer_bundle("bundled-fixture", &other).unwrap());
    assert!(
        !manager
            .can_install_bundle("bundled-fixture", &other)
            .unwrap()
    );
    manager.set_workspace_trust(false).unwrap();
    assert!(
        !manager
            .can_offer_bundle("bundled-fixture", &workspace)
            .unwrap()
    );
    assert!(
        !manager
            .can_install_bundle("bundled-fixture", &workspace)
            .unwrap()
    );
}

/// Both uninstall choices must prevent automatic re-offering after reopening the same private store.
#[test]
fn uninstall_keeps_the_default_choice_after_restart_and_plugin_data_deletion() {
    for delete_data in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("private-runtime");
        let environment = Environment {
            workspace: directory.path().display().to_string(),
            ..Default::default()
        };
        let package = package();
        let mut manager = Manager::open(root.clone(), environment.clone()).unwrap();
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
        let data = manager.data_directory("bundled-fixture");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("retained.txt"), "private plugin data").unwrap();
        manager.uninstall("bundled-fixture", delete_data).unwrap();
        assert!(!manager.installed.contains_key("bundled-fixture"));
        assert_eq!(data.join("retained.txt").exists(), !delete_data);
        assert!(
            manager.has_bundle_choice("bundled-fixture").unwrap(),
            "uninstall is an enduring user choice, even when the package registry entry is gone"
        );
        drop(manager);
        let reopened = Manager::open(root, environment).unwrap();
        assert!(
            reopened.has_bundle_choice("bundled-fixture").unwrap(),
            "a restart must not offer a previously uninstalled default package"
        );
    }
}
