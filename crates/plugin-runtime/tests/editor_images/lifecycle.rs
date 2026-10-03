//! Lifecycle and cancellation regressions use the same independently built guest and public manager.
use super::*;

/// Continuous public events must revoke inputs immediately, without relying on the host polling cadence.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn image_input_withdrawal_and_source_replacement_revoke_before_the_next_public_event() {
    use plugin_runtime::plugin_protocol::settings::{EffectiveValue, Phase, Source};
    let mut host = Harness::new(&image_package(|_, _| {}));
    host.preview("image-input-fixture", Some(document(7)))
        .unwrap();
    let input = host
        .offer(
            "image-input-fixture",
            HostImageOrigin::Drop,
            vec![image(Arc::new(vec![1]))],
        )
        .unwrap()
        .pop()
        .unwrap();
    let incarnation = host
        .manager
        .instance_id("image-input-fixture")
        .unwrap()
        .to_string();
    // The existing example changes between its composable and plain native documents by configuration.
    // This normal SDK notification changes the published opt-in, without replacing the WASM incarnation.
    host.manager
        .event(
            "image-input-fixture",
            None,
            api::Notification::Configuration {
                phase: Phase::Apply,
                values: BTreeMap::from([(
                    "label".into(),
                    EffectiveValue {
                        value: json!("plain"),
                        source: Source::User,
                    },
                )]),
            },
        )
        .unwrap();
    assert_eq!(
        host.manager.instance_id("image-input-fixture"),
        Some(incarnation.as_str())
    );
    assert!(!host.manager.live["image-input-fixture"].views["welcome"].editor_image_input);
    let after_withdrawal = host.save("image-input-fixture", &input.handle, "img.png");

    host.manager
        .event(
            "image-input-fixture",
            None,
            api::Notification::Configuration {
                phase: Phase::Apply,
                values: BTreeMap::from([(
                    "label".into(),
                    EffectiveValue {
                        value: json!("composable-ui"),
                        source: Source::User,
                    },
                )]),
            },
        )
        .unwrap();
    let input = host
        .offer(
            "image-input-fixture",
            HostImageOrigin::Drop,
            vec![image(Arc::new(vec![2]))],
        )
        .unwrap()
        .pop()
        .unwrap();
    host.preview("image-input-fixture", Some(document(8)))
        .unwrap();
    let after_source_replacement = host.save("image-input-fixture", &input.handle, "img.png");
    assert!(
        matches!(
            after_withdrawal,
            Err(api::Failure {
                code: api::ErrorCode::InvalidHandle,
                ..
            })
        ),
        "Withdrawn declaration accepted a save before poll: {after_withdrawal:?}"
    );
    assert!(
        matches!(
            after_source_replacement,
            Err(api::Failure {
                code: api::ErrorCode::InvalidHandle,
                ..
            })
        ),
        "Replaced source accepted a save before poll: {after_source_replacement:?}"
    );
    assert!(
        host.manager
            .live
            .get_mut("image-input-fixture")
            .unwrap()
            .take_editor_requests()
            .is_empty()
    );
}

/// Preview advance revokes unused offers but never destroys a success receipt from an accepted writer.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn image_input_source_withdrawal_keeps_accepted_save_receipt_and_revokes_unused_handles() {
    let mut host = Harness::new(&image_package(|_, _| {}));
    host.preview("image-input-fixture", Some(document(7)))
        .unwrap();
    let inputs = host
        .offer(
            "image-input-fixture",
            HostImageOrigin::Drop,
            vec![image(Arc::new(vec![1])), image(Arc::new(vec![2]))],
        )
        .unwrap();
    host.save("image-input-fixture", &inputs[0].handle, "img.png")
        .unwrap();
    let accepted = host.take("image-input-fixture");
    assert!(accepted.begin() && accepted.enter_side_effect());
    host.preview("image-input-fixture", Some(document(8)))
        .unwrap();
    assert_eq!(
        // Cleanup probes do not supersede the example's current EditorTask; a new Save intent would.
        host.probe(
            "image-input-fixture",
            json!({"method":"close_resource","handle":inputs[1].handle})
        )
        .unwrap_err()
        .code,
        api::ErrorCode::InvalidHandle
    );
    accepted.finish(Ok(api::EditorValue::ImageSaved {
        input: inputs[0].handle.clone(),
        document: document(7),
        name: "img.png".into(),
    }));
    host.manager.poll();
    let receipt = text(&host.manager, "image-input-fixture");
    assert!(
        receipt.contains("ImageSaved") && receipt.contains("revision: 7"),
        "{receipt}"
    );
    assert_eq!(
        host.save("image-input-fixture", &inputs[1].handle, "img1.png")
            .unwrap_err()
            .code,
        api::ErrorCode::InvalidHandle
    );
    host.preview("image-input-fixture", Some(document(7)))
        .unwrap_err();
}

