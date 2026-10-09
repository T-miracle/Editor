//! Native resource revocation retains the actual tree/EOF observation independently of guest handles.
use super::*;

impl Processes {
    /// Revoke input now; a bounded background reaper drains final output before reporting termination.
    /// The retained callback is authenticated by the original resource context, not by a new caller.
    pub(crate) fn close_observed(
        &mut self,
        id: u64,
        mut observe: impl FnMut(Result<Update, String>) + Send + 'static,
    ) -> anyhow::Result<()> {
        let Some(mut process) = self.items.remove(&id) else {
            return Ok(());
        };
        #[cfg(windows)]
        let termination = process._job.try_terminate();
        #[cfg(not(windows))]
        let termination = process.child.kill().map_err(anyhow::Error::from);
        if let Err(error) = termination {
            observe(Err(format!("{error:#}")));
            return Err(error);
        }
        process.input.take();
        if let Some(master) = process.master.take() {
            std::thread::spawn(move || drop(master));
        }
        self.reapers.push(std::thread::spawn(move || {
            let outcome = (|| -> anyhow::Result<()> {
                let deadline = Instant::now() + Duration::from_secs(10);
                let mut eof = false;
                loop {
                    if !eof {
                        match process.output.recv_timeout(Duration::from_millis(10)) {
                            Ok((stream, bytes)) => observe(Ok(Update::Output { stream, bytes })),
                            Err(mpsc::RecvTimeoutError::Disconnected) => eof = true,
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                        }
                    }
                    if eof && process.child.try_wait()?.is_some() {
                        #[cfg(windows)]
                        process._job.terminate_and_wait()?;
                        observe(Ok(Update::Terminated));
                        return Ok(());
                    }
                    anyhow::ensure!(
                        Instant::now() < deadline,
                        "Native tree/EOF cleanup could not be confirmed"
                    );
                    if eof {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
            })();
            if let Err(error) = &outcome {
                observe(Err(format!("{error:#}")));
            }
            outcome
        }));
        Ok(())
    }

    /// Reclaim completed reaper slots so repeated close/open cannot grow memory or bypass quotas.
    pub(crate) fn reap_finished(&mut self) {
        let mut pending = Vec::new();
        for reaper in self.reapers.drain(..) {
            if reaper.is_finished() {
                if let Ok(Err(error)) = reaper.join() {
                    eprintln!("Native cleanup failed: {error:#}");
                }
            } else {
                pending.push(reaper);
            }
        }
        self.reapers = pending;
    }
    /// Lifecycle retirement waits for already revoked resources too, before installing another instance.
    pub(crate) fn wait_closed(&mut self) -> anyhow::Result<()> {
        let mut failure = None;
        for reaper in self.reapers.drain(..) {
            match reaper.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    failure.get_or_insert(error);
                }
                Err(_) => {
                    failure.get_or_insert_with(|| anyhow::anyhow!("Native cleanup worker failed"));
                }
            }
        }
        failure.map_or(Ok(()), Err)
    }
}
