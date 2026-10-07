//! Optional service preparation can preserve resource features without weakening update rollback.
use plugin_runtime::{Manager, Package, plugin_protocol::Environment};
use serde_json::json;
use std::{
    io::{Cursor, Write},
    path::Path,
};

/// A novel resource package deliberately references missing private service bytes, without network I/O.
fn package(version: &str, missing: &Path, optional: bool) -> Package {
    let manifest = json!({
        "id":"novel-optional", "name":"Novel optional", "version":version, "protocol":7,
        "api":{"base":"^1", "required":{"language.lsp":"^1", "process":"^1", "dependencies":">=1.1,<2"}},
        "contributions":"plugin.toml", "storage_limit":1024,
        "permissions":["process.service.analysis", "dependencies.prepare"],
        "services":{"analysis":{"program":"novel-missing", "installation":{
            "artifacts":[{"id":"native", "version":"1", "platform":format!("{}-{}",std::env::consts::OS,std::env::consts::ARCH),
                "sha256":"0000000000000000000000000000000000000000000000000000000000000000",
                "source":{"kind":"local","path":missing}, "format":{"kind":"file","path":"server.exe"}}],
            "executable":"native/server.exe"}}},
        "language_servers":[{"id":"analysis", "language":"novel", "service":"analysis", "optional_installation":optional}]
    });
    let contribution = format!(
        "[plugin]\nid='novel-optional'\nname='Novel optional'\nversion='{version}'\nhost_version='^0.1'\n[[language_definitions]]\nid='novel'\nname='Novel'\nextensions=['novel']\n"
    );
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
        ("plugin.toml", contribution.into_bytes()),
    ] {
        archive
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&archive.finish().unwrap().into_inner()).unwrap()
}

/// First installation may retain its declared language, while updates preserve the old version on failure.
#[test]
fn optional_language_dependency_failure_keeps_resources_and_rejects_failed_update() {
    let workspace = tempfile::tempdir().unwrap();
    let root = workspace.path().join("private");
    let mut manager = Manager::open(
        root,
        Environment {
            workspace: workspace.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let first = package("1.0.0", &workspace.path().join("missing.exe"), true);
    manager
        .install(&first, first.manifest.permissions.clone())
        .expect("optional service failure must not erase resource features");
    assert!(
        manager
            .published_entries()
            .iter()
            .any(|entry| entry.manifest.id == "novel-optional"
                && entry.enabled
                && entry.error.is_none())
    );
    assert!(
        manager.language_services()["novel-optional/analysis"].is_err(),
        "missing dependency must remain a visible preparation failure"
    );
    let update = package("1.1.0", &workspace.path().join("missing.exe"), true);
    assert!(
        manager
            .install(&update, update.manifest.permissions.clone())
            .is_err()
    );
    assert_eq!(manager.installed["novel-optional"].digest, first.digest);
    manager.uninstall("novel-optional", false).unwrap();
    let required = package("1.0.0", &workspace.path().join("missing.exe"), false);
    assert!(
        manager
            .install(&required, required.manifest.permissions.clone())
            .is_err(),
        "existing packages keep mandatory preparation semantics"
    );
}

/// Deep user-owned installations must publish dependency receipts and survive reopening on Windows too.
#[test]
fn long_private_installation_root_publishes_optional_dependency_receipt() {
    let workspace = tempfile::tempdir().unwrap();
    // Each component is legal; the fixed two-hash receipt name makes the complete path exceed MAX_PATH.
    let root = workspace
        .path()
        .join(format!("native-private-{}", "x".repeat(80)));
    let environment = Environment {
        workspace: workspace.path().display().to_string(),
        ..Default::default()
    };
    let first = package("1.0.0", &workspace.path().join("missing.exe"), true);
    let mut manager = Manager::open(root.clone(), environment.clone()).unwrap();
    manager
        .install(&first, first.manifest.permissions.clone())
        .expect("a valid deep private root must not turn an optional service failure into install failure");
    drop(manager);
    let mut reopened = Manager::open(root, environment).unwrap();
    assert_eq!(reopened.installed["novel-optional"].digest, first.digest);
    assert!(reopened.language_services()["novel-optional/analysis"].is_err());
    assert!(reopened.published_entries().iter().any(|entry| {
        entry.manifest.id == "novel-optional" && entry.enabled && entry.error.is_none()
    }));
}
