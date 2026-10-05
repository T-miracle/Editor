//! Host IDs pin debug targets to their provider, source authority and observed native retirement.
use super::*;
use crate::{
    native_work::NativeWork,
    plugin_services::{Context, Provider},
};
use plugin_protocol::{api::RequestUpdate, process::ExitMode};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// A bounded exchange retains the actual native exit barrier after guest ownership is revoked.
#[derive(Clone)]
pub struct DebugRequest {
    completion: Completion<Value>,
    session: String,
    native: Arc<Mutex<NativeWork>>,
}
impl DebugRequest {
    /// The immutable host identity is available before adapter creation, including for early stop.
    pub fn session(&self) -> &str {
        &self.session
    }
    /// Terminal state receipts wait for tree exit and EOF; inspection keeps its published schema.
    pub fn status(&self) -> RequestUpdate<Value> {
        let native = self.native.lock().unwrap();
        if let Some(message) = native.failure() {
            return RequestUpdate::Completed {
                result: Err(Failure::new(ErrorCode::OperationFailed, message)),
            };
        }
        match self.completion.status() {
            RequestUpdate::Completed {
                result: Ok(mut value),
            } => {
                if terminal(&value) && !native.drained() {
                    return RequestUpdate::Accepted;
                }
                if value.get("session").is_some() {
                    value["session"] = serde_json::json!(self.session);
                }
                RequestUpdate::Completed { result: Ok(value) }
            }
            status => status,
        }
    }
}
/// One target owns its native observer even while an asynchronous reaper holds the old job.
pub(in crate::manager) struct Owned {
    pub id: String,
    pub configuration: Option<String>,
    pub origin: Context,
    pub provider: Provider,
    pub alive: Arc<AtomicBool>,
    pub start: Completion<Value>,
    pub observation: Option<DebugRequest>,
    pub observed: Option<Value>,
    native: Arc<Mutex<NativeWork>>,
    /// The independent stop receipt survives the revoked guest invocation's cancellation.
    stop_completion: Option<Completion<Value>>,
    stop_call: Option<Completion<Value>>,
    stopping: Option<Instant>,
    /// A terminal protocol event is intent; only the native observer makes it a final state.
    retirement: Option<Value>,
    last_poll: Option<Instant>,
    failed_polls: u8,
}
fn terminal(value: &Value) -> bool {
    value["state"] == "exited" || value["state"] == "failed"
}
impl Owned {
    fn request(&self, completion: Completion<Value>) -> DebugRequest {
        DebugRequest {
            completion,
            session: self.id.clone(),
            native: self.native.clone(),
        }
    }
    fn backend(&self) -> anyhow::Result<String> {
        match self.start.status() {
            RequestUpdate::Completed { result: Ok(value) } => value["session"]
                .as_str()
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| anyhow::anyhow!("Provider returned no debug session identity")),
            _ => Err(anyhow::anyhow!("Debug target has no creation receipt yet")),
        }
    }
    /// Failed cleanup keeps ownership occupied: starting a replacement would overlap an old tree.
    fn ended(&self) -> bool {
        let native = self.native.lock().unwrap();
        native.failure().is_none()
            && native.drained()
            && self.observed.as_ref().is_some_and(terminal)
    }
    fn ensure_stop_receipt(&mut self) -> Completion<Value> {
        self.stop_completion
            .get_or_insert_with(|| Completion::new(15_000))
            .clone()
    }
    /// Invalidate the last pause immediately, while retaining a nonterminal state until actual exit.
    fn retire(&mut self, state: &str, reason: Option<&str>) {
        if self.retirement.is_some() {
            return;
        }
        let epoch = self
            .observed
            .as_ref()
            .and_then(|value| value["pause"].as_u64())
            .unwrap_or(0)
            .saturating_add(1)
            .min(i64::MAX as u64);
        let mut value = serde_json::json!({"session":self.id,"state":state,"pause":epoch});
        if let Some(reason) = reason {
            let mut end = reason.len().min(64);
            while !reason.is_char_boundary(end) {
                end -= 1;
            }
            value["reason"] = serde_json::json!(&reason[..end]);
        }
        self.retirement = Some(value);
        self.observed = Some(
            serde_json::json!({"session":self.id,"state":"running","pause":epoch,"reason":"Stopping debugger"}),
        );
        self.native.lock().unwrap().stop(ExitMode::Force);
        self.alive.store(false, Ordering::Release);
        self.observation = None;
    }
    fn fail(&mut self, reason: &str) {
        self.retire("failed", Some(reason));
    }
    /// A missing identity fails immediately; transient status failures have three actual attempts.
    fn poll_failed(&mut self, error: Failure) {
        self.failed_polls = self.failed_polls.saturating_add(1);
        if matches!(
            error.code,
            ErrorCode::InvalidHandle | ErrorCode::InvalidState
        ) || self.failed_polls >= 3
        {
            self.fail(&error.message);
        }
    }
    /// Publication and Stop/Force replies use the same real exit barrier, never a DAP assumption.
    fn finish_retirement(&mut self) {
        let native = self.native.lock().unwrap();
        if let Some(message) = native.failure() {
            let failure = Failure::new(ErrorCode::OperationFailed, message);
            self.observed = Some(
                serde_json::json!({"session":self.id,"state":"failed","reason":"Native cleanup failed"}),
            );
            if let Some(completion) = &self.stop_completion {
                completion.finish(Err(failure));
            }
        } else if native.drained()
            && let Some(value) = &self.retirement
        {
            self.observed = Some(value.clone());
            if let Some(completion) = &self.stop_completion {
                completion.finish(Ok(value.clone()));
            }
        }
    }
}
#[derive(Default)]
pub(in crate::manager) struct Sessions {
    pub entries: BTreeMap<String, Owned>,
    next: u64,
}

