//! Preparation observation checks use public process bytes and immutable resource ownership.
use super::*;

/// Split native reads must not corrupt Chinese text or combine bytes from different sources.
#[test]
fn utf8_output_is_decoded_per_process_and_stream() {
    let a = ResourceHandle {
        instance: "provider".into(),
        scope: "workspace".into(),
        resource: 1,
    };
    let b = ResourceHandle {
        resource: 2,
        ..a.clone()
    };
    let mut work = NativeWork::default();
    work.update(
        &a,
        &Update::Output {
            stream: plugin_protocol::process::Stream::Stdout,
            bytes: vec![0xe4, 0xb8],
        },
    );
    work.update(
        &b,
        &Update::Output {
            stream: plugin_protocol::process::Stream::Stdout,
            bytes: b"other".to_vec(),
        },
    );
    work.update(
        &a,
        &Update::Output {
            stream: plugin_protocol::process::Stream::Stderr,
            bytes: b"error".to_vec(),
        },
    );
    work.update(
        &a,
        &Update::Output {
            stream: plugin_protocol::process::Stream::Stdout,
            bytes: vec![0xad],
        },
    );
    assert_eq!(work.snapshot().output, "othererror中");
}

/// A lost native cleanup observation must end a preparation as failed without claiming tree exit.
#[test]
fn failed_cleanup_seals_the_receipt_without_fabricating_exit() {
    let root = Arc::new(AtomicBool::new(true));
    let work = Arc::new(Mutex::new(NativeWork::default()));
    let registry = PreparationRegistry::default();
    registry.register(&root, &work);
    let handle = ResourceHandle {
        instance: "provider".into(),
        scope: "fixture".into(),
        resource: 1,
    };
    registry.opened(&[root.clone()], &handle);
    let mut observer = registry.closing(&[root], &handle);
    observer(Err("EOF or job observation unavailable".into()));
    let work = work.lock().unwrap();
    assert_eq!(work.snapshot().state, crate::ExecutionState::Failed);
    assert!(
        !work.drained(),
        "failure cannot certify actual process cleanup"
    );
    assert!(work.failure().unwrap().contains("EOF"));
}
