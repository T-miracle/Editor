//! Host-owned execution limits and bounded diagnostics, independent of any plugin's identity.
use plugin_protocol::{Event, Message};

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
}

/// Log operation identity without copying arbitrary document text, credentials or request arguments.
pub(crate) fn operation(message: &Message) -> String {
    match message {
        Message::Prepare { .. } => "prepare".into(),
        Message::Activate => "activate".into(),
        Message::Snapshot => "snapshot".into(),
        Message::Event(event) => event_operation(event),
    }
}
fn event_operation(event: &Event) -> String {
    match event {
        Event::Surface { event, .. } => event_operation(event),
        Event::Command { id, .. } => format!("command:{id}"),
        Event::Capability(plugin_protocol::api::Notification::LanguageService(context)) => {
            format!("lsp-hook:{}", context.provider)
        }
        Event::Capability(plugin_protocol::api::Notification::Service(
            plugin_protocol::service::Notification::Invoke(call),
        )) => format!("service:{}:{}", call.contract, call.method),
        _ => "event".into(),
    }
}