/// The called optional method is the only addition to the six required signatures.
fn dependency_for(method: &str) -> Result<Dependency, Failure> {
    let mut dependency = debug_dependency()?;
    dependency
        .methods
        .retain(|name, _| DEBUG_REQUIRED_METHODS.contains(&name.as_str()) || name == method);
    Ok(dependency)
}
impl Manager {
    /// Enqueue an exchange without blocking the actor or interpreting a short delay as failure.
    pub fn begin_debug_call(
        &mut self,
        method: &str,
        arguments: Value,
    ) -> anyhow::Result<DebugRequest> {
        self.begin_configured_debug_call(None, method, arguments)
    }
    /// Configuration deduplication includes native retirement; pending cleanup cannot launch again.
    pub fn begin_configured_debug_call(
        &mut self,
        configuration: Option<&str>,
        method: &str,
        mut arguments: Value,
    ) -> anyhow::Result<DebugRequest> {
        anyhow::ensure!(
            self.trusted && self.workspace_open,
            "Restricted/closed workspace cannot debug"
        );
        let timeout = debug_timeout_ms(method)
            .ok_or_else(|| anyhow::anyhow!("Unknown debug method {method}"))?;
        let dependency = dependency_for(method).map_err(start_failure)?;
        let scope = self.host_scope();
        self.refresh_services();
        let (id, origin, reference, is_start, native) = if method == "start" {
            if let Some(config) = configuration
                && let Some(session) = self.debug_sessions.entries.values().find(|session| {
                    session.configuration.as_deref() == Some(config)
                        && session.origin.caller.scope == scope
                        && !session.ended()
                })
            {
                anyhow::ensure!(
                    session.retirement.is_none() && session.stopping.is_none(),
                    "Previous debugger still closing; wait for its actual exit"
                );
                return Ok(session.request(session.start.clone()));
            }
            while self.debug_sessions.entries.len() >= 64 {
                let expired = self
                    .debug_sessions
                    .entries
                    .iter()
                    .find(|(_, entry)| entry.ended())
                    .map(|(id, _)| id.clone());
                let Some(id) = expired else {
                    break;
                };
                self.debug_sessions.entries.remove(&id);
            }
            anyhow::ensure!(
                self.debug_sessions.entries.len() < 64,
                "Debug session quota exceeded"
            );
            self.debug_sessions.next = self
                .debug_sessions
                .next
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("Debug identity exhausted"))?;
            let id = format!("debug-{}", self.debug_sessions.next);
            let caller = host_caller(&scope);
            let alive = Arc::new(AtomicBool::new(true));
            let native = Arc::new(Mutex::new(NativeWork::default()));
            self.host_resources.preparations.register(&alive, &native);
            let origin = Context {
                caller: caller.clone(),
                permissions: caller.permissions.clone(),
                ancestry: vec![],
                lifetimes: vec![self.host_alive.clone(), alive],
            };
            let reference = self
                .plugin_services
                .lock()
                .unwrap()
                .resolve(&caller, DEBUG_CONTRACT, &dependency)
                .map_err(start_failure)?;
            (id, origin, reference, true, native)
        } else {
            let id = arguments["session"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing host debug session"))?
                .to_owned();
            let session = self
                .debug_sessions
                .entries
                .get(&id)
                .filter(|entry| entry.origin.caller.scope == scope)
                .ok_or_else(|| anyhow::anyhow!("Unknown/foreign host debug session"))?;
            // Historical status remains queryable after the provider root has been sealed.
            if method == "status" && session.retirement.is_some() {
                let completion = Completion::new(timeout);
                completion.finish(Ok(session.retirement.clone().unwrap()));
                return Ok(session.request(completion));
            }
            if method == "stop" && session.stop_completion.is_some() {
                return Ok(session.request(session.stop_completion.as_ref().unwrap().clone()));
            }
            anyhow::ensure!(
                session.alive.load(Ordering::Acquire),
                "Debug session owner retired"
            );
            if method == "stop" && session.backend().is_err() {
                return self.force_debug_session(&id);
            }
            arguments["session"] = Value::String(session.backend()?);
            let reference = self
                .plugin_services
                .lock()
                .unwrap()
                .resolve_pinned(
                    &session.origin.caller,
                    DEBUG_CONTRACT,
                    &dependency,
                    &session.provider.caller.instance,
                )
                .map_err(start_failure)?;
            (
                id,
                session.origin.clone(),
                reference,
                false,
                session.native.clone(),
            )
        };
        let mut completion = Completion::new(timeout);
        completion.lifetimes = origin.lifetimes.clone();
        let provider = reference.provider.clone();
        let mut call = host_method_call(
            &origin.caller,
            reference,
            method,
            arguments,
            &dependency,
            completion.clone(),
            self.host_alive.clone(),
        )
        .map_err(start_failure)?;
        call.context = origin
            .delegate(&provider, &call.signature)
            .map_err(start_failure)?;
        self.plugin_services
            .lock()
            .unwrap()
            .enqueue(call)
            .map_err(start_failure)?;
        if is_start {
            self.debug_sessions.entries.insert(
                id.clone(),
                Owned {
                    id: id.clone(),
                    configuration: configuration.map(str::to_owned),
                    provider,
                    alive: origin.lifetimes.last().unwrap().clone(),
                    origin,
                    start: completion.clone(),
                    observation: None,
                    observed: None,
                    native: native.clone(),
                    stop_completion: None,
                    stop_call: None,
                    stopping: None,
                    retirement: None,
                    last_poll: None,
                    failed_polls: 0,
                },
            );
        } else if method == "stop" {
            let session = self.debug_sessions.entries.get_mut(&id).unwrap();
            session.native.lock().unwrap().seal();
            session.stopping = Some(Instant::now());
            session.stop_call = Some(completion);
            let receipt = session.ensure_stop_receipt();
            return Ok(session.request(receipt));
        }
        Ok(DebugRequest {
            completion,
            session: id,
            native,
        })
    }
    /// Seal only this target and return a receipt that completes after the actual tree and EOF.
    pub fn force_debug_session(&mut self, id: &str) -> anyhow::Result<DebugRequest> {
        let scope = self.host_scope();
        let session = self
            .debug_sessions
            .entries
            .get_mut(id)
            .filter(|entry| entry.origin.caller.scope == scope)
            .ok_or_else(|| anyhow::anyhow!("Unknown host debug session"))?;
        let completion = session.ensure_stop_receipt();
        session.retire("exited", None);
        session.finish_retirement();
        Ok(session.request(completion))
    }
    /// Actual observations retain original identities, including histories after provider retirement.
    pub fn debug_observations(&self) -> Vec<DebugSession> {
        let scope = self.host_scope();
        self.debug_sessions
            .entries
            .values()
            .filter(|entry| entry.origin.caller.scope == scope)
            .filter_map(|entry| {
                entry
                    .observed
                    .as_ref()
                    .and_then(|value| DebugSession::from_value(value).ok())
                    .map(|mut observation| {
                        observation.provider = Some(entry.provider.caller.plugin.clone());
                        observation
                    })
            })
            .collect()
    }
    /// One status wait per target; normal Stop escalates after three seconds on every entry point.
    pub(in crate::manager) fn poll_debug_sessions(&mut self) {
        let scope = self.host_scope();
        let live: Vec<_> = self
            .live
            .values()
            .filter_map(|instance| instance.instance_id().map(str::to_owned))
            .collect();
        let mut queries = Vec::new();
        for session in self
            .debug_sessions
            .entries
            .values_mut()
            .filter(|entry| entry.origin.caller.scope == scope)
        {
            if session.retirement.is_some() {
                session.finish_retirement();
                continue;
            }
            if !live.contains(&session.provider.caller.instance) {
                let reason = self
                    .installed
                    .get(&session.provider.caller.plugin)
                    .and_then(|entry| entry.error.as_deref())
                    .unwrap_or("Debug provider retired");
                session.fail(reason);
            }
            if session.retirement.is_none() {
                match session.start.status() {
                    RequestUpdate::Completed { result: Err(error) } => session.fail(&error.message),
                    RequestUpdate::Cancelled { reason, .. } => {
                        session.fail(&format!("Debug launch cancelled: {reason:?}"))
                    }
                    _ => {}
                }
            }
            if let Some(stop) = &session.stop_call
                && let RequestUpdate::Completed { result: Ok(value) } = stop.status()
            {
                if terminal(&value) {
                    session.retire("exited", None);
                }
            }
            if session.stopping.is_some_and(|since| {
                since.elapsed() >= Duration::from_millis(crate::DEFAULT_STOP_GRACE_MS.into())
            }) {
                session.retire("exited", None);
            }
            if session.retirement.is_some() {
                session.finish_retirement();
                continue;
            }
            if let Some(request) = &session.observation {
                match request.completion.status() {
                    RequestUpdate::Completed { result } => {
                        match result {
                            Ok(mut value) => {
                                value["session"] = serde_json::json!(session.id);
                                session.failed_polls = 0;
                                if terminal(&value) {
                                    session.retire(
                                        if value["state"] == "failed" {
                                            "failed"
                                        } else {
                                            "exited"
                                        },
                                        value["reason"].as_str(),
                                    );
                                } else {
                                    session.observed = Some(value);
                                }
                            }
                            Err(error) => session.poll_failed(error),
                        }
                        session.observation = None;
                    }
                    RequestUpdate::Cancelled { .. } => {
                        session.observation = None;
                        session.poll_failed(Failure::new(
                            ErrorCode::TimedOut,
                            "Debug status request cancelled",
                        ));
                    }
                    _ => {}
                }
            }
            if session.retirement.is_some() {
                session.finish_retirement();
                continue;
            }
            if session.backend().is_ok()
                && session.observation.is_none()
                && session
                    .last_poll
                    .is_none_or(|last| last.elapsed() >= Duration::from_millis(250))
            {
                queries.push(session.id.clone());
                session.last_poll = Some(Instant::now());
            }
        }
        for id in queries {
            match self.begin_debug_call("status", serde_json::json!({"session":id})) {
                Ok(request) => {
                    self.debug_sessions
                        .entries
                        .get_mut(&id)
                        .unwrap()
                        .observation = Some(request)
                }
                Err(error) => self
                    .debug_sessions
                    .entries
                    .get_mut(&id)
                    .unwrap()
                    .poll_failed(Failure::new(ErrorCode::OperationFailed, error.to_string())),
            }
        }
    }
    /// Leaving uses the same grace/force barrier as user Stop, before retiring the outer source.
    pub(in crate::manager) fn stop_owned_debuggers(&mut self) {
        let scope = self.host_scope();
        let ids: Vec<_> = self
            .debug_sessions
            .entries
            .values()
            .filter(|entry| entry.origin.caller.scope == scope && !entry.ended())
            .map(|entry| entry.id.clone())
            .collect();
        let mut pending = Vec::new();
        for id in &ids {
            if let Ok(request) = self
                .begin_debug_call("stop", serde_json::json!({"session":id}))
                .or_else(|_| self.force_debug_session(id))
            {
                pending.push(request);
            }
        }
        let deadline = Instant::now() + Duration::from_secs(13);
        while pending
            .iter()
            .any(|request| !request.status().is_terminal())
            && Instant::now() < deadline
        {
            self.poll();
            std::thread::sleep(Duration::from_millis(2));
        }
        for id in ids {
            let _ = self.force_debug_session(&id);
        }
    }
}
