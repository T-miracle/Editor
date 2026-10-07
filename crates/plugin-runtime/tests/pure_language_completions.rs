//! Real public SDK guest packages exercise pure authority, result validation and provider retirement.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{
        Environment,
        api::DocumentVersion,
        language::{CompletionDiagnostic, SourceSnapshot},
    },
};
use serde_json::json;
use std::{
    io::{Cursor, Write},
    path::PathBuf,
};

/// Repackage the formal independent fixture with a novel provider; package inspection remains the gate.
fn package(read: bool, capability: bool) -> anyhow::Result<Package> {
    let original = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
    )?;
    let mut manifest = serde_json::to_value(&original.manifest)?;
    manifest["api"]["required"]["language.lsp"] = json!("^1");
    manifest["api"]["required"]["process"] = json!("^1");
    manifest["api"]["optional"]
        .as_object_mut()
        .unwrap()
        .remove("language.completion");
    if capability {
        manifest["api"]["required"]["language.completion"] = json!("^1");
    }
    if !read {
        manifest["permissions"]
            .as_array_mut()
            .unwrap()
            .retain(|value| value != "editor.read");
    }
    manifest["permissions"]
        .as_array_mut()
        .unwrap()
        .push(json!("process.service.analysis"));
    manifest["services"] =
        json!({"analysis":{"program":std::env::current_exe()?.display().to_string()}});
    manifest["language_servers"] = json!([{"id":"analysis", "language":"novel-pure", "service":"analysis", "completion_hook":true}]);
    let mut files = original.files;
    files.insert("manifest.json".into(), serde_json::to_vec(&manifest)?);
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        archive.start_file(name, zip::write::SimpleFileOptions::default())?;
        archive.write_all(&bytes)?;
    }
    Package::from_bytes(&archive.finish()?.into_inner())
}

/// Version identity belongs to the native document; a request nonce cannot substitute for its revision.
fn snapshot(text: &str) -> SourceSnapshot {
    SourceSnapshot {
        document: DocumentVersion {
            id: "opened-pure-test".into(),
            path: "document.novel".into(),
            revision: 42,
        },
        text: text.into(),
    }
}

/// Permission/capability gates, actual denied IO, hostile replies and revocation share one installation.
#[test]
#[ignore = "build-capability-example.ps1 with the current host first; uses its real SDK WASM component"]
fn pure_language_completion_package_enforces_authority_and_source_identity() {
    assert!(
        package(false, true).is_err(),
        "source access requires editor.read"
    );
    assert!(
        package(true, false).is_err(),
        "hook use requires the declared capability"
    );
    let workspace = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        store.path().to_path_buf(),
        Environment {
            workspace: workspace.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let package = package(true, true).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let service = manager.language_services()["capability-example/analysis"]
        .as_ref()
        .unwrap()
        .clone();
    let first = service
        .complete_snapshot(
            snapshot("中文🙂"),
            "中文🙂".len(),
            "file:///C:/document.novel".into(),
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(first.document.revision, 42);
    assert_eq!(first.items[0].label, "pure-worker-denied-io");
    assert_eq!(
        first.items[0].new_text, "Discovered label",
        "pure work sees the resolved discovery configuration"
    );
    let next = service
        .complete_snapshot(
            snapshot("next"),
            4,
            "file:///C:/document.novel".into(),
            None,
        )
        .unwrap()
        .unwrap();
    assert!(next.request > first.request && next.document.revision == 42);
    for text in ["stale", "bad-range", "excessive"] {
        assert!(
            service
                .complete_snapshot(
                    snapshot(text),
                    text.len(),
                    "file:///C:/document.novel".into(),
                    None
                )
                .is_err(),
            "host accepted {text} reply"
        );
    }
    assert!(
        service
            .complete_snapshot(
                snapshot("中文"),
                1,
                "file:///C:/document.novel".into(),
                None
            )
            .is_err(),
        "split UTF-8 caret must fail before invocation"
    );
    assert!(
        service
            .complete_snapshot(
                snapshot(&"x".repeat(1024 * 1024 + 1)),
                0,
                "file:///C:/document.novel".into(),
                None
            )
            .is_err()
    );
    // Native summaries and logical identities have their own quotas before any guest sees them.
    for (uri, diagnostics) in [
        ("file:///C:/document.novel\n".into(), None),
        (
            "file:///C:/document.novel".into(),
            Some(vec![CompletionDiagnostic {
                code: Some(json!(true)),
                message: "invalid code".into(),
            }]),
        ),
        (
            "file:///C:/document.novel".into(),
            Some(vec![CompletionDiagnostic {
                code: Some(json!("x".repeat(129))),
                message: "oversized code".into(),
            }]),
        ),
        (
            "file:///C:/document.novel".into(),
            Some(vec![CompletionDiagnostic {
                code: None,
                message: "x".repeat(1025),
            }]),
        ),
        (
            "file:///C:/document.novel".into(),
            Some(vec![
                CompletionDiagnostic {
                    code: Some(json!(7)),
                    message: "bounded".into(),
                };
                129
            ]),
        ),
    ] {
        assert!(
            service
                .complete_snapshot(snapshot("quota"), 5, uri, diagnostics)
                .is_err(),
            "invalid native context must be rejected"
        );
    }
    assert!(
        service
            .complete_snapshot(
                snapshot("valid preview"),
                13,
                "file:///C:/document.novel".into(),
                Some(vec![CompletionDiagnostic {
                    code: Some(json!(7)),
                    message: "当前源码🙂".into(),
                }]),
            )
            .is_ok(),
        "bounded standard diagnostics remain ordinary readonly input"
    );
    assert_eq!(
        service.process_count(),
        0,
        "snapshot work must never start a native process"
    );
    // Settings used only by a pure hook still invalidate its lease, even when the native command is identical.
    manager
        .update_setting(
            "capability-example",
            plugin_runtime::plugin_protocol::settings::Scope::Project,
            "label",
            Some(json!("updated")),
        )
        .unwrap();
    assert!(!service.is_active());
    let service = manager.language_services()["capability-example/analysis"]
        .as_ref()
        .unwrap()
        .clone();
    assert_eq!(
        service
            .complete_snapshot(
                snapshot("new setting"),
                11,
                "file:///C:/document.novel".into(),
                None
            )
            .unwrap()
            .unwrap()
            .items[0]
            .new_text,
        "updated"
    );
    manager.disable("capability-example").unwrap();
    assert!(
        service
            .complete_snapshot(
                snapshot("next"),
                4,
                "file:///C:/document.novel".into(),
                None
            )
            .is_err(),
        "a borrowed lease cannot outlive revocation"
    );
}
