//! An independently SDK-built unfamiliar provider proves pure authority, response validation and lease revocation.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{
        Environment, api::DocumentVersion, language::SourceSnapshot, settings::Scope,
    },
};
use serde_json::json;
use std::{
    io::{Cursor, Write},
    path::PathBuf,
    sync::Arc,
};

/// Use the real hostile guest with ordinary install grants, including IO grants that its pure executor must deny.
fn package() -> Package {
    let original = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
    )
    .unwrap();
    let mut files = original.files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["structure_providers"] =
        json!([{"id":"definitions", "language":"unfamiliar-language"}]);
    manifest["api"]["required"]["language.structure"] = json!("^1");
    manifest["api"]["optional"]
        .as_object_mut()
        .unwrap()
        .remove("language.structure");
    // Geometry-only validation must reject packaged active SVG without discarding its valid definition.
    files.insert("icons/structure-unsafe.svg".into(), br#"<svg xmlns="http://www.w3.org/2000/svg"><image href="https://example.invalid/track"/></svg>"#.to_vec());
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

/// A pure invocation carries no native handle or file authority, including when its normal guest has IO grants.
fn source(text: &str) -> SourceSnapshot {
    SourceSnapshot {
        document: DocumentVersion {
            id: "independent-structure".into(),
            path: "unknown.file".into(),
            revision: 7,
        },
        text: text.into(),
    }
}

/// Malformed targets, UTF-8, paths and quotas are rejected; missing/active artwork falls back without suppressing nodes.
#[test]
#[ignore = "build capability-example through the current public SDK first"]
fn independent_structure_validates_replies_and_denies_io() {
    let workspace = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    let package = package();
    let mut manager = Manager::open(
        store.path().into(),
        Environment {
            workspace: workspace.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    assert!(manager.language_services().is_empty());
    let providers = manager.structure_providers();
    let provider = providers["capability-example/definitions"]
        .as_ref()
        .unwrap();
    let snapshot = provider.describe(source("ordinary source")).unwrap();
    assert_eq!(snapshot.proposal.nodes[0].kind, "unfamiliar-definition");
    assert_eq!(snapshot.proposal.nodes[0].name, "纯结构定义🙂");
    assert!(
        snapshot.icons.is_empty(),
        "missing icon must retain a navigable tree for native fallback"
    );
    for text in ["stale", "bad-range", "中文", "escape-icon", "excessive"] {
        assert!(
            provider.describe(source(text)).is_err(),
            "host accepted hostile reply for {text}"
        );
    }
    let snapshot = provider.describe(source("unsafe-svg")).unwrap();
    assert_eq!(snapshot.proposal.nodes.len(), 1);
    assert!(
        snapshot.icons.is_empty(),
        "active artwork must not enter native SVG rendering"
    );
    assert!(
        provider.describe(source("after rejection")).is_ok(),
        "validation errors must not poison other pure calls"
    );
}

/// Trap and excessive encoded nesting retire only their pure lease; configuration creates a fresh healthy worker.
#[test]
#[ignore = "build capability-example through the current public SDK first"]
fn independent_structure_trap_retires_its_lease_and_configuration_recovers() {
    let workspace = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    let package = package();
    let mut manager = Manager::open(
        store.path().into(),
        Environment {
            workspace: workspace.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let provider = manager.structure_providers()["capability-example/definitions"]
        .as_ref()
        .unwrap()
        .clone();
    let resources = manager.resource_count();
    assert!(provider.describe(source("trap")).is_err());
    assert!(
        !provider.is_active(),
        "a trapped pure executor must invalidate every retained handle"
    );
    assert!(provider.describe(source("after trap")).is_err());
    assert_eq!(
        manager.resource_count() + 1,
        resources,
        "trap must release the one independently counted pure executor"
    );
    assert!(
        manager
            .published_entries()
            .iter()
            .any(|entry| entry.manifest.id == "capability-example"
                && entry.enabled
                && entry.error.is_none()),
        "a pure structure fault must not disable the owner's normal guest or its other roles"
    );
    let unchanged = manager.structure_providers()["capability-example/definitions"]
        .as_ref()
        .unwrap()
        .clone();
    assert!(
        Arc::ptr_eq(&provider, &unchanged),
        "publication must not silently restart a trapped worker"
    );
    manager
        .update_setting(
            "capability-example",
            Scope::Project,
            "label",
            Some(json!("recovered")),
        )
        .unwrap();
    let replaced = manager.structure_providers()["capability-example/definitions"]
        .as_ref()
        .unwrap()
        .clone();
    assert!(!Arc::ptr_eq(&provider, &replaced));
    assert!(replaced.describe(source("healthy replacement")).is_ok());
    let recovered_resources = manager.resource_count();
    let error = replaced.describe(source("overdepth")).unwrap_err();
    assert!(format!("{error:#}").contains("nesting"), "{error:#}");
    assert!(!replaced.is_active());
    assert_eq!(manager.resource_count() + 1, recovered_resources);
    let unchanged = manager.structure_providers()["capability-example/definitions"]
        .as_ref()
        .unwrap()
        .clone();
    assert!(
        Arc::ptr_eq(&replaced, &unchanged),
        "overdepth retirement must not silently restart the worker"
    );
    assert!(
        unchanged
            .describe(source("after excessive nesting"))
            .is_err()
    );
    assert!(
        manager
            .published_entries()
            .iter()
            .any(|entry| entry.manifest.id == "capability-example"
                && entry.enabled
                && entry.error.is_none()),
        "malformed structure output must leave the owner's normal roles available"
    );
}

/// Configuration, trust and workspace replacement invalidate even Arcs retained by a native window.
#[test]
#[ignore = "build capability-example through the current public SDK first"]
fn independent_structure_retires_on_settings_trust_workspace_and_uninstall() {
    let workspace = tempfile::tempdir().unwrap();
    let other_workspace = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    let package = package();
    let environment = |path: &std::path::Path| Environment {
        workspace: path.display().to_string(),
        ..Default::default()
    };
    let mut manager = Manager::open(store.path().into(), environment(workspace.path())).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let first = manager.structure_providers()["capability-example/definitions"]
        .as_ref()
        .unwrap()
        .clone();
    assert!(first.describe(source("valid")).is_ok());
    let unchanged = manager.structure_providers()["capability-example/definitions"]
        .as_ref()
        .unwrap()
        .clone();
    assert!(
        Arc::ptr_eq(&first, &unchanged),
        "unchanged publication must retain the effective provider"
    );
    manager
        .update_setting(
            "capability-example",
            Scope::Project,
            "label",
            Some(json!("replacement")),
        )
        .unwrap();
    let replaced = manager.structure_providers()["capability-example/definitions"]
        .as_ref()
        .unwrap()
        .clone();
    assert!(!first.is_active());
    assert!(first.describe(source("valid")).is_err());
    assert!(!Arc::ptr_eq(&first, &replaced));
    manager.set_workspace_trust(false).unwrap();
    assert!(!replaced.is_active());
    assert!(manager.structure_providers().is_empty());
    assert_eq!(manager.resource_count(), 0);
    manager.set_workspace_trust(true).unwrap();
    let resumed = manager.structure_providers()["capability-example/definitions"]
        .as_ref()
        .unwrap()
        .clone();
    assert!(resumed.describe(source("valid")).is_ok());
    manager
        .switch_workspace(environment(other_workspace.path()), true)
        .unwrap();
    assert!(!resumed.is_active());
    let switched = manager.structure_providers()["capability-example/definitions"]
        .as_ref()
        .unwrap()
        .clone();
    assert!(switched.describe(source("valid")).is_ok());
    manager.uninstall("capability-example", false).unwrap();
    assert!(!switched.is_active());
    assert!(switched.describe(source("valid")).is_err());
    assert!(manager.structure_providers().is_empty());
}
