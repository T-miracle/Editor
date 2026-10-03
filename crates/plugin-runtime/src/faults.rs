//! Host-owned execution limits and bounded diagnostics, independent of any plugin's identity.
use plugin_protocol::api::{Input, Notification};

/// Epochs bound WASM execution time; fuel also bounds deterministic work between clock ticks.
pub const EPOCH_TICK_MS: u64 = 10;
pub const CALL_DEADLINE_MS: u64 = 1000;
pub const SNAPSHOT_DEADLINE_MS: u64 = 10000;
pub const CALL_FUEL: u64 = 100_000_000;
pub const SNAPSHOT_FUEL: u64 = 1_000_000_000;
pub const MEMORY_BYTES: usize = 256 * 1024 * 1024;

/// Account for aggregate linear memory across all modules in one component, not one limit per memory.
#[derive(Default)]
pub(crate) struct MemoryBudget {
    allocated: usize,
    reserved: usize,
}
impl wasmtime::ResourceLimiter for MemoryBudget {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> anyhow::Result<bool> {
        self.reserved = desired.saturating_sub(current);
        anyhow::ensure!(
            self.reserved <= MEMORY_BYTES.saturating_sub(self.allocated),
            "WASM memory budget exceeded (256 MiB total)"
        );
        if maximum.is_some_and(|maximum| desired > maximum) {
            self.reserved = 0;
            return Ok(false);
        }
        self.allocated += self.reserved;
        Ok(true)
    }
    fn memory_grow_failed(&mut self, error: anyhow::Error) -> anyhow::Result<()> {
        self.allocated = self.allocated.saturating_sub(self.reserved);
        self.reserved = 0;
        Err(error.context("WASM memory allocation failed"))
    }
    fn table_growing(
        &mut self,
        _: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> anyhow::Result<bool> {
        anyhow::ensure!(desired <= 100_000, "WASM table budget exceeded");
        Ok(maximum.is_none_or(|maximum| desired <= maximum))
    }
    fn instances(&self) -> usize {
        32
    }
    fn tables(&self) -> usize {
        32
    }
    fn memories(&self) -> usize {
        32
    }
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub plugin: String,
    pub scope: String,
    pub operation: String,
    pub message: String,
    // A single clock orders foreground WASM failures and delayed native retirement reports.
    sequence: u64,
}
impl Diagnostic {
    pub(crate) fn new(plugin: String, scope: String, operation: String, message: String) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        Self {
            plugin,
            scope,
            operation,
            message,
            sequence: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        }
    }
}

/// Merge by recording order so a full queue from one source cannot hide a newer failure.
pub(crate) fn latest(mut entries: Vec<Diagnostic>) -> Vec<Diagnostic> {
    entries.sort_by_key(|entry| entry.sequence);
    let excess = entries.len().saturating_sub(32);
    entries.drain(..excess);
    entries
}

/// A manager-owned bounded sink remains readable after the originating instance is retired.
#[derive(Clone, Default)]
pub(crate) struct NativeDiagnostics {
    entries: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<Diagnostic>>>,
    logs: crate::RuntimeLogs,
}
impl NativeDiagnostics {
    /// Retired native workers retain this process-local sink as well as the bounded diagnostic queue.
    pub(crate) fn with_logs(logs: crate::RuntimeLogs) -> Self {
        Self {
            logs,
            ..Default::default()
        }
    }
    pub(crate) fn reporter(&self, plugin: &str, scope: &str) -> NativeReporter {
        NativeReporter {
            sink: self.clone(),
            plugin: plugin.into(),
            scope: scope.into(),
        }
    }
    pub(crate) fn for_plugin(&self, plugin: &str) -> Vec<Diagnostic> {
        self.entries
            .lock()
            .unwrap()
            .iter()
            .filter(|entry| entry.plugin == plugin)
            .cloned()
            .collect()
    }
}

