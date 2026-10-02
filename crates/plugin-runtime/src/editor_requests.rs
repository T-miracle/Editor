//! Owned asynchronous calls cross the worker/UI boundary without exposing WASM stores to the UI.
use plugin_protocol::api::{CancelMode, CancellationEffect};
use plugin_protocol::api::{
    EditorOperation, EditorValue, ErrorCode, Failure, RequestUpdate, ResourceHandle,
};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Clones share one completion gate; dropping an instance seals every outstanding call.
#[derive(Clone)]
pub struct EditorRequest {
    handle: ResourceHandle,
    operation: EditorOperation,
    workspace: String,
    state: Arc<Mutex<(u64, RequestUpdate, bool)>>,
    deadline: Instant,
}

impl EditorRequest {
    /// Only the runtime can create a call after checking permission and ownership.
    pub(crate) fn new(
        handle: ResourceHandle,
        operation: EditorOperation,
        workspace: String,
        timeout_ms: u32,
    ) -> Self {
        Self {
            handle,
            operation,
            workspace,
            state: Arc::new(Mutex::new((0, RequestUpdate::Accepted, false))),
            deadline: Instant::now() + Duration::from_millis(timeout_ms.into()),
        }
    }
    pub fn handle(&self) -> &ResourceHandle {
        &self.handle
    }
    pub fn operation(&self) -> &EditorOperation {
        &self.operation
    }
    pub fn workspace(&self) -> &str {
        &self.workspace
    }
    /// Claim queued work exactly once, so repeated publication cannot duplicate side effects.
    pub fn begin(&self) -> bool {
        self.expire(Instant::now());
        let mut state = self.state.lock().unwrap();
        if !matches!(state.1, RequestUpdate::Accepted) {
            return false;
        }
        state.0 += 1;
        state.1 = RequestUpdate::Progress {
            message: "Executing".into(),
        };
        true
    }
    /// Late completions cannot replace a terminal failure or an earlier successful result.
    pub fn finish(&self, result: Result<EditorValue, Failure>) {
        self.expire(Instant::now());
        let mut state = self.state.lock().unwrap();
        if state.1.is_terminal() {
            return;
        }
        state.0 += 1;
        state.1 = RequestUpdate::Completed { result };
    }
    /// Terminal state remains observable to in-flight host work after instance teardown.
    pub fn status(&self) -> RequestUpdate {
        self.state.lock().unwrap().1.clone()
    }
    pub(crate) fn update(&self) -> (u64, RequestUpdate) {
        self.expire(Instant::now());
        let state = self.state.lock().unwrap();
        (state.0, state.1.clone())
    }
    pub(crate) fn retire(&self) {
        let _ = self.cancel(CancelMode::TryTerminate, ErrorCode::Cancelled);
    }
    /// The host calls this immediately before an irreversible side effect, after cancellable preparation.
    pub fn enter_side_effect(&self) -> bool {
        self.expire(Instant::now());
        let mut state = self.state.lock().unwrap();
        if !matches!(state.1, RequestUpdate::Progress { .. }) {
            return false;
        }
        state.2 = true;
        true
    }
    /// Explicit host ticks allow bounded queue deadlines without a timer per guest request.
    pub fn expire(&self, now: Instant) {
        if now >= self.deadline {
            let _ = self.cancel(CancelMode::TryTerminate, ErrorCode::TimedOut);
        }
    }
    pub(crate) fn cancel(
        &self,
        _mode: CancelMode,
        reason: ErrorCode,
    ) -> Result<CancellationEffect, Failure> {
        let mut state = self.state.lock().unwrap();
        if state.1.is_terminal() {
            return Err(Failure::new(
                ErrorCode::InvalidState,
                "Request already completed",
            ));
        }
        // Current editor operations cannot interrupt an atomic filesystem commit; try-terminate reports that limit honestly.
        let effect = if state.2 {
            CancellationEffect::WaitingStopped
        } else {
            CancellationEffect::NotExecuted
        };
        state.0 += 1;
        state.1 = RequestUpdate::Cancelled { reason, effect };
        Ok(effect)
    }
}

pub(crate) struct PendingRequest {
    pub call: EditorRequest,
    pub sent: bool,
    pub reported: u64,
}
