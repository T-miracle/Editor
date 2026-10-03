//! Regression coverage for asynchronous publications and real Rust semantic errors.

use super::*;

/// A delayed push from a closed document must not match the next lifetime of its URI.
#[test]
fn reopened_document_rejects_old_lifetime_diagnostics() {
    let uri: Uri = "file:///unknown.novel".parse().unwrap();
    let mut store = DiagnosticsStore::default();
    store.synchronized(uri.as_str().into(), "old".into(), 1);
    store.close(uri.as_str());
    let version = store.next_version(uri.as_str(), "new").unwrap();
    assert!(version > 1);
    store.synchronized(uri.as_str().into(), "new".into(), version);
    store.publish(PublishDiagnosticsParams::new(
        uri,
        vec![Diagnostic::new_simple(
            lsp_types::Range::default(),
            "stale".into(),
        )],
        Some(1),
    ));
    assert!(store.snapshot("file:///unknown.novel", "new").is_none());
}

/// A delayed publication must not overwrite a newer version or another document.
#[test]
fn diagnostic_versions_and_empty_publications() {
    let uri: Uri = "file:///sample.rs".parse().unwrap();
    let mut store = DiagnosticsStore::default();
    store.synchronized(uri.as_str().into(), "old".into(), 1);
    let error = Diagnostic::new_simple(
        lsp_types::Range::default(),
        "cannot find function missing".into(),
    );
    store.publish(PublishDiagnosticsParams::new(
        uri.clone(),
        vec![error.clone()],
        Some(1),
    ));
    assert_eq!(store.snapshot(uri.as_str(), "old").unwrap().len(), 1);
    assert_eq!(store.next_version(uri.as_str(), "old"), None);
    assert_eq!(store.next_version(uri.as_str(), "new"), Some(2));
    store.synchronized(uri.as_str().into(), "new".into(), 2);
    store.publish(PublishDiagnosticsParams::new(
        uri.clone(),
        vec![error.clone()],
        Some(1),
    ));
    assert!(store.snapshot(uri.as_str(), "new").is_none());
    store.publish(PublishDiagnosticsParams::new(
        uri.clone(),
        vec![error],
        Some(2),
    ));
    assert_eq!(store.snapshot(uri.as_str(), "new").unwrap().len(), 1);
    assert!(store.snapshot(uri.as_str(), "old").is_none());
    store.publish(PublishDiagnosticsParams::new(
        uri.clone(),
        Vec::new(),
        Some(2),
    ));
    assert_eq!(store.snapshot(uri.as_str(), "new"), Some(Vec::new()));
}

/// Language servers omitting versions still replace their current snapshot's diagnostics.
#[test]
fn versionless_diagnostics_are_received() {
    let uri: Uri = "file:///sample.rs".parse().unwrap();
    let mut store = DiagnosticsStore::default();
    store.synchronized(uri.as_str().into(), "source".into(), 1);
    store.publish(PublishDiagnosticsParams::new(
        uri.clone(),
        vec![Diagnostic::new_simple(
            lsp_types::Range::default(),
            "type mismatch".into(),
        )],
        None,
    ));
    assert_eq!(
        store.snapshot(uri.as_str(), "source").unwrap()[0].message,
        "type mismatch"
    );
}

/// rust-analyzer may lowercase and percent-encode a Windows drive in published URIs.
#[test]
#[cfg(target_os = "windows")]
fn windows_diagnostic_uri_matches_document() {
    // Normalize outgoing workspace roots as well as matching incoming publications.
    assert_eq!(
        file_uri(Path::new(r"C:\Project\hello world.rs"))
            .unwrap()
            .as_str(),
        "file:///c:/Project/hello%20world.rs"
    );
    let mut store = DiagnosticsStore::default();
    let opened = "file:///C:/Project/hello%20world.rs";
    store.synchronized(opened.into(), "source".into(), 1);
    store.publish(PublishDiagnosticsParams::new(
        "file:///c%3A/Project/hello%20world.rs".parse().unwrap(),
        vec![Diagnostic::new_simple(
            lsp_types::Range::default(),
            "unresolved function".into(),
        )],
        Some(1),
    ));
    assert_eq!(store.snapshot(opened, "source").unwrap().len(), 1);
}

/// Confirm the actual installed server reports all three semantic errors and clears a correction.
#[test]
#[ignore = "requires a locally installed rust-analyzer and Rust toolchain"]
fn local_rust_semantic_diagnostics() {
    rust_semantic_fixture(true);
}

/// didChange alone must report and clear errors while the valid disk file stays untouched.
#[test]
#[ignore = "requires a locally installed rust-analyzer and Rust toolchain"]
fn local_rust_unsaved_semantic_diagnostics() {
    rust_semantic_fixture(false);
}

/// Exercise the same production transport with either saved or buffer-only revisions.
fn rust_semantic_fixture(save: bool) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("src")).unwrap();
    std::fs::write(
        directory.path().join("Cargo.toml"),
        "[package]\nname = \"diagnostic_fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    let path = directory.path().join("src/main.rs");
    let broken = "// Valid syntax with three semantic errors.\nfn main() { let value: bool = 42; missing_function(); let _: Typoo = value; }\n";
    let corrected = "// Corrected fixture.\nfn main() { let _value: bool = true; }\n";
    std::fs::write(&path, corrected).unwrap();
    let (_storage, _manager, server) =
        crate::language::navigation::readiness_tests::installed_rust_server(directory.path());
    server.prepare_until_ready().unwrap();
    let uri = file_uri(&path).unwrap();
    // First synchronize valid text so the next analysis must follow didChange.
    // A clean initial document need not produce a push until its diagnostics change.
    server.diagnostics(uri.clone(), corrected).unwrap();
    if save {
        std::fs::write(&path, broken).unwrap();
        server.document_saved(uri.clone(), broken.into()).unwrap();
    }
    server.diagnostics(uri.clone(), broken).unwrap();
    let errors = wait_for(&server, &uri, broken, |items| {
        let messages = items
            .iter()
            .map(|item| item.message.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if save {
            messages.contains("bool")
                && messages.contains("missing_function")
                && messages.contains("Typoo")
        } else {
            // Native unresolved-name messages omit the identifier; verify its
            // location rather than requiring rustc's save-only message wording.
            let start = broken
                .lines()
                .nth(1)
                .unwrap()
                .find("missing_function")
                .unwrap() as u32;
            messages.contains("bool")
                && items.iter().any(|item| {
                    item.range.start.line == 1
                        && item.range.start.character == start
                        && item.severity == Some(lsp_types::DiagnosticSeverity::ERROR)
                })
        }
    });
    assert!(errors.len() >= if save { 3 } else { 2 }, "{errors:?}");
    if save {
        std::fs::write(&path, corrected).unwrap();
        server
            .document_saved(uri.clone(), corrected.into())
            .unwrap();
    } else {
        assert_eq!(std::fs::read_to_string(&path).unwrap(), corrected);
    }
    wait_for(&server, &uri, corrected, |items| {
        items
            .iter()
            .all(|item| item.severity != Some(lsp_types::DiagnosticSeverity::ERROR))
    });
}

/// Poll without synthetic hover requests, matching an idle editor receiving server pushes.
fn wait_for(
    server: &LanguageServer,
    uri: &Uri,
    source: &str,
    ready: impl Fn(&[Diagnostic]) -> bool,
) -> Vec<Diagnostic> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut last = None;
    while Instant::now() < deadline {
        if let Some(items) = server.diagnostics(uri.clone(), source).unwrap() {
            if ready(&items) {
                return items;
            }
            last = Some(items);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("semantic diagnostics timed out: {last:?}");
}
