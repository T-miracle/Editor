//! Finite transport recovery uses a monotonic clock; no failed editor operation is replayed.
use super::*;

pub(super) const MAX_FAILURES: usize = 3;
#[derive(Default)]
pub(super) struct Recovery {
    failures: usize,
    retry_at: Duration,
    healthy_since: Option<Duration>,
    message: Option<String>,
    running: bool,
}
impl Recovery {
    fn check(&self, now: Duration) -> anyhow::Result<()> {
        ensure!(
            self.failures < MAX_FAILURES,
            "语言服务已暂停，请手动重启插件"
        );
        ensure!(
            now >= self.retry_at,
            "语言服务等待重试（{} ms）",
            self.retry_at.saturating_sub(now).as_millis()
        );
        Ok(())
    }
    fn failed(&mut self, operation: &str, error: &anyhow::Error, now: Duration) {
        self.failures += 1;
        self.healthy_since = None;
        self.running = false;
        self.retry_at = now + Duration::from_secs(if self.failures == 1 { 1 } else { 4 });
        self.message = Some(
            format!("{operation}: {error:#}")
                .chars()
                .take(4096)
                .collect(),
        );
    }
    fn succeeded(&mut self, now: Duration) {
        self.running = true;
        self.retry_at = Duration::ZERO;
        self.healthy_since.get_or_insert(now);
    }
    fn healthy(&mut self, now: Duration) {
        // Only successful operations on the same connection count; offline time never buys a fresh retry budget.
        if self
            .healthy_since
            .is_some_and(|start| now.saturating_sub(start) >= Duration::from_secs(60))
        {
            *self = Self::default();
            self.running = true;
            self.healthy_since = Some(now);
        }
    }
}
impl LanguageServer {
    /// Production uses elapsed monotonic time; acceptance tests can drive identical gates without sleeps.
    #[cfg(test)]
    pub(crate) fn prepare_at(&self, now: Duration) -> anyhow::Result<()> {
        self.recovery_attempt(|| now, "initialize", || self.prepare_once())
    }
    pub fn prepare(&self) -> anyhow::Result<()> {
        self.recovery_attempt(
            || self.started.elapsed(),
            "initialize",
            || self.prepare_once(),
        )
    }
    fn recovery_attempt<T>(
        &self,
        clock: impl Fn() -> Duration,
        operation: &str,
        work: impl FnOnce() -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        // Serialize checking the retry gate with launching; simultaneous views cannot spend extra attempts.
        let _attempt = self.attempt.lock().unwrap();
        self.recovery.lock().unwrap().check(clock())?;
        ensure!(self.is_active(), "LSP provider has been retired");
        let result = work();
        // All paths acquire the transport lock before the recovery lock; request failure uses that order too.
        if result.is_err() {
            *self.connection.lock().unwrap() = None;
        }
        let mut recovery = self.recovery.lock().unwrap();
        let now = clock();
        let mut log = None;
        match &result {
            Ok(_) => recovery.succeeded(now),
            Err(error) => {
                recovery.failed(operation, error, now);
                log = recovery.message.clone();
            }
        }
        drop(recovery);
        if let Some(message) = log {
            self.log_failure(operation, &message);
        }
        result
    }
    /// Startup retries run only on the dedicated transport executor and stop on retirement or three failures.
    pub fn prepare_until_ready(&self) -> anyhow::Result<()> {
        loop {
            let result = self.recovery_attempt(
                || self.started.elapsed(),
                "initialize/readiness",
                || self.prepare_ready_once(),
            );
            if result.is_ok() || !self.is_active() {
                return result;
            }
            let retry_at = {
                let recovery = self.recovery.lock().unwrap();
                if recovery.failures >= MAX_FAILURES {
                    return result;
                }
                recovery.retry_at
            };
            while self.started.elapsed() < retry_at {
                ensure!(self.is_active(), "LSP provider has been retired");
                std::thread::sleep(Duration::from_millis(25));
            }
        }
    }
    /// Ordinary requests never replay; their next explicit attempt must pass the same recovery gate.
    pub(super) fn connection_failed<T>(
        &self,
        operation: &str,
        result: &anyhow::Result<T>,
        connection: &mut Option<LanguageServerConnection>,
    ) {
        if let Err(error) = result {
            *connection = None;
            if self.is_active() {
                let mut recovery = self.recovery.lock().unwrap();
                recovery.failed(operation, error, self.started.elapsed());
                let message = recovery.message.clone().unwrap_or_default();
                drop(recovery);
                self.log_failure(operation, &message);
            }
        } else {
            self.recovery
                .lock()
                .unwrap()
                .healthy(self.started.elapsed());
        }
    }
    pub(crate) fn recovery_status(&self) -> Option<String> {
        let recovery = self.recovery.lock().unwrap();
        let message = recovery.message.as_ref()?;
        let state = if recovery.failures >= MAX_FAILURES {
            "已暂停，重启插件可恢复"
        } else if recovery.running {
            "已恢复"
        } else {
            "等待有限重试"
        };
        Some(format!(
            "{} [{}] {state}（{}/{}）：{message}",
            self.service.owner.as_str(),
            self.root.display(),
            recovery.failures,
            MAX_FAILURES
        ))
    }
    /// Structured bounded logs identify the owner and operation without serializing document or request payloads.
    fn log_failure(&self, operation: &str, message: &str) {
        let plugin = self.service.owner.as_str();
        tracing::warn!(plugin, scope=%self.root.display(), operation, message, "language service failure");
    }
}
