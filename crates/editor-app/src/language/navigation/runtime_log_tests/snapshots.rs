//! Unversioned pushes must identify an immutable wire snapshot before reaching a live document.
use super::*;

/// Unknown language packages opt in through the same public declaration as real XML.
#[test]
#[ignore = "build cargo build -p plugin-runtime --example lsp_fixture first"]
fn installed_snapshot_service_rejects_late_unversioned_diagnostics() {
    let directory = tempfile::tempdir().unwrap();
    let mut files = fixture_package(directory.path(), None).files;
    let mut manifest: Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["api"]["required"]["language.lsp"] = ">=1.3,<2".into();
    manifest["language_servers"][0]["diagnostic_snapshots"] = true.into();
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let package = resource_package(files);
    let (mut manager, server) = installed_package(directory.path(), &package);
    server.prepare_until_ready().unwrap();
    let uri = file_uri(&directory.path().join("draft.unfamiliar")).unwrap();
    let first = server.open_document(uri.clone());
    // A real request flushes preceding didOpen and native pushes through the existing transport.
    let locations = server
        .definitions_for(first.clone(), "first".into(), Position::default())
        .unwrap();
    assert_eq!(locations[0].target_uri, uri);
    let old_wire = latest_wire(directory.path());
    assert_ne!(old_wire, uri.as_str());
    publish(&server, &old_wire, "first snapshot");
    assert_eq!(
        messages(&server, first.clone(), "first"),
        ["first snapshot"]
    );

    server
        .definitions_for(first.clone(), "second".into(), Position::default())
        .unwrap();
    let current_wire = latest_wire(directory.path());
    assert_ne!(current_wire, old_wire);
    publish(&server, &current_wire, "current snapshot");
    publish(&server, &old_wire, "late old snapshot");
    publish(&server, uri.as_str(), "unproven canonical push");
    assert_eq!(
        messages(&server, first.clone(), "second"),
        ["current snapshot"]
    );

    let retired = server.retire_document(&uri).unwrap();
    server.close_document(retired).unwrap();
    let reopened = server.open_document(uri.clone());
    server
        .definitions_for(reopened.clone(), "second".into(), Position::default())
        .unwrap();
    let reopened_wire = latest_wire(directory.path());
    assert_ne!(reopened_wire, current_wire);
    publish(&server, &reopened_wire, "reopened snapshot");
    publish(&server, &current_wire, "late closed snapshot");
    // A delayed close belongs to the old lifetime and cannot remove the reopened mapping.
    server.close_document(first).unwrap();
    assert_eq!(
        messages(&server, reopened.clone(), "second"),
        ["reopened snapshot"]
    );
    manager.disable(OWNER).unwrap();
    assert!(!server.is_active());
    assert!(server.diagnostics_for(reopened, "second").is_err());
}

/// Read the observed native wire instead of deriving a URI from the implementation under test.
fn latest_wire(root: &Path) -> String {
    std::fs::read_to_string(root.join("wire.jsonl"))
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|message| message["method"] == "textDocument/didOpen")
        .last()
        .unwrap()["params"]["textDocument"]["uri"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// The controllable native service emits actual queued notifications with no version metadata.
fn publish(server: &LanguageServer, uri: &str, message: &str) {
    server
        .connection
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .request(
            "fixture/notifications",
            json!({"notifications":[{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics",
        "params":{"uri":uri,"diagnostics":[{"range":{"start":{"line":0,"character":0},
        "end":{"line":0,"character":1}},"message":message}]}}]}),
        )
        .unwrap();
}

/// Observe the public document result after its lifetime and immutable source have been validated.
fn messages(server: &LanguageServer, document: DocumentLease, source: &str) -> Vec<String> {
    server
        .diagnostics_for(document, source)
        .unwrap()
        .unwrap()
        .into_iter()
        .map(|diagnostic| diagnostic.message)
        .collect()
}
