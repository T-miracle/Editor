//! One completion gate and deadline policy is shared by editor work and plugin service calls.
use plugin_protocol::api::{CancelMode, CancellationEffect, ErrorCode, Failure, RequestUpdate};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct Completion<T> {
    state: Arc<Mutex<(u64, RequestUpdate<T>, bool)>>,
    deadline: Instant,
    /// Instance retirement revokes queued native work even before the manager's next polling tick.
    pub(crate) lifetimes: Vec<Arc<AtomicBool>>,
}
impl<T: Clone> Completion<T> {
    pub fn new(timeout_ms: u32) -> Self {
        Self {
            state: Arc::new(Mutex::new((0, RequestUpdate::Accepted, false))),
            deadline: Instant::now() + Duration::from_millis(timeout_ms.into()),
            lifetimes: Vec::new(),
        }
    }
    /// Begin and finish are idempotent even if a publication or completion arrives twice.
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
    pub fn finish(&self, result: Result<T, Failure>) {
        self.expire(Instant::now());
        let mut state = self.state.lock().unwrap();
        if !state.1.is_terminal() {
            state.0 += 1;
            state.1 = RequestUpdate::Completed { result };
        }
    }
    pub fn status(&self) -> RequestUpdate<T> {
        self.expire(Instant::now());
        self.state.lock().unwrap().1.clone()
    }
    pub fn update(&self) -> (u64, RequestUpdate<T>) {
        self.expire(Instant::now());
        let state = self.state.lock().unwrap();
        (state.0, state.1.clone())
    }
    /// Side effects cannot be rolled back by cancelling the wait for their result.
    pub fn enter_side_effect(&self) -> bool {
        self.expire(Instant::now());
        let mut state = self.state.lock().unwrap();
        if !matches!(state.1, RequestUpdate::Progress { .. }) {
            return false;
        }
        state.2 = true;
        true
    }
    pub fn expire(&self, now: Instant) {
        if self
            .lifetimes
            .iter()
            .any(|alive| !alive.load(Ordering::Acquire))
        {
            let _ = self.cancel(CancelMode::TryTerminate, ErrorCode::InvalidHandle);
            return;
        }
        if now >= self.deadline {
            let _ = self.cancel(CancelMode::TryTerminate, ErrorCode::TimedOut);
        }
    }
    pub fn cancel(
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
        let effect = if state.2 {
            CancellationEffect::WaitingStopped
        } else {
            CancellationEffect::NotExecuted
        };
        state.0 += 1;
        state.1 = RequestUpdate::Cancelled { reason, effect };
        Ok(effect)
    }
    pub fn retire(&self) {
        let _ = self.cancel(CancelMode::TryTerminate, ErrorCode::Cancelled);
    }
}
