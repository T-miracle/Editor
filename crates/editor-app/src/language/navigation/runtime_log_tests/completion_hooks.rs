//! The real SDK worker may fail, while a separately authorized native transport remains usable.
use super::*;
use plugin_runtime::plugin_protocol::api::DocumentVersion;

/// Compose two published seams: a native stdio declaration and the independently built SDK component.
#[test]
#[ignore = "build-capability-example.ps1 and cargo build -p plugin-runtime --example lsp_fixture first"]
fn pure_completion_faults_preserve_native_results_and_process() {
    let directory = tempfile::tempdir().unwrap();
    let native = fixture_package(directory.path(), None);
    let sdk = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
    )
    .unwrap();
    // Keep ordinary guest declarations and assets: its normal instance still activates through the same gates.
    let mut manifest = serde_json::to_value(&sdk.manifest).unwrap();
    manifest["id"] = json!(OWNER);
    manifest["name"] = json!(native.manifest.name);
    manifest["version"] = json!(native.manifest.version);
    manifest["services"] = serde_json::to_value(&native.manifest.services).unwrap();
    manifest["language_servers"] = serde_json::to_value(&native.manifest.language_servers).unwrap();
    manifest["api"]["required"]["language.lsp"] = json!("^1");
    manifest["api"]["required"]["process"] = json!("^1");
    manifest["api"]["optional"]
        .as_object_mut()
        .unwrap()
        .remove("language.completion");
    manifest["api"]["required"]["language.completion"] = json!("^1");
    manifest["permissions"]
        .as_array_mut()
        .unwrap()
        .push(json!("process.service.analysis"));
    manifest["language_servers"][0]["completion_hook"] = json!(true);
    let mut files = sdk.files;
    files.insert("plugin.toml".into(), native.files["plugin.toml"].clone());
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let package = resource_package(files);
    let (_manager, server) = installed_package(directory.path(), &package);
    server.prepare_until_ready().unwrap();
    let logs = server.service.runtime_logs();
    let document =
        server.open_document(file_uri(&directory.path().join("pure.unfamiliar")).unwrap());
    for (revision, source) in ["stale", "bad-range", "excessive"].into_iter().enumerate() {
        let before = logs.records(OWNER).len();
        let response = server
            .completions_at_version(
                document.clone(),
                source.into(),
                Position::new(0, source.len() as u32),
                DocumentVersion {
                    id: "pure-lsp-document".into(),
                    path: "pure.unfamiliar".into(),
                    revision: revision as u64,
                },
            )
            .unwrap();
        let CompletionResponse::Array(items) = response else {
            panic!("native fixture returns an array")
        };
        assert_eq!(
            items.len(),
            1,
            "invalid supplements cannot add or erase native candidates"
        );
        assert_eq!(items[0].label, "fixture-completion");
        assert_eq!(
            server.service.process_count(),
            1,
            "supplement faults must not retire native IO"
        );
        assert!(
            logs.records(OWNER)[before..].iter().any(
                |entry| entry.source == "language/completion" && entry.level == LogLevel::Error
            )
        );
    }
    // Response validation does not poison a healthy worker; its valid result merges after native items.
    let response = server
        .completions_at_version(
            document.clone(),
            "healthy".into(),
            Position::new(0, 7),
            DocumentVersion {
                id: "pure-lsp-document".into(),
                path: "pure.unfamiliar".into(),
                revision: 5,
            },
        )
        .unwrap();
    let CompletionResponse::Array(items) = response else {
        panic!("merged result is an array")
    };
    assert_eq!(items[0].label, "fixture-completion");
    assert_eq!(items[1].label, "pure-worker-denied-io");
    assert_eq!(server.service.process_count(), 1);
    // A CPU-budget trap pauses this isolated worker; the native transport remains fully usable afterwards.
    for (revision, source) in [(6, "fuel"), (7, "after-fuel")] {
        let response = server
            .completions_at_version(
                document.clone(),
                source.into(),
                Position::new(0, source.len() as u32),
                DocumentVersion {
                    id: "pure-lsp-document".into(),
                    path: "pure.unfamiliar".into(),
                    revision,
                },
            )
            .unwrap();
        let CompletionResponse::Array(items) = response else {
            panic!("native fixture returns an array")
        };
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "fixture-completion");
        assert_eq!(server.service.process_count(), 1);
    }
    assert!(
        logs.records(OWNER)
            .iter()
            .any(|entry| entry.source == "language/completion" && entry.level == LogLevel::Error)
    );
}
