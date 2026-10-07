//! Bounded provider observations keep public session snapshots current without blocking a caller.
use super::*;
use std::time::{Duration, Instant};

impl Manager {
    /// Poll each active execution at most four times per second and retain only one pending query.
    ///
    /// All readers use the same observed state: a plugin calling session.host.status does not need
    /// privileged access to the native query API to discover that its program ended. Each query
    /// remains bound to the original source and provider, so retirement revokes it normally.
    pub(crate) fn poll_execution_states(&mut self) {
        self.poll_execution_stops();
        self.poll_session_operations();
        let active = self
            .executions()
            .into_iter()
            .filter(|entry| entry.snapshot().state.is_active())
            .map(|entry| entry.id())
            .collect::<Vec<_>>();
        self.host_sessions.observations.retain(|id, completion| {
            active.contains(id)
                && matches!(
                    completion.status(),
                    RequestUpdate::Accepted | RequestUpdate::Progress { .. }
                )
        });
        let now = Instant::now();
        if self
            .host_sessions
            .last_observation
            .is_some_and(|last| now.duration_since(last) < Duration::from_millis(250))
        {
            return;
        }
        self.host_sessions.last_observation = Some(now);
        for id in active {
            if !self.host_sessions.observations.contains_key(&id) {
                // Failure to ask is not evidence of exit. Retirement and request timeout retain
                // their own diagnostics rather than fabricating a successful end.
                if let Ok(completion) = self.query_execution(id) {
                    self.host_sessions.observations.insert(id, completion);
                }
            }
        }
    }
}
