//! Startup sharing conflicts must recover before the real actor publishes a fatal manager toast.
use super::*;
use std::os::windows::fs::OpenOptionsExt;

/// Observe the production actor's completion channel and published UI status, rather than a second manager.
#[test]
fn plugin_manager_startup_recovers_from_transient_registry_reader() {
    let directory = tempfile::tempdir().unwrap();
    let registry = directory.path().join("registry.json");
    std::fs::write(&registry, b"{}").unwrap();
    let reader = std::fs::OpenOptions::new()
        .read(true)
        // Keep READ/WRITE sharing while briefly denying the rename's DELETE access.
        .share_mode(1 | 2)
        .open(&registry)
        .unwrap();
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(60));
        drop(reader);
    });
    let worker = Worker::start_background(directory.path().into(), Environment::default(), false);
    let (acknowledge, mut completed) = futures::channel::oneshot::channel();
    worker.tx.send(Work::Shutdown(Some(acknowledge))).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let succeeded = loop {
        match completed.try_recv() {
            Ok(Some(())) => break true,
            Err(_) => break false,
            Ok(None) if Instant::now() >= deadline => break false,
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    release.join().unwrap();
    let published = worker.state.lock().unwrap();
    let status = published.status.as_ref().map(|status| &status.message);
    assert!(
        succeeded,
        "plugin manager actor did not recover: {status:?}"
    );
    assert!(
        status.is_none(),
        "startup published a fatal toast: {status:?}"
    );
    assert_eq!(std::fs::read(&registry).unwrap(), b"{}");
}
