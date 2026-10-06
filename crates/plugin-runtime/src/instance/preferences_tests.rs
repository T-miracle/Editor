//! Scoped disk behavior verifies races, corruption and rollback without granting test-only authority.
use super::*;
use std::sync::{Arc, Barrier};

fn key() -> PreferenceKey {
    PreferenceKey {
        file_type: "md".into(),
        name: "display".into(),
    }
}

/// Concurrent live instances using the same record must observe one winner and one typed conflict.
#[test]
fn compare_and_set_prevents_lost_updates_and_preserves_revision() {
    let directory = tempfile::tempdir().unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let results = std::thread::scope(|scope| {
        let tasks = [0, 1].map(|mode| {
            let root = directory.path();
            let barrier = barrier.clone();
            scope.spawn(move || {
                barrier.wait();
                compare_and_set(root, 8192, &mut None, &key(), 0, serde_json::json!(mode))
            })
        });
        tasks.map(|task| task.join().unwrap())
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results.iter().find_map(|r| r.as_ref().err()).unwrap().code,
        ErrorCode::Conflict
    );
    let value = read(directory.path(), &None, &key()).unwrap();
    assert_eq!(value.revision, 1);
    let same = compare_and_set(
        directory.path(),
        8192,
        &mut None,
        &key(),
        1,
        value.data.clone().unwrap(),
    )
    .unwrap();
    assert_eq!(same, value);
    assert_eq!(
        compare_and_set(
            directory.path(),
            8192,
            &mut None,
            &key(),
            0,
            serde_json::json!(2)
        )
        .unwrap_err()
        .code,
        ErrorCode::Conflict
    );
    let next = compare_and_set(
        directory.path(),
        8192,
        &mut None,
        &key(),
        1,
        serde_json::json!(3),
    )
    .unwrap();
    assert_eq!(next.revision, 2);
    assert_eq!(read(directory.path(), &None, &key()).unwrap(), next);
}

/// Distinct owner/workspace directories and file types cannot inherit each other's persisted intent.
#[test]
fn preferences_are_scoped_and_corrupt_records_remain_untouched() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    compare_and_set(
        first.path(),
        8192,
        &mut None,
        &key(),
        0,
        serde_json::json!(2),
    )
    .unwrap();
    assert_eq!(
        read(second.path(), &None, &key()).unwrap(),
        PreferenceValue::default()
    );
    let png = PreferenceKey {
        file_type: "png".into(),
        ..key()
    };
    assert_eq!(
        read(first.path(), &None, &png).unwrap(),
        PreferenceValue::default()
    );
    let path = first.path().join(record_path(&key()));
    for corrupt in [
        b"invalid JSON".as_slice(),
        br#"{"revision":0,"data":2}"#.as_slice(),
    ] {
        std::fs::write(&path, corrupt).unwrap();
        assert_eq!(
            read(first.path(), &None, &key()).unwrap_err().code,
            ErrorCode::InvalidState
        );
        assert!(
            compare_and_set(
                first.path(),
                8192,
                &mut None,
                &key(),
                0,
                serde_json::json!(0)
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), corrupt);
    }
}

/// Preference writes share other private-file quotas and staging; failures cannot mutate disk.
#[test]
fn preference_quotas_and_staged_rollback_share_private_file_rules() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("existing"), [9; 100]).unwrap();
    assert_eq!(
        compare_and_set(
            directory.path(),
            110,
            &mut None,
            &key(),
            0,
            serde_json::json!(1)
        )
        .unwrap_err()
        .code,
        ErrorCode::LimitExceeded
    );
    assert_eq!(
        read(directory.path(), &None, &key()).unwrap(),
        PreferenceValue::default()
    );
    let mut staged = Some(Default::default());
    let value = compare_and_set(
        directory.path(),
        8192,
        &mut staged,
        &key(),
        0,
        serde_json::json!(1),
    )
    .unwrap();
    assert_eq!(read(directory.path(), &staged, &key()).unwrap(), value);
    assert_eq!(
        read(directory.path(), &None, &key()).unwrap(),
        PreferenceValue::default()
    );
    let previous = staged.clone();
    assert_eq!(
        compare_and_set(
            directory.path(),
            110,
            &mut staged,
            &key(),
            1,
            serde_json::json!(2)
        )
        .unwrap_err()
        .code,
        ErrorCode::LimitExceeded
    );
    assert_eq!(staged, previous);
    assert_eq!(
        std::fs::read(directory.path().join("existing")).unwrap(),
        [9; 100]
    );
}
