//! Explicit session cleanup survives native process shutdown even when UI entities are still retained.
use gpui_kit::{BackgroundExecutor, Task};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Default)]
pub(super) struct Resources {
    // Every path originates from a successful private tempfile allocation, never from a user-selected path.
    entries: Mutex<Vec<(PathBuf, Weak<AtomicBool>, Arc<AtomicBool>)>>,
}
/// Completion belongs to the worker future, including unwinding or cancellation before it returns an outcome.
pub(super) struct Lifetime(Arc<AtomicBool>);
impl Drop for Lifetime {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
impl Resources {
    /// Register an owned backup and its cancellation signal before any file operation starts.
    pub fn register(&self, backup: PathBuf, cancel: &Arc<AtomicBool>) -> Lifetime {
        let completed = Arc::new(AtomicBool::new(false));
        self.entries
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push((backup, Arc::downgrade(cancel), completed.clone()));
        Lifetime(completed)
    }
    /// A window with no owned backup can close immediately without scheduling a cleanup task.
    pub fn is_empty(&self) -> bool {
        self.entries
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_empty()
    }
    /// Stop workers and delete only registered private backups; GPUI awaits this task during normal shutdown.
    pub fn cleanup(&self, executor: &BackgroundExecutor) -> Task<()> {
        let entries = std::mem::take(
            &mut *self
                .entries
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        );
        for (_, cancel, _) in &entries {
            if let Some(cancel) = cancel.upgrade() {
                cancel.store(true, Ordering::Relaxed);
            }
        }
        let timer = executor.clone();
        executor.spawn(async move {
            while entries.iter().any(|(_, _, completed)| !completed.load(Ordering::Acquire)) {
                timer.timer(std::time::Duration::from_millis(10)).await;
            }
            for (path, _, _) in entries {
                if let Err(error) = std::fs::remove_dir_all(&path)
                    && error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(%error, path = %path.display(), "file operation session backup cleanup failed");
                }
            }
        })
    }
}
