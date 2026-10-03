//! One bounded background preparation owns its unpublished candidate until the actor accepts the result.
use super::*;

pub(super) struct BackgroundPreparation {
    pub id: String,
    pub control: plugin_runtime::InstallControl,
    ready: mpsc::Receiver<anyhow::Result<plugin_runtime::PreparedInstallation>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl BackgroundPreparation {
    /// Move only immutable inputs and the isolated candidate to another thread, never the live manager.
    pub fn start(
        id: String,
        job: plugin_runtime::InstallationPreparation,
        control: plugin_runtime::InstallControl,
    ) -> anyhow::Result<Self> {
        let (send, ready) = mpsc::sync_channel(1);
        let progress = control.clone();
        let thread = std::thread::Builder::new()
            .name("plugin-install-preparation".into())
            .spawn(move || {
                // A disconnected actor drops the result and therefore its uncommitted transaction.
                let _ = send.send(job.run(&progress));
            })?;
        Ok(Self {
            id,
            control,
            ready,
            thread: Some(thread),
        })
    }

    /// Polling cannot wait on download or compilation; a panicked helper becomes an explicit install failure.
    pub fn try_ready(&mut self) -> Option<anyhow::Result<plugin_runtime::PreparedInstallation>> {
        match self.ready.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(anyhow::anyhow!(
                "Plugin preparation worker stopped before returning its result"
            ))),
        }
    }

    /// Call only after completion: removing the handle distinguishes normal transfer from cancellation.
    pub fn join_completed(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for BackgroundPreparation {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.control.cancel();
            // Shutdown owns no live UI work here; cleanup must finish before the manager drops its owner lock.
            self.join_completed();
        }
    }
}
