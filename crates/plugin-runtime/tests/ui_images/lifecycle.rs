//! Real package lifecycle gates invalidate image consumers before delayed IO can publish.
use super::*;

/// A late body cannot replace a freshly loaded image from a newer source revision with the same node/URI.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn ui_images_revision_changes_discard_late_http_completion() {
    let server = PausedServer::new();
    let mut harness = Harness::new(vec![image("picture", &server.url())], &["network.images"]);
    harness.preview(1);
    server.accepted(&mut harness.manager);
    let old = harness.manager.image_resources()["images-fixture/welcome/image/picture"].clone();
    assert!(matches!(old.state, ImageState::Loading));
    harness.manager.document_changed(api::DocumentChange {
        document: Harness::source(2),
        closed: false,
    });
    assert!(
        harness.manager.image_resources().is_empty(),
        "old UI resurrected a retired revision"
    );
    harness.preview(2);
    let resources = harness.settle(5);
    assert_eq!(ready(&resources, "picture"), b"fresh-image");
    server.release();
    std::thread::sleep(Duration::from_millis(100));
    let resources = harness.manager.image_resources();
    assert_eq!(
        resources["images-fixture/welcome/image/picture"].source,
        Harness::source(2)
    );
    assert_eq!(ready(&resources, "picture"), b"fresh-image");
    assert!(
        matches!(old.state, ImageState::Loading),
        "retired publication was mutated in place"
    );
}

/// Save As can change a source path without editing text; relative resources must retire on that identity change.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn ui_images_document_path_change_retires_same_revision_resources() {
    let mut harness = Harness::new(vec![image("picture", "photo.png")], &["workspace.read"]);
    std::fs::write(harness.workspace.join("notes/photo.png"), b"old-location").unwrap();
    std::fs::create_dir(harness.workspace.join("moved")).unwrap();
    std::fs::write(harness.workspace.join("moved/photo.png"), b"new-location").unwrap();
    harness.preview(1);
    assert_eq!(ready(&harness.settle(5), "picture"), b"old-location");
    let mut moved = Harness::source(1);
    moved.path = "moved/source.sample".into();
    harness.manager.document_changed(api::DocumentChange {
        document: moved.clone(),
        closed: false,
    });
    assert!(
        harness.manager.image_resources().is_empty(),
        "same-revision path change retained old relative bytes"
    );
    harness
        .manager
        .event(
            "images-fixture",
            Some("welcome".into()),
            api::Notification::Preview {
                document: Some(moved.clone()),
                text: "Unsaved image markup".into(),
            },
        )
        .unwrap();
    let resources = harness.settle(5);
    assert_eq!(
        resources["images-fixture/welcome/image/picture"].source,
        moved
    );
    assert_eq!(ready(&resources, "picture"), b"new-location");
}

/// Closing a document retires its images before a guest can emit the next empty preview.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn ui_images_document_close_and_disable_cancel_consumers() {
    let server = PausedServer::with_paused(2);
    let mut harness = Harness::new(vec![image("picture", &server.url())], &["network.images"]);
    harness.preview(1);
    server.accepted(&mut harness.manager);
    harness.manager.document_changed(api::DocumentChange {
        document: Harness::source(1),
        closed: true,
    });
    assert!(harness.manager.image_resources().is_empty());
    harness
        .manager
        .event(
            "images-fixture",
            Some("welcome".into()),
            api::Notification::Preview {
                document: None,
                text: String::new(),
            },
        )
        .unwrap();
    assert!(harness.manager.image_resources().is_empty());
    // A second active HTTP body is independently retired by disabling the instance itself.
    harness.preview(2);
    server.accepted(&mut harness.manager);
    let old_incarnation = harness
        .manager
        .instance_id("images-fixture")
        .unwrap()
        .to_owned();
    harness.manager.disable("images-fixture").unwrap();
    server.release();
    server.release();
    assert!(harness.manager.image_resources().is_empty());
    harness.manager.enable("images-fixture").unwrap();
    assert_ne!(
        harness.manager.instance_id("images-fixture"),
        Some(old_incarnation.as_str())
    );
    harness.preview(3);
    assert_eq!(ready(&harness.settle(5), "picture"), b"fresh-image");
}

/// Parked instances do not publish a late image into the selected workspace or borrow its root.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn ui_images_workspace_switch_retires_old_publication() {
    let server = PausedServer::new();
    let mut harness = Harness::new(vec![image("picture", &server.url())], &["network.images"]);
    harness.preview(1);
    server.accepted(&mut harness.manager);
    let other = harness._root.path().join("other-workspace");
    std::fs::create_dir(&other).unwrap();
    harness
        .manager
        .switch_workspace(
            Environment {
                workspace: other.display().to_string(),
                ..Default::default()
            },
            true,
        )
        .unwrap();
    server.release();
    assert!(harness.manager.image_resources().is_empty());
    harness
        .manager
        .switch_workspace(
            Environment {
                workspace: harness.workspace.display().to_string(),
                ..Default::default()
            },
            true,
        )
        .unwrap();
    assert_eq!(ready(&harness.settle(5), "picture"), b"fresh-image");
}

/// The production 30-second deadline seals failure before a delayed HTTP body can publish success.
#[test]
#[ignore = "build current capability-example through the host SDK first; exercises the real 30s deadline"]
fn ui_images_http_deadline_is_terminal_for_late_bodies() {
    let server = PausedServer::new();
    let mut harness = Harness::new(vec![image("picture", &server.url())], &["network.images"]);
    harness.preview(1);
    server.accepted(&mut harness.manager);
    let start = Instant::now();
    failed(&harness.settle(35), "picture", api::ErrorCode::TimedOut);
    assert!(start.elapsed() < Duration::from_secs(34));
    server.release();
    std::thread::sleep(Duration::from_millis(100));
    failed(
        &harness.manager.image_resources(),
        "picture",
        api::ErrorCode::TimedOut,
    );
    // A different source version owns a new request rather than reviving the timed-out receiver.
    harness.preview(2);
    assert_eq!(ready(&harness.settle(5), "picture"), b"fresh-image");
}
