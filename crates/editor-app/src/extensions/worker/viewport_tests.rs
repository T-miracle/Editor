//! Exercise delayed source measurements through the real worker and delivered guest.
use super::*;

/// Poll a published outcome with a finite deadline; the production actor owns all guest calls.
fn wait_for(worker: &Worker, ready: impl Fn(&Published) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if ready(&worker.state.lock().unwrap()) {
            return;
        }
        assert!(Instant::now() < deadline, "viewport worker timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A new preview can overtake the previous frame's source measurement in the actor queue.
/// Reject that obsolete input without a user-facing error, but retain malformed-input errors.
#[test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn stale_source_viewport_is_discarded_without_plugin_error() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("plugins");
    let environment = Environment {
        workspace: directory.path().display().to_string(),
        ..Default::default()
    };
    let package = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/markdown.zip"),
    )
    .unwrap();
    let mut manager = Manager::open(root.clone(), environment.clone()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    drop(manager);
    let worker = Worker::start_background(root, environment, true);
    wait_for(&worker, |state| {
        state.instance_epochs.contains_key("markdown")
    });
    let epoch = worker.state.lock().unwrap().instance_epochs["markdown"];
    let send = |event| {
        worker
            .tx
            .send(Work::Event(
                "markdown".into(),
                epoch,
                Some("preview".into()),
                event,
            ))
            .unwrap()
    };
    let document = api::DocumentVersion {
        id: "document".into(),
        path: "notes.md".into(),
        revision: 1,
    };
    send(api::Notification::Preview {
        document: Some(document.clone()),
        text: "# First\n\nParagraph".into(),
    });
    wait_for(&worker, |state| {
        state
            .views
            .get("markdown/preview")
            .is_some_and(|scene| scene.source.as_ref() == Some(&document))
    });
    let revision = worker.state.lock().unwrap().views["markdown/preview"].revision;
    let next = api::DocumentVersion {
        revision: 2,
        ..document.clone()
    };
    send(api::Notification::Preview {
        document: Some(next.clone()),
        text: "# First\n\nParagraph edited".into(),
    });
    wait_for(&worker, |state| {
        state
            .views
            .get("markdown/preview")
            .is_some_and(|scene| scene.source.as_ref() == Some(&next))
    });
    send(api::Notification::SourceViewport(api::SourceViewport {
        document: document.clone(),
        ui_revision: revision,
        offset: 0,
        line_fraction: 0.25,
        origin: None,
        layout: false,
    }));
    wait_for(&worker, |state| {
        state.logs.records("markdown").iter().any(|entry| {
            entry
                .message
                .contains("Viewport source or scene has changed")
        })
    });
    let (status, severity, current) = {
        let state = worker.state.lock().unwrap();
        (
            state.status.is_some(),
            state.logs.unread_severity("markdown"),
            state.views["markdown/preview"].source.clone(),
        )
    };
    assert!(
        !status,
        "obsolete source measurements must not surface as plugin failures"
    );
    assert_eq!(severity, None);
    assert_eq!(current.as_ref(), Some(&next));
    // Validation runs before revision checks: a zero origin is invalid even on an obsolete scene.
    send(api::Notification::SourceViewport(api::SourceViewport {
        document,
        ui_revision: revision,
        offset: 0,
        line_fraction: 0.25,
        origin: Some(0),
        layout: false,
    }));
    wait_for(&worker, |state| state.status.is_some());
    assert_eq!(
        worker
            .state
            .lock()
            .unwrap()
            .logs
            .unread_severity("markdown"),
        Some(plugin_runtime::logs::LogLevel::Error)
    );
    let (tx, rx) = futures::channel::oneshot::channel();
    worker.tx.send(Work::Shutdown(Some(tx))).unwrap();
    futures::executor::block_on(rx).unwrap();
}