/// Closing and workspace parking invalidate capture slots, including before a replacement Preview arrives.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn image_input_close_withdraw_switch_and_reincarnation_cannot_revive_input() {
    let mut host = Harness::new(&image_package(|_, _| {}));
    host.preview("image-input-fixture", Some(document(7)))
        .unwrap();
    let input = host
        .offer(
            "image-input-fixture",
            HostImageOrigin::Drop,
            vec![image(Arc::new(vec![1]))],
        )
        .unwrap()
        .pop()
        .unwrap();
    host.manager.document_changed(api::DocumentChange {
        document: document(7),
        closed: true,
    });
    assert_eq!(
        host.save("image-input-fixture", &input.handle, "img.png")
            .unwrap_err()
            .code,
        api::ErrorCode::InvalidHandle
    );
    assert_eq!(
        code(
            host.offer(
                "image-input-fixture",
                HostImageOrigin::Drop,
                vec![image(Arc::new(vec![1]))]
            )
            .unwrap_err()
        ),
        api::ErrorCode::StaleRevision
    );
    host.preview("image-input-fixture", Some(document(8)))
        .unwrap();
    // Switch away and back keeps the same parked guest, but retires input authority immediately.
    host.manager
        .offer_image_input(
            "image-input-fixture",
            "welcome",
            document(8),
            api::TextRange { start: 0, end: 0 },
            HostImageOrigin::Drop,
            vec![image(Arc::new(vec![1]))],
        )
        .unwrap();
    let api::Notification::ImageInput { images, .. } =
        serde_json::from_str(&text(&host.manager, "image-input-fixture")).unwrap()
    else {
        panic!()
    };
    let old_workspace = host.manager.workspace().to_string();
    let other = host._root.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    host.manager
        .switch_workspace(
            Environment {
                workspace: other.to_string_lossy().into(),
                ..Environment::default()
            },
            true,
        )
        .unwrap();
    host.manager
        .switch_workspace(
            Environment {
                workspace: old_workspace,
                ..Environment::default()
            },
            true,
        )
        .unwrap();
    assert_eq!(
        host.save("image-input-fixture", &images[0].handle, "img.png")
            .unwrap_err()
            .code,
        api::ErrorCode::InvalidHandle
    );
    host.preview("image-input-fixture", None).unwrap();
    assert!(!host.manager.live["image-input-fixture"].views["welcome"].editor_image_input);
    host.manager.disable("image-input-fixture").unwrap();
    host.manager.enable("image-input-fixture").unwrap();
    assert_eq!(
        host.save("image-input-fixture", &images[0].handle, "img.png")
            .unwrap_err()
            .code,
        api::ErrorCode::InvalidHandle
    );
}

/// Unexecuted cancellation allows a retry; waiting cancellation cannot certify that native files were rolled back.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn image_input_cancelled_save_retries_only_before_the_side_effect_gate() {
    let mut host = Harness::new(&image_package(|_, _| {}));
    host.preview("image-input-fixture", Some(document(7)))
        .unwrap();
    let input = host
        .offer(
            "image-input-fixture",
            HostImageOrigin::Drop,
            vec![image(Arc::new(vec![1]))],
        )
        .unwrap()
        .pop()
        .unwrap();
    let api::Value::Accepted(handle) = host
        .save("image-input-fixture", &input.handle, "img.png")
        .unwrap()
    else {
        panic!()
    };
    let request = host.take("image-input-fixture");
    host.probe(
        "image-input-fixture",
        json!({"method":"cancel_request","handle":handle,"mode":"try_terminate"}),
    )
    .unwrap();
    host.manager.poll();
    assert!(!request.begin());
    let api::Value::Accepted(handle) = host
        .save("image-input-fixture", &input.handle, "img.png")
        .unwrap()
    else {
        panic!()
    };
    let request = host.take("image-input-fixture");
    assert!(request.begin() && request.enter_side_effect());
    host.probe(
        "image-input-fixture",
        json!({"method":"cancel_request","handle":handle,"mode":"try_terminate"}),
    )
    .unwrap();
    host.manager.poll();
    request.finish(Ok(api::EditorValue::ImageSaved {
        input: input.handle.clone(),
        document: document(7),
        name: "img.png".into(),
    }));
    assert!(matches!(
        request.status(),
        api::RequestUpdate::Cancelled {
            effect: api::CancellationEffect::WaitingStopped,
            ..
        }
    ));
    assert_eq!(
        host.save("image-input-fixture", &input.handle, "img1.png")
            .unwrap_err()
            .code,
        api::ErrorCode::Conflict
    );
}

/// Real monotonic expiry is exercised through the manager, without a test-only clock or host resource API.
#[test]
#[ignore = "build current capability-example through the host SDK first; waits for the public 30s input deadline"]
fn image_input_expiry_releases_pixels_and_invalidates_the_guest_handle() {
    let mut host = Harness::new(&image_package(|_, _| {}));
    host.preview("image-input-fixture", Some(document(7)))
        .unwrap();
    let bytes = Arc::new(vec![1]);
    let weak = Arc::downgrade(&bytes);
    let input = host
        .offer(
            "image-input-fixture",
            HostImageOrigin::Drop,
            vec![image(bytes.clone())],
        )
        .unwrap()
        .pop()
        .unwrap();
    drop(bytes);
    assert!(weak.upgrade().is_some());
    std::thread::sleep(std::time::Duration::from_millis(30_020));
    host.manager.poll();
    assert!(weak.upgrade().is_none());
    assert_eq!(
        host.save("image-input-fixture", &input.handle, "img.png")
            .unwrap_err()
            .code,
        api::ErrorCode::InvalidHandle
    );
}