/// Capture attribution before spawning native work; no logging subscriber or live guest is required.
pub(crate) struct NativeReporter {
    sink: NativeDiagnostics,
    plugin: String,
    scope: String,
}
impl NativeReporter {
    pub(crate) fn record(&self, operation: &str, message: String) {
        let message: String = message.chars().take(4096).collect();
        self.sink.logs.append(
            &self.plugin,
            crate::LogLevel::Error,
            &format!("native/{operation}"),
            format!("scope={} {message}", self.scope),
        );
        let mut entries = self.sink.entries.lock().unwrap();
        if entries.len() == 128 {
            entries.pop_front();
        }
        entries.push_back(Diagnostic::new(
            self.plugin.clone(),
            self.scope.clone(),
            operation.into(),
            message,
        ));
    }
}

/// Async retirement is observable through the same bounded host diagnostics after its owner goes away.
#[cfg(test)]
mod native_diagnostic_tests {
    use super::{Diagnostic, NativeDiagnostics, latest};
    #[test]
    fn delayed_native_errors_retain_attribution_and_stay_bounded() {
        let logs = crate::RuntimeLogs::default();
        let manager = NativeDiagnostics::with_logs(logs.clone());
        let instance = manager.clone();
        let reporter = instance.reporter("unknown-plugin", "workspace-one");
        drop(instance);
        std::thread::spawn(move || {
            for index in 0..140 {
                reporter.record("pty.retire.wait", format!("process=42 failure={index}"));
            }
        })
        .join()
        .unwrap();
        let entries = manager.for_plugin("unknown-plugin");
        assert_eq!(entries.len(), 128);
        assert_eq!(entries[0].scope, "workspace-one");
        assert_eq!(entries[0].operation, "pty.retire.wait");
        assert!(entries.last().unwrap().message.contains("failure=139"));
        assert!(manager.for_plugin("another-plugin").is_empty());
        let records = logs.records("unknown-plugin");
        assert_eq!(records.len(), 140);
        assert_eq!(records.last().unwrap().source, "native/pty.retire.wait");
        assert_eq!(records.last().unwrap().level, crate::LogLevel::Error);
        assert!(
            records
                .last()
                .unwrap()
                .message
                .contains("process=42 failure=139")
        );
        assert!(logs.records("another-plugin").is_empty());
    }

    /// Old native failures never displace a later WASM trap, irrespective of merge source order.
    #[test]
    fn latest_failures_are_selected_across_native_and_wasm_sources() {
        let manager = NativeDiagnostics::default();
        let reporter = manager.reporter("plugin", "workspace");
        for _ in 0..40 {
            reporter.record("pty.retire.wait", "old native failure".into());
        }
        let wasm = Diagnostic::new(
            "plugin".into(),
            "workspace".into(),
            "activate".into(),
            "new WASM trap".into(),
        );
        reporter.record("pty.retire.cursor", "last native failure".into());
        let mut mixed = vec![wasm];
        mixed.extend(manager.for_plugin("plugin"));
        let result = latest(mixed);
        assert_eq!(result.len(), 32);
        assert_eq!(result[30].message, "new WASM trap");
        assert_eq!(result[31].message, "last native failure");
    }
}

/// Log operation identity without copying arbitrary document text, credentials or request arguments.
pub(crate) fn operation(message: &Input) -> String {
    match message {
        Input::Prepare { .. } => "prepare".into(),
        Input::Activate => "activate".into(),
        Input::Snapshot => "snapshot".into(),
        Input::Event { event, .. } => event_operation(event),
    }
}
fn event_operation(event: &Notification) -> String {
    match event {
        Notification::Command { id, .. } => format!("command:{id}"),
        plugin_protocol::api::Notification::LanguageService(context) => {
            format!("lsp-hook:{}", context.provider)
        }
        plugin_protocol::api::Notification::Service(
            plugin_protocol::service::Notification::Invoke(call),
        ) => format!("service:{}:{}", call.contract, call.method),
        _ => "event".into(),
    }
}
