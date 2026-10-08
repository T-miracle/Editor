//! Windows registry replacement must tolerate a short-lived reader without losing the saved metadata.
use super::*;
use std::os::windows::fs::OpenOptionsExt;

/// Exercise actual startup persistence with a reader that briefly denies Windows delete sharing.
#[test]
fn startup_registry_replacement_survives_transient_windows_reader() {
    let directory = tempfile::tempdir().unwrap();
    let registry = directory.path().join("registry.json");
    std::fs::write(&registry, b"{}").unwrap();
    let reader = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1 | 2)
        .open(&registry)
        .unwrap();
    let release = std::thread::spawn(move || {
        // Match a background reader that closes shortly after startup attempts its atomic replacement.
        std::thread::sleep(std::time::Duration::from_millis(60));
        drop(reader);
    });
    let result = Manager::open_with_trust(directory.path().into(), Environment::default(), false);
    release.join().unwrap();
    assert!(
        result.is_ok(),
        "startup registry replacement failed: {:#}",
        result.err().unwrap()
    );
    assert_eq!(std::fs::read(&registry).unwrap(), b"{}");
}
