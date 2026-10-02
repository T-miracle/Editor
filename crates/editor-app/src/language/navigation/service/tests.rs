//! Native fixture requests exercise cancellation and blocked startup across provider retirement.
use super::*;

/// The actual fixture deliberately withholds a response so the public 30-second budget can be observed.
#[test]
#[ignore = "build SDK capability-example and lsp_fixture first; waits for the real request deadline"]
fn unanswered_lsp_request_sends_cancel_without_replaying_the_operation() {
    let directory = tempfile::tempdir().unwrap();
    let exe = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug/examples/lsp_fixture.exe")
        .canonicalize()
        .unwrap();
    let log = directory.path().join("wire.jsonl");
    let package = crate::extensions::lsp_tests::package(&exe, &log);
    let mut manager = plugin_runtime::Manager::open(
        directory.path().join("plugins"),
        plugin_runtime::plugin_protocol::Environment {
            workspace: directory.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let plan = manager.language_services()["capability-example/analysis"]
        .as_ref()
        .unwrap()
        .clone();
    let server = LanguageServer::from_service(plan).unwrap();
    server.prepare_until_ready().unwrap();
    let mut connection = server.connection.lock().unwrap();
    let connection = connection.as_mut().unwrap();
    let started = Instant::now();
    assert!(connection.request("fixture/pending", json!({})).is_err());
    assert!(
        started.elapsed() >= Duration::from_secs(29) && started.elapsed() < Duration::from_secs(35)
    );
    connection.request("fixture/barrier", json!({})).unwrap();
    let wire = std::fs::read_to_string(log).unwrap();
    assert!(wire.contains("$/cancelRequest"));
    assert_eq!(
        wire.lines()
            .filter(|line| line.contains("fixture/pending"))
            .count(),
        1
    );
}

#[test]
#[ignore = "build SDK capability-example and lsp_fixture first"]
fn retiring_provider_cancels_request_and_rejects_waiting_startup() {
    let directory = tempfile::tempdir().unwrap();
    let exe = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug/examples/lsp_fixture.exe")
        .canonicalize()
        .unwrap();
    let log = directory.path().join("wire.jsonl");
    let package = crate::extensions::lsp_tests::package(&exe, &log);
    let mut manager = plugin_runtime::Manager::open(
        directory.path().join("plugins"),
        plugin_runtime::plugin_protocol::Environment {
            workspace: directory.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let plan = manager.language_services()["capability-example/analysis"]
        .as_ref()
        .unwrap()
        .clone();
    let server = Arc::new(LanguageServer::from_service(plan.clone()).unwrap());
    server.prepare_until_ready().unwrap();
    let pending = server.clone();
    let request = std::thread::spawn(move || {
        pending
            .connection
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .request("fixture/pending", json!({}))
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while !std::fs::read_to_string(&log)
        .unwrap_or_default()
        .contains("fixture/pending")
    {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let pending = server.clone();
    let prepare = std::thread::spawn(move || pending.prepare());
    server.retire();
    assert!(request.join().unwrap().is_err());
    assert!(prepare.join().unwrap().is_err());
    assert!(server.prepare_until_ready().is_err());
    // Selecting the same still-installed plan gets a new adapter lifetime, never revives the old one.
    let replacement = LanguageServer::from_service(plan).unwrap();
    replacement.prepare_until_ready().unwrap();
    assert!(server.prepare().is_err());
    let uri = file_uri(&directory.path().join("reopened.novel")).unwrap();
    let old_document = replacement.open_document(uri.clone());
    replacement
        .diagnostics_for(old_document.clone(), "old")
        .unwrap();
    let delayed_close = replacement.retire_document(&uri).unwrap();
    let reopened = replacement.open_document(uri);
    replacement
        .diagnostics_for(reopened.clone(), "new")
        .unwrap();
    // Invert executor completion order: a late close must not remove the new wire lifetime.
    replacement.close_document(delayed_close).unwrap();
    let starts_before = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .filter(|line| line.contains("\"method\":\"initialize\""))
        .count();
    assert!(
        replacement
            .diagnostics_for(old_document.clone(), "stale")
            .is_err()
    );
    // Late saves are document-lifetime failures, never transport failures or retry-budget consumption.
    let recovery_before = replacement.recovery_status();
    for _ in 0..3 {
        assert!(
            replacement
                .document_saved_for(old_document.clone(), "stale save".into())
                .is_err()
        );
    }
    assert_eq!(replacement.recovery_status(), recovery_before);
    assert_eq!(
        replacement
            .definitions_for(reopened, "new".into(), Position::default())
            .unwrap()
            .len(),
        1
    );
    let wire = std::fs::read_to_string(&log).unwrap();
    assert_eq!(
        starts_before,
        wire.lines()
            .filter(|line| line.contains("\"method\":\"initialize\""))
            .count()
    );
    assert!(!wire.contains("stale"));
    assert_eq!(
        wire.lines()
            .filter(|line| line.contains("textDocument/didClose"))
            .count(),
        1
    );
    replacement.retire();
}
