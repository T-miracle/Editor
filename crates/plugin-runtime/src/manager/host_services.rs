//! Host-owned execution sessions started through the public service contract.
//!
//! The editor's run controls are an ordinary service consumer: they resolve a compatible provider
//! for `interactive.execute` by contract and logical scope, never by plugin identity. Each start is
//! tracked as a request whose completion acknowledges program creation only — a provider result is
//! never interpreted as program exit, and no host business branch is added per language or tool.
use super::*;
use crate::plugin_services::{Call, Context as CallContext, Provider, Reference};
use crate::request_state::Completion;
use plugin_protocol::{
    api::{ErrorCode, Failure, RequestUpdate, ResourceHandle},
    service::{Caller, Dependency},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

/// The only execution contract the host consumes directly; its version family is negotiated.
pub const EXECUTION_CONTRACT: &str = "interactive.execute";
/// The session contract the host itself offers, so a consumer plugin and the title bar share one table.
///
/// The host is the participant that owns a workspace's visible sessions: it decides what is running,
/// which launch a session belongs to, and which request a repeated launch locates. A consumer that
/// started a program through a provider directly would create a session the host cannot see, so this
/// contract is the public seam where a consumer asks the host for a session instead. Both entry
/// points therefore reach the same table and the same deduplication rule, and neither has to know
/// which provider answers.
pub const SESSION_CONTRACT: &str = "session.host";
/// Version 2 requires the source's execution authority; older method declarations are refused.
pub const SESSION_CONTRACT_VERSION: &str = "2.0.0";
/// The only debug contract the host consumes directly.
///
/// Debugging is a provider contract like execution, so the host never learns which debugger answers,
/// how it is driven, or over what transport.
pub const DEBUG_CONTRACT: &str = "debug.session";
/// A debug start includes building and attaching, so it is allowed the longest window here.
pub const DEBUG_START_TIMEOUT_MS: u32 = 60_000;
/// Setting breakpoints is a state change the provider answers from its own model.
pub const DEBUG_BREAKPOINT_TIMEOUT_MS: u32 = 15_000;
/// Resume, pause, stop and status are short control exchanges.
pub const DEBUG_CONTROL_TIMEOUT_MS: u32 = 15_000;
/// A start request that a provider neither accepts nor rejects within this window is abandoned.
pub const EXECUTION_START_TIMEOUT_MS: u32 = 30_000;
/// A stop request is a short control exchange; waiting longer hides an unreachable provider.
pub const EXECUTION_STOP_TIMEOUT_MS: u32 = 10_000;
/// A status query is a bounded read of state the provider already holds.
pub const EXECUTION_STATUS_TIMEOUT_MS: u32 = 5_000;
/// Bound on retained host sessions for one workspace; ordinary work never approaches this.
const MAX_HOST_EXECUTIONS: usize = 64;

#[path = "host_services/lifecycle.rs"]
mod lifecycle;
#[path = "host_services/stop.rs"]
mod stop;
#[path = "host_services/subscriptions.rs"]
mod subscriptions;
pub use stop::{DEFAULT_STOP_GRACE_MS, StopOptions};
#[cfg(test)]
#[path = "host_services/regression_tests.rs"]
mod regression_tests;

/// How one provider stands with respect to the contract a caller needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderCandidate {
    /// Package identity, which is what a user's choice names.
    pub plugin: String,
    /// Why this provider is not usable, or `None` when it is.
    pub unavailable: Option<String>,
    /// Whether a launch in this scope would use this provider right now.
    pub selected: bool,
}

/// Version 1 requests carry literal argv and an optional working directory or label.
///
/// The host never composes a shell string here: parameters stay an argument vector so quoting and
/// metacharacters remain literal until an explicitly configured shell mode interprets them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRequest {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Entries applied over the environment the program would otherwise inherit.
    ///
    /// These belong to the program the user asked for: the host neither reads nor logs the values,
    /// and a request without entries inherits exactly what it did before this field existed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<RunEnvEntry>,
}

/// One environment entry of an execution request, as the service contract expresses it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunEnvEntry {
    pub name: String,
    pub value: String,
}

/// Longest accepted environment variable name, matching the service schema's bound.
const MAX_ENV_NAME_BYTES: usize = 128;
/// Most environment entries one launch may carry.
const MAX_ENV_ENTRIES: usize = 64;
/// Longest accepted environment value.
///
/// A real search path is easily several kilobytes, so this bound is generous enough to carry one
/// while still refusing a value that could not be passed to a native child.
const MAX_ENV_VALUE_BYTES: usize = 32 * 1024;

impl RunRequest {
    /// Reject oversized or malformed requests before they can occupy a provider's queue.
    fn validate(&self) -> Result<(), Failure> {
        let bounded = |text: &str, limit: usize| text.len() <= limit && !text.contains('\0');
        if !bounded(&self.program, 4096) || self.program.trim().is_empty() {
            return Err(Failure::new(
                ErrorCode::InvalidRequest,
                "Execution program must be a non-empty bounded executable name",
            ));
        }
        if self.args.len() > 128
            || !self.args.iter().all(|arg| bounded(arg, 4096))
            || self.args.iter().map(String::len).sum::<usize>() > 65536
        {
            return Err(Failure::new(
                ErrorCode::InvalidRequest,
                "Execution arguments exceed the bounded argv contract",
            ));
        }
        if let Some(cwd) = &self.cwd
            && !bounded(cwd, 4096)
        {
            return Err(Failure::new(
                ErrorCode::InvalidRequest,
                "Execution directory exceeds the bounded path contract",
            ));
        }
        if let Some(name) = &self.name
            && (!bounded(name, 256) || name.chars().any(char::is_control))
        {
            return Err(Failure::new(
                ErrorCode::InvalidRequest,
                "Execution label must be a bounded printable name",
            ));
        }
        if self.env.len() > MAX_ENV_ENTRIES {
            return Err(Failure::new(
                ErrorCode::InvalidRequest,
                "Execution environment carries too many entries",
            ));
        }
        for entry in &self.env {
            // A name that could not be passed to a native child is refused here, where the user can
            // see why, rather than failing after a panel has already opened.
            let valid_name = !entry.name.is_empty()
                && entry.name.len() <= MAX_ENV_NAME_BYTES
                && !entry.name.contains('=')
                && !entry.name.chars().any(char::is_control);
            if !valid_name || !bounded(&entry.value, MAX_ENV_VALUE_BYTES) {
                return Err(Failure::new(
                    ErrorCode::InvalidRequest,
                    format!("Invalid execution environment entry: {}", entry.name),
                ));
            }
        }
        Ok(())
    }
    /// One stable key per literal command so repeated clicks cannot silently start a second program.
    ///
    /// A launch with different environment entries is a different program context, so it is not
    /// collapsed onto the session of a launch that never had them.
    pub fn dedup_key(&self, cwd: Option<&str>) -> String {
        let mut hasher = DefaultHasher::new();
        self.program.hash(&mut hasher);
        self.args.hash(&mut hasher);
        self.cwd.as_deref().or(cwd).hash(&mut hasher);
        for entry in &self.env {
            entry.name.hash(&mut hasher);
            entry.value.hash(&mut hasher);
        }
        format!("{:016x}", hasher.finish())
    }
}

/// A session's externally meaningful phase; provider state is never merged into one host enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionState {
    /// Requested and not yet answered, or answered without a terminal program outcome.
    Starting,
    /// The provider confirmed program creation; this is not evidence that the program has exited.
    Running,
    /// A normal-exit request is outstanding; the program and all output remain owned.
    Stopping,
    /// The stop deadline expired or force was explicitly requested; actual exit is still awaited.
    Terminating,
    /// The request was refused, timed out, or its provider retired before answering.
    Failed,
    /// The provider observed that the owned program has ended, independently of its start request.
    Exited,
}

impl ExecutionState {
    /// The name a consumer sees. These are part of the session contract, so they are stated once.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Terminating => "terminating",
            Self::Failed => "failed",
            Self::Exited => "exited",
        }
    }
    /// Active ownership includes creation and both exit-request phases until an actual final result.
    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Starting | Self::Running | Self::Stopping | Self::Terminating
        )
    }
}

/// Why a host session stopped being active, retained for the visible result of a launch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionFailure {
    pub code: ErrorCode,
    pub message: String,
}

/// An immutable view of one host session, safe to publish to the native UI.
#[derive(Clone, Debug)]
pub struct ExecutionSnapshot {
    pub id: u64,
    /// Resolved provider package identity, reported for transparency rather than routing.
    pub plugin: String,
    pub request: RunRequest,
    pub dedup_key: String,
    pub state: ExecutionState,
    /// Provider-reported session identity, when its bounded result carried one.
    pub provider_session: Option<String>,
    pub failure: Option<ExecutionFailure>,
}

/// One host session's live request; clones observe the same completion gate.
#[derive(Clone)]
pub struct HostExecution {
    id: u64,
    /// The original caller and delegation chain own this execution for its entire lifetime.
    origin: CallContext,
    /// Only a validated observation from the pinned provider can release active-session capacity.
    ended: Arc<AtomicBool>,
    /// A provider that lost its session cannot keep consuming capacity through failing status polls.
    observation_failure: Arc<Mutex<Option<ExecutionFailure>>>,
    /// Independent revocation prevents one failed stop from killing another session of this source.
    execution_alive: Arc<AtomicBool>,
    /// Stop admission, deadline and force escalation never replace the observed final result.
    stop: Arc<Mutex<Option<stop::StopState>>>,
    plugin: String,
    /// Exact provider incarnation pinned at start time; its retirement fails this session.
    provider_instance: String,
    provider_alive: Arc<AtomicBool>,
    dedup_key: String,
    request: RunRequest,
    completion: Completion<Value>,
    /// The caller's cancelled wait is separate from the receipt needed to manage its native program.
    cancelled_wait: Arc<Mutex<Option<plugin_protocol::api::CancellationEffect>>>,
    /// Set when the pinned provider retired or was replaced after the session had started.
    provider_retired: Arc<AtomicBool>,
}

/// Diagnostics name the session and its command; the shared completion gate is not printable.
impl std::fmt::Debug for HostExecution {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HostExecution")
            .field("id", &self.id)
            .field("plugin", &self.plugin)
            .field("provider_instance", &self.provider_instance)
            .field("dedup_key", &self.dedup_key)
            .field("request", &self.request)
            .field("provider_active", &self.provider_active())
            .finish_non_exhaustive()
    }
}

impl HostExecution {
    /// Host session identity, unique within one runtime incarnation.
    pub fn id(&self) -> u64 {
        self.id
    }
    /// The provider package selected at start time; later selection changes do not retarget it.
    pub fn plugin(&self) -> &str {
        &self.plugin
    }
    /// Stable identity of the literal command, used to locate an existing session.
    pub fn dedup_key(&self) -> &str {
        &self.dedup_key
    }
    pub fn request(&self) -> &RunRequest {
        &self.request
    }
    /// Current provider answer; a non-terminal update is an accepted, not a finished, execution.
    pub fn update(&self) -> RequestUpdate<Value> {
        if let Some(effect) = *self.cancelled_wait.lock().unwrap() {
            RequestUpdate::Cancelled {
                reason: ErrorCode::Cancelled,
                effect,
            }
        } else {
            self.completion.status()
        }
    }
    /// The provider incarnation this session is bound to; later selections never retarget it.
    pub fn provider_instance(&self) -> &str {
        &self.provider_instance
    }
    /// Match a provider-local session only in its pinned incarnation. Local IDs can repeat across
    /// providers or after a provider update, so output routing must compare both parts of identity.
    pub fn owns_provider_session(&self, instance: &str, session: &str) -> bool {
        self.provider_instance == instance
            && self.snapshot().provider_session.as_deref() == Some(session)
    }
    /// Whether the pinned provider is still the live incarnation that answered this session.
    pub fn provider_active(&self) -> bool {
        self.provider_alive.load(Ordering::Acquire)
            && !self.provider_retired.load(Ordering::Acquire)
            && self
                .origin
                .lifetimes
                .iter()
                .all(|alive| alive.load(Ordering::Acquire))
    }
    /// The trusted window can manage its workspace; a plugin can address only its own incarnation's work.
    fn visible_to(&self, caller: &Caller) -> bool {
        self.origin.caller.scope == caller.scope
            && (caller.instance == host_caller(&caller.scope).instance
                || self.origin.caller.instance == caller.instance)
    }
    /// The provider incarnation this session is pinned to, for diagnostics and host publications.
    pub fn provider_identity(&self) -> (&str, &str) {
        (&self.plugin, &self.provider_instance)
    }
    /// Publishable view for the run controls; derived without mutating the session.
    pub fn snapshot(&self) -> ExecutionSnapshot {
        let (mut state, provider_session, mut failure) = self.lifecycle();
        // A session outlives its provider only as a visible result: the program it started is no
        // longer managed by this runtime, so it is never reported as still running.
        if state.is_active() && !self.provider_active() {
            state = ExecutionState::Failed;
            failure = Some(ExecutionFailure {
                code: ErrorCode::InvalidHandle,
                message: "Execution owner or provider retired".into(),
            });
        }
        ExecutionSnapshot {
            id: self.id,
            plugin: self.plugin.clone(),
            request: self.request.clone(),
            dedup_key: self.dedup_key.clone(),
            state,
            provider_session,
            failure,
        }
    }
    /// Provider-reported lifecycle, independent of whether that provider is still present.
    fn lifecycle(&self) -> (ExecutionState, Option<String>, Option<ExecutionFailure>) {
        if let Some(failure) = self.observation_failure.lock().unwrap().clone() {
            let provider_session = match self.completion.status() {
                RequestUpdate::Completed { result: Ok(value) } => provider_session_of(&value),
                _ => None,
            };
            return (ExecutionState::Failed, provider_session, Some(failure));
        }
        match self.completion.status() {
            RequestUpdate::Accepted | RequestUpdate::Progress { .. } => {
                let state = self
                    .stop
                    .lock()
                    .unwrap()
                    .as_ref()
                    .map_or(ExecutionState::Starting, |stop| stop.phase());
                (state, None, None)
            }
            RequestUpdate::Completed { result: Ok(value) } => {
                let failure = self.observation_failure.lock().unwrap().clone();
                let state = if failure.is_some() {
                    ExecutionState::Failed
                } else if self.ended.load(Ordering::Acquire) {
                    ExecutionState::Exited
                } else {
                    // Creation is only an acknowledgement; a later status must confirm exit.
                    self.stop
                        .lock()
                        .unwrap()
                        .as_ref()
                        .map_or(ExecutionState::Running, |stop| stop.phase())
                };
                (state, provider_session_of(&value), failure)
            }
            RequestUpdate::Completed { result: Err(error) } => (
                ExecutionState::Failed,
                None,
                Some(ExecutionFailure {
                    code: error.code,
                    message: error.message.clone(),
                }),
            ),
            RequestUpdate::Cancelled { reason, .. } => (
                ExecutionState::Failed,
                None,
                Some(ExecutionFailure {
                    code: reason,
                    message: "Execution request was cancelled".into(),
                }),
            ),
        }
    }
    /// Abandon the wait without claiming that an already created program stopped.
    pub fn cancel(&self) {
        let mut wait = self.cancelled_wait.lock().unwrap();
        if wait.is_none() {
            if let Ok(effect) = self.completion.detach_wait() {
                *wait = Some(effect);
            }
        }
    }

    /// Whether a stop is meaningful: the provider confirmed a program and is still present.
    pub fn stoppable(&self) -> bool {
        self.state().is_active() && self.provider_active()
    }

    /// Lifecycle as recorded by the provider, ignoring whether that provider is still present.
    pub fn state(&self) -> ExecutionState {
        self.lifecycle().0
    }
}

/// Bounded host session table; entries stay after completion so a repeat launch locates its session.
#[derive(Default)]
pub(crate) struct HostSessions {
    next_id: u64,
    entries: BTreeMap<u64, HostExecution>,
    /// Cleared when this runtime retires so queued starts cannot outlive their owning window.
    alive: Arc<AtomicBool>,
    /// One outstanding observation per execution, shared by every public snapshot reader.
    observations: BTreeMap<u64, Completion<Value>>,
    /// Bound polling frequency independently of UI frames and plugin query frequency.
    last_observation: Option<std::time::Instant>,
    /// Subscription identities are never reused across source incarnations.
    next_subscription: u64,
    subscriptions: BTreeMap<u64, subscriptions::Subscription>,
    /// At most 128 gateway forwards wait on real provider results without blocking the actor.
    next_operation: u64,
    operations: BTreeMap<u64, subscriptions::Forwarded>,
}

impl HostSessions {
    pub(crate) fn new(alive: Arc<AtomicBool>) -> Self {
        Self {
            next_id: 1,
            entries: BTreeMap::new(),
            alive,
            observations: BTreeMap::new(),
            last_observation: None,
            next_subscription: 1,
            subscriptions: Default::default(),
            next_operation: 1,
            operations: Default::default(),
        }
    }
    /// A session for the same literal command, if one is retained for this scope.
    pub(crate) fn find(&self, dedup_key: &str, caller: &Caller) -> Option<HostExecution> {
        self.entries
            .values()
            .find(|entry| {
                entry.dedup_key == dedup_key
                    && entry.origin.caller.instance == caller.instance
                    && entry.origin.caller.scope == caller.scope
                    && matches!(
                        entry.snapshot().state,
                        ExecutionState::Starting
                            | ExecutionState::Running
                            | ExecutionState::Stopping
                            | ExecutionState::Terminating
                    )
            })
            .cloned()
    }
    pub(crate) fn get(&self, id: u64) -> Option<HostExecution> {
        self.entries.get(&id).cloned()
    }
    pub(crate) fn iter(&self) -> impl Iterator<Item = &HostExecution> {
        self.entries.values()
    }
    /// Retain one request with its resolved provider and reserved identity.
    pub(crate) fn insert(
        &mut self,
        provider: Provider,
        request: RunRequest,
        dedup_key: String,
        completion: Completion<Value>,
        mut origin: CallContext,
    ) -> Result<HostExecution, Failure> {
        self.reserve_capacity()?;
        let id = self.next_id;
        self.next_id += 1;
        // The provider sees this token in every resource it creates for this invocation. Only this
        // execution's stop failure or retirement may revoke it; other work by the same caller survives.
        let execution_alive = Arc::new(AtomicBool::new(true));
        origin.lifetimes.push(execution_alive.clone());
        let execution = HostExecution {
            id,
            origin,
            ended: Arc::new(AtomicBool::new(false)),
            observation_failure: Arc::new(Mutex::new(None)),
            execution_alive,
            stop: Arc::new(Mutex::new(None)),
            plugin: provider.caller.plugin.clone(),
            provider_instance: provider.caller.instance.clone(),
            provider_alive: provider.alive.clone(),
            dedup_key,
            request,
            completion,
            cancelled_wait: Arc::new(Mutex::new(None)),
            provider_retired: Arc::new(AtomicBool::new(false)),
        };
        self.entries.insert(id, execution.clone());
        Ok(execution)
    }
    /// Reserve space before enqueueing any side effect. Start acknowledgement is never an eviction signal.
    fn reserve_capacity(&mut self) -> Result<(), Failure> {
        while self.entries.len() >= MAX_HOST_EXECUTIONS {
            let candidate = self
                .entries
                .iter()
                .find(|(_, entry)| {
                    matches!(
                        entry.snapshot().state,
                        ExecutionState::Failed | ExecutionState::Exited
                    )
                })
                .map(|(id, _)| *id);
            match candidate {
                Some(id) => {
                    self.entries.remove(&id);
                    self.observations.remove(&id);
                }
                None => {
                    return Err(Failure::new(
                        ErrorCode::LimitExceeded,
                        "Execution session capacity reached",
                    ));
                }
            }
        }
        Ok(())
    }
    /// Apply status only to the session owned by the original caller and the pinned incarnation.
    pub(crate) fn observe_status(&self, call: &Call, value: &Value) {
        // A stop acknowledgement says termination was issued. Only a status observation says
        // the program actually ended, so it is the only result that releases active capacity.
        if call.reference.contract != EXECUTION_CONTRACT || call.method != "status" {
            return;
        }
        let Some(session) = call.arguments.get("session").and_then(Value::as_str) else {
            return;
        };
        if value.get("session").and_then(Value::as_str) != Some(session) {
            return;
        }
        if !matches!(
            value.get("state").and_then(Value::as_str),
            Some("exited" | "ended" | "stopped" | "terminated")
        ) {
            return;
        }
        for entry in self.entries.values().filter(|entry| {
            entry.provider_instance == call.reference.provider.caller.instance
                && entry.origin.caller.instance == call.context.caller.instance
                && entry.snapshot().provider_session.as_deref() == Some(session)
        }) {
            entry.ended.store(true, Ordering::Release);
        }
    }
    /// A pinned provider's permanent session loss is a failure, never a successful program exit.
    pub(crate) fn observe_error(&self, call: &Call, failure: &Failure) {
        if call.reference.contract != EXECUTION_CONTRACT
            || call.method != "status"
            || failure.code != ErrorCode::InvalidHandle
        {
            // Transient query failures say nothing about whether the program is still running.
            return;
        }
        let Some(session) = call.arguments.get("session").and_then(Value::as_str) else {
            return;
        };
        for entry in self.entries.values().filter(|entry| {
            entry.provider_instance == call.reference.provider.caller.instance
                && entry.origin.caller.instance == call.context.caller.instance
                && entry.snapshot().provider_session.as_deref() == Some(session)
                && entry.snapshot().state.is_active()
        }) {
            *entry.observation_failure.lock().unwrap() = Some(ExecutionFailure {
                code: failure.code,
                message: failure.message.clone(),
            });
        }
    }
    /// A retired or replaced provider ends its sessions' active state without replaying any command.
    ///
    /// The set of pinned incarnations is passed in rather than inspected here so the runtime decides
    /// liveness once per frame.
    pub(crate) fn retire_absent_providers(&mut self, present: &dyn Fn(&str) -> bool) {
        for entry in self.entries.values() {
            if !present(&entry.provider_instance) {
                entry.provider_retired.store(true, Ordering::Release);
            }
        }
    }
    /// Retire every outstanding request without pretending that native side effects were undone.
    pub(crate) fn retire(&mut self) {
        self.alive.store(false, Ordering::Release);
        for entry in self.entries.values() {
            entry.completion.retire();
        }
        for observation in self.observations.values() {
            observation.retire();
        }
        self.observations.clear();
        self.subscriptions.clear();
        for operation in self.operations.values() {
            operation.completion.retire();
            operation.call.completion.retire();
        }
        self.operations.clear();
    }
}

/// Extract a provider-reported session identity without imposing a provider-specific result shape.
/// Configuration IDs share one active launch for each source; literal starts have a separate key space.
fn execution_identity(
    request: &RunRequest,
    configuration: Option<&str>,
) -> Result<String, Failure> {
    match configuration {
        Some(id) if !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control) => {
            Ok(format!("configuration:{id}"))
        }
        Some(_) => Err(Failure::new(
            ErrorCode::InvalidRequest,
            "Invalid configuration identity",
        )),
        None => Ok(request.dedup_key(None)),
    }
}

fn provider_session_of(value: &Value) -> Option<String> {
    let object = value.as_object()?;
    for key in ["session", "id", "handle"] {
        if let Some(text) = object.get(key).and_then(Value::as_str)
            && !text.is_empty()
            && text.len() <= 128
        {
            return Some(text.to_owned());
        }
    }
    None
}

/// The dependency the host requires of any execution provider.
///
/// A consumer matches a provider only by asking for the identical method shape, so these bounds and
/// the declared authority mirror execution contract 1.1 exactly; the host's own permission set, not
/// this declaration, decides what authority a start delegates to the provider. `stop` is required
/// rather than optional: the run controls promise a stop, so a provider that cannot stop is
/// incompatible instead of appearing available and then failing to end a program.
/// Turn a host-written declaration into the requirement a provider has to match, method for method.
///
/// The host declares what it calls rather than deriving it from a provider, so the requirement is the
/// contract; `required` names the methods whose absence makes a declaration unusable, and everything
/// else in the declaration keeps its exact shape.
pub(crate) fn dependency_from_declaration(
    declaration: Value,
    required: &[&str],
    version: &str,
    incomplete: &str,
) -> Result<Dependency, Failure> {
    let contract: plugin_protocol::service::Contract = serde_json::from_value(declaration)
        .map_err(|error| Failure::new(ErrorCode::OperationFailed, error.to_string()))?;
    let methods = contract.methods;
    if required.iter().any(|method| !methods.contains_key(*method)) {
        return Err(Failure::new(ErrorCode::OperationFailed, incomplete));
    }
    Ok(Dependency {
        version: version
            .parse()
            .expect("the host's own version requirement is valid"),
        optional: false,
        methods,
    })
}

pub(crate) fn execution_dependency() -> Result<Dependency, Failure> {
    let declaration: Value = serde_json::from_str(
        r#"{"version":"2.0.0","methods":{
            "execute":{
                "parameters":{"type":"record","fields":{
                    "program":{"type":"string","max_bytes":4096},
                    "args":{"type":"array","max_items":128,"items":{"type":"string","max_bytes":4096}},
                    "cwd":{"type":"string","max_bytes":4096},
                    "name":{"type":"string","max_bytes":256},
                    "env":{"type":"array","max_items":64,"items":{"type":"record","fields":{
                        "name":{"type":"string","max_bytes":128},
                        "value":{"type":"string","max_bytes":32768}}}}},
                    "optional":["cwd","name","env"]},
                "result":{"type":"record","fields":{
                    "session":{"type":"string","max_bytes":128},
                    "state":{"type":"string","max_bytes":32}}},
                "permissions":["process.exec","ui.panels"]},
            "stop":{
                "parameters":{"type":"record","fields":{
                    "session":{"type":"string","max_bytes":128},
                    "mode":{"type":"string","max_bytes":16}},"optional":["mode"]},
                "result":{"type":"record","fields":{
                    "session":{"type":"string","max_bytes":128},
                    "state":{"type":"string","max_bytes":32}}},
                "permissions":["process.exec"]},
            "status":{
                "parameters":{"type":"record","fields":{
                    "session":{"type":"string","max_bytes":128}}},
                "result":{"type":"record","fields":{
                    "session":{"type":"string","max_bytes":128},
                    "state":{"type":"string","max_bytes":32},
                    "code":{"type":"integer","min":0,"max":4294967295}},
                    "optional":["code"]},
                "permissions":["process.exec"]}}}"#,
    )
    .expect("execution contract declaration is valid JSON");
    let contract: plugin_protocol::service::Contract = serde_json::from_value(declaration)
        .map_err(|error| Failure::new(ErrorCode::OperationFailed, error.to_string()))?;
    let mut methods = contract.methods;
    methods.extend(plugin_protocol::execution::observation_methods());
    if !methods.contains_key("execute")
        || !methods.contains_key("stop")
        || !methods.contains_key("status")
    {
        return Err(Failure::new(
            ErrorCode::OperationFailed,
            "Execution contract is incomplete",
        ));
    }
    Ok(Dependency {
        // A newer provider may add methods, but these two must keep their exact shape.
        version: "^2"
            .parse()
            .expect("execution version requirement is valid"),
        optional: false,
        methods,
    })
}

/// The host principal for one logical scope; it can never borrow another instance's resources.
pub(crate) fn host_caller(scope: &str) -> Caller {
    Caller {
        plugin: "me-editor".into(),
        instance: format!("host@{scope}"),
        scope: scope.to_owned(),
        permissions: ["process.exec", "ui.panels", "editor.read", "workspace.read"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
    }
}

/// The host as a broker participant offering its session contract for one logical scope.
///
/// Aliveness is the runtime's own flag, so the registration disappears with the runtime that owns
/// the sessions rather than outliving them: a consumer can only reach sessions that still exist.
pub(crate) fn session_provider(
    scope: &str,
    alive: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> crate::plugin_services::Provider {
    use plugin_protocol::service::{Contract, Method};
    let mut methods = std::collections::BTreeMap::new();
    // Starting, listing, querying and stopping are the four operations the ticket names; each is a
    // declared shape, so a consumer can only ask for what the host advertises. A session is named by
    // the string the host publishes, which is what the title bar and the consumer both address.
    let mut declare = |name: &str, parameters: &str, result: &str| {
        methods.insert(
            name.to_owned(),
            Method {
                parameters: serde_json::from_str(parameters).expect("session schema is valid"),
                result: serde_json::from_str(result).expect("session schema is valid"),
                // Delegation never grants authority: the caller must authorize every effect.
                permissions: match name {
                    "start" => ["process.exec", "ui.panels"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect(),
                    "stop" => ["process.exec"].into_iter().map(str::to_owned).collect(),
                    _ => Default::default(),
                },
            },
        );
    };
    declare(
        "start",
        r#"{"type":"record","fields":{
            "program":{"type":"string","max_bytes":4096},
            "args":{"type":"array","max_items":128,"items":{"type":"string","max_bytes":4096}},
            "cwd":{"type":"string","max_bytes":4096},
            "name":{"type":"string","max_bytes":256},
            "configuration":{"type":"string","max_bytes":256},
            "env":{"type":"array","max_items":64,"items":{"type":"record","fields":{
                "name":{"type":"string","max_bytes":128},
                "value":{"type":"string","max_bytes":32768}}}}},
            "optional":["cwd","name","env","configuration"]}"#,
        r#"{"type":"record","fields":{
            "session":{"type":"string","max_bytes":128},
            "state":{"type":"string","max_bytes":32},
            "located":{"type":"boolean"}}}"#,
    );
    declare(
        "list",
        r#"{"type":"record","fields":{}}"#,
        r#"{"type":"record","fields":{
            "sessions":{"type":"array","max_items":64,"items":{"type":"record","fields":{
                "session":{"type":"string","max_bytes":128},
                "state":{"type":"string","max_bytes":32}}}}}}"#,
    );
    declare(
        "status",
        r#"{"type":"record","fields":{"session":{"type":"string","max_bytes":128}}}"#,
        r#"{"type":"record","fields":{
            "session":{"type":"string","max_bytes":128},
            "state":{"type":"string","max_bytes":32}}}"#,
    );
    declare(
        "stop",
        r#"{"type":"record","fields":{
            "session":{"type":"string","max_bytes":128},
            "mode":{"type":"string","max_bytes":16},
            "grace_ms":{"type":"integer","min":1,"max":60000}},"optional":["mode","grace_ms"]}"#,
        r#"{"type":"record","fields":{
            "session":{"type":"string","max_bytes":128},
            "state":{"type":"string","max_bytes":32}}}"#,
    );
    methods.extend(subscriptions::methods());
    crate::plugin_services::Provider {
        alive,
        caller: host_caller(scope),
        contracts: [(
            SESSION_CONTRACT.to_owned(),
            Contract {
                version: SESSION_CONTRACT_VERSION
                    .parse()
                    .expect("session contract version is valid"),
                methods,
            },
        )]
        .into_iter()
        .collect(),
    }
}

/// Reason a start could not even be requested, before any provider state changed.
pub(crate) fn start_failure(error: Failure) -> anyhow::Error {
    anyhow::anyhow!("{:?}: {}", error.code, error.message)
}

/// The four operations the host answers on its session contract, as method names.
///
/// Only checks name them for now: a consumer learns them from the declaration it opens rather than
/// from a list here, so this exists to assert that the declaration and the answer agree.
#[cfg(test)]
const SESSION_METHODS: [&str; 9] = [
    "start",
    "list",
    "status",
    "stop",
    "input",
    "locate",
    "subscribe",
    "next",
    "unsubscribe",
];

/// What a consumer requires of the host's session contract.
///
/// A consumer states this to open the contract, and resolution compares it against what the host
/// advertises, so asking for an operation the host does not offer is refused at open time rather
/// than at the moment it is called.
///
/// Built from the host's own declaration, which is why it can be derived rather than repeated. The
/// application does not consult it — the host is the provider, not a consumer of its own contract —
/// and a check uses it to resolve the contract through the same path a guest would.
#[cfg(test)]
pub(crate) fn session_dependency() -> Result<Dependency, Failure> {
    let provider = session_provider(
        "dependency",
        Arc::new(std::sync::atomic::AtomicBool::new(true)),
    );
    let contract = provider
        .contracts
        .get(SESSION_CONTRACT)
        .cloned()
        .ok_or_else(|| Failure::new(ErrorCode::UnsupportedOperation, "Unknown session contract"))?;
    Ok(Dependency {
        version: SESSION_CONTRACT_VERSION
            .parse()
            .expect("session contract version is valid"),
        optional: false,
        methods: contract.methods,
    })
}

/// Answer one call on the host's session contract from the table the title bar reads.
///
/// A consumer and the title bar therefore see one session, not two: `start` locates an existing
/// session with the same identity instead of creating a second one, which is the rule the title bar
/// already follows, and `list`, `status` and `stop` address that same entry. Nothing here decides
/// which provider answers — the session records the incarnation it was started under, and stopping
/// is refused once that incarnation is gone.
pub(crate) fn session_answer(
    manager: &mut Manager,
    context: &CallContext,
    method: &str,
    arguments: &Value,
) -> Result<Value, Failure> {
    let missing = |field: &str| {
        Failure::new(
            ErrorCode::InvalidRequest,
            format!("Session method {method} requires {field}"),
        )
    };
    let session_of = |manager: &Manager, arguments: &Value| {
        let session = arguments
            .get("session")
            .and_then(Value::as_str)
            .ok_or_else(|| missing("session"))?;
        let id = session
            .parse::<u64>()
            .map_err(|_| missing("a numeric session"))?;
        manager
            .host_sessions
            .get(id)
            .filter(|session| session.visible_to(&context.caller))
            .ok_or_else(|| Failure::new(ErrorCode::InvalidHandle, "Unknown or foreign session"))
    };
    // A caller may only address the workspace it is scoped to; two workspaces never share a table.
    if context.caller.scope != manager.host_scope() {
        return Err(Failure::new(
            ErrorCode::PermissionDenied,
            "Session belongs to another workspace",
        ));
    }
    if !context
        .lifetimes
        .iter()
        .all(|alive| alive.load(Ordering::Acquire))
    {
        return Err(Failure::new(
            ErrorCode::InvalidHandle,
            "Session source has retired",
        ));
    }
    // Recheck at dispatch as well as at the guest boundary, including queued calls from older declarations.
    let required: &[&str] = match method {
        "start" => &["process.exec", "ui.panels"],
        "stop" => &["process.exec"],
        _ => &[],
    };
    if required
        .iter()
        .any(|permission| !context.permissions.contains(*permission))
    {
        return Err(Failure::new(
            ErrorCode::PermissionDenied,
            "Session operation requires delegated authority",
        ));
    }
    match method {
        "start" => {
            let mut launch = arguments.clone();
            let configuration = launch.as_object_mut().unwrap().remove("configuration");
            let configuration = configuration.as_ref().and_then(Value::as_str);
            let request: RunRequest = serde_json::from_value(launch)
                .map_err(|error| Failure::new(ErrorCode::InvalidRequest, error.to_string()))?;
            // The rule the title bar follows: a launch that is already known locates its session.
            let dedup_key = execution_identity(&request, configuration)?;
            if let Some(existing) = manager.host_sessions.find(&dedup_key, &context.caller) {
                let snapshot = existing.snapshot();
                return Ok(serde_json::json!({
                    "session": snapshot.id.to_string(),
                    "state": snapshot.state.as_str(),
                    "located": true,
                }));
            }
            // A launch nothing can serve is refused as a missing capability, before any session is
            // recorded: the title bar must not offer to stop a program that was never started. The
            // reason the listed providers give is what a consumer acts on.
            let usable = manager
                .execution_providers()
                .into_iter()
                .any(|candidate| candidate.unavailable.is_none());
            if !usable {
                return Err(Failure::new(
                    ErrorCode::CapabilityUnavailable,
                    format!("No provider can serve {EXECUTION_CONTRACT} in this workspace"),
                ));
            }
            let session = manager
                .start_execution_from(request, context.clone(), configuration, false)
                .map_err(|error| Failure::new(ErrorCode::OperationFailed, format!("{error:#}")))?;
            let snapshot = session.snapshot();
            Ok(serde_json::json!({
                "session": snapshot.id.to_string(),
                "state": snapshot.state.as_str(),
                "located": false,
            }))
        }
        "list" => {
            let sessions = manager
                .host_sessions
                .iter()
                .filter(|session| session.visible_to(&context.caller))
                .map(|session| {
                    let snapshot = session.snapshot();
                    serde_json::json!({
                        "session": snapshot.id.to_string(),
                        "state": snapshot.state.as_str(),
                    })
                })
                .collect::<Vec<_>>();
            Ok(serde_json::json!({ "sessions": sessions }))
        }
        "status" => {
            let snapshot = session_of(manager, arguments)?.snapshot();
            Ok(serde_json::json!({
                "session": snapshot.id.to_string(),
                "state": snapshot.state.as_str(),
            }))
        }
        "subscribe" => {
            let session = session_of(manager, arguments)?;
            manager.subscribe_execution(context, session.id())
        }
        "unsubscribe" => manager.unsubscribe_execution(context, arguments),
        "stop" => {
            let session = session_of(manager, arguments)?;
            // Control options cannot change the session identity, authority or provider ownership.
            let mut options = arguments.clone();
            options.as_object_mut().unwrap().remove("session");
            let options: StopOptions = serde_json::from_value(options)
                .map_err(|error| Failure::new(ErrorCode::InvalidRequest, error.to_string()))?;
            manager
                .stop_execution_with(session.id(), options)
                .map_err(|error| Failure::new(ErrorCode::OperationFailed, format!("{error:#}")))?;
            let snapshot = session.snapshot();
            Ok(serde_json::json!({
                "session": snapshot.id.to_string(),
                "state": snapshot.state.as_str(),
            }))
        }
        other => Err(Failure::new(
            ErrorCode::UnsupportedOperation,
            format!("Unknown session method {other}"),
        )),
    }
}

/// Assemble the broker call for a host start from an already resolved reference.
pub(crate) fn host_call(
    caller: &Caller,
    reference: Reference,
    request: &RunRequest,
    dependency: &Dependency,
    completion: Completion<Value>,
    alive: Arc<AtomicBool>,
) -> Result<Call, Failure> {
    host_method_call(
        caller,
        reference,
        "execute",
        serde_json::to_value(request)
            .map_err(|error| Failure::new(ErrorCode::InvalidRequest, error.to_string()))?,
        dependency,
        completion,
        alive,
    )
}

/// Assemble a call for one method of the execution contract.
///
/// The declared method shape travels with the call, so a provider can only be asked for operations
/// it actually advertised, and the result is validated against that same declaration. The deadline
/// belongs to the completion, which is what a caller observes and cancels.
pub(crate) fn host_method_call(
    caller: &Caller,
    reference: Reference,
    method: &str,
    arguments: Value,
    dependency: &Dependency,
    completion: Completion<Value>,
    alive: Arc<AtomicBool>,
) -> Result<Call, Failure> {
    let signature =
        dependency.methods.get(method).cloned().ok_or_else(|| {
            Failure::new(ErrorCode::UnsupportedOperation, "Unknown execution method")
        })?;
    // The broker keys queued work by known participants, so this request handle names the pinned
    // provider incarnation rather than the host itself: a call queued for a retired provider is
    // abandoned with it, exactly like a guest's queued call.
    let handle = ResourceHandle {
        instance: reference.provider.caller.instance.clone(),
        scope: reference.provider.caller.scope.clone(),
        resource: 0,
    };
    let context = CallContext {
        lifetimes: vec![alive],
        caller: caller.clone(),
        ancestry: Vec::new(),
        permissions: caller.permissions.clone(),
    }
    .delegate(&reference.provider, &signature)?;
    signature.parameters.accepts(&arguments)?;
    Ok(Call {
        handle,
        reference,
        method: method.into(),
        signature,
        arguments,
        context,
        completion,
    })
}

/// The host's execution surface: the run controls consume the same contract as any plugin.
impl Manager {
    /// Workspace whose logical scope owns every session this host starts or asks about.
    ///
    /// Shared with the debug side so a debug call is routed to the same scope its provider was
    /// selected in; two answers about one workspace would otherwise be possible.
    pub(super) fn host_scope(&self) -> String {
        scopes::workspace_key(&self.environment.workspace)
    }
    /// Every retained host session, newest identity last; finished sessions stay locatable by key.
    pub fn executions(&self) -> Vec<HostExecution> {
        let caller = host_caller(&self.host_scope());
        self.host_sessions
            .iter()
            .filter(|entry| entry.visible_to(&caller))
            .cloned()
            .collect()
    }
    /// Look up one session by host identity, including one that has already finished.
    pub fn execution(&self, id: u64) -> Option<HostExecution> {
        self.host_sessions
            .get(id)
            .filter(|entry| entry.visible_to(&host_caller(&self.host_scope())))
    }
    /// A retained session for the same literal command, so repeat clicks reveal instead of duplicating.
    pub fn execution_for(&self, request: &RunRequest, cwd: Option<&str>) -> Option<HostExecution> {
        self.host_sessions
            .find(&request.dedup_key(cwd), &host_caller(&self.host_scope()))
    }
    /// Begin one execution through the selected compatible provider.
    ///
    /// Returns after the request is queued: an accepted provider answer is program creation, not
    /// process exit. Provider selection is by contract and scope, so no language or tool identity
    /// enters host code, and a missing or ambiguous provider fails before anything is started.
    pub fn start_execution(&mut self, request: RunRequest) -> anyhow::Result<HostExecution> {
        let caller = host_caller(&self.host_scope());
        let context = CallContext {
            lifetimes: vec![self.host_alive.clone()],
            permissions: caller.permissions.clone(),
            caller,
            ancestry: Vec::new(),
        };
        self.start_execution_from(request, context, None, false)
    }
    /// A saved configuration owns one active launch even after its literal command is edited.
    /// Different IDs remain independent; an ID does not grant extra source authority.
    pub fn start_configuration_execution(
        &mut self,
        configuration: &str,
        request: RunRequest,
    ) -> anyhow::Result<HostExecution> {
        let caller = host_caller(&self.host_scope());
        let context = CallContext {
            lifetimes: vec![self.host_alive.clone()],
            permissions: caller.permissions.clone(),
            caller,
            ancestry: Vec::new(),
        };
        self.start_execution_from(request, context, Some(configuration), false)
    }
    /// Start one configuration in an existing native VT view, keeping exact source ownership.
    /// Compatible providers negotiate optional cursor inheritance; headless execution remains unchanged.
    /// Returns an asynchronous creation receipt or a trust, schema, quota or capability error.
    pub fn start_terminal_configuration_execution(
        &mut self,
        configuration: &str,
        request: RunRequest,
    ) -> anyhow::Result<HostExecution> {
        let caller = host_caller(&self.host_scope());
        let context = CallContext {
            lifetimes: vec![self.host_alive.clone()],
            permissions: caller.permissions.clone(),
            caller,
            ancestry: Vec::new(),
        };
        self.start_execution_from(request, context, Some(configuration), true)
    }
    /// Forward the source's shrinking authority and lifetime through the host gateway.
    fn start_execution_from(
        &mut self,
        request: RunRequest,
        context: CallContext,
        configuration: Option<&str>,
        terminal: bool,
    ) -> anyhow::Result<HostExecution> {
        let dedup_key = execution_identity(&request, configuration).map_err(start_failure)?;
        if let Some(existing) = self.host_sessions.find(&dedup_key, &context.caller) {
            return Ok(existing);
        }
        request.validate().map_err(start_failure)?;
        self.host_sessions
            .reserve_capacity()
            .map_err(start_failure)?;
        let mut dependency = execution_dependency().map_err(start_failure)?;
        let caller = &context.caller;
        self.refresh_services();
        let (mut reference, provider) = {
            let broker = self.plugin_services.lock().unwrap();
            let reference = broker
                .resolve(&caller, EXECUTION_CONTRACT, &dependency)
                .map_err(start_failure)?;
            let provider = reference.provider.clone();
            (reference, provider)
        };
        let method = if terminal
            && provider.contracts[EXECUTION_CONTRACT]
                .methods
                .get("execute_terminal")
                == Some(&plugin_protocol::execution::terminal_execute_method())
        {
            dependency.methods.insert(
                "execute_terminal".into(),
                plugin_protocol::execution::terminal_execute_method(),
            );
            reference = self
                .plugin_services
                .lock()
                .unwrap()
                .resolve_pinned(
                    caller,
                    EXECUTION_CONTRACT,
                    &dependency,
                    &provider.caller.instance,
                )
                .map_err(start_failure)?;
            "execute_terminal"
        } else {
            "execute"
        };
        let mut completion = Completion::new(EXECUTION_START_TIMEOUT_MS);
        completion.lifetimes = context.lifetimes.clone();
        let execution = self
            .host_sessions
            .insert(
                provider,
                request.clone(),
                dedup_key,
                completion.clone(),
                context,
            )
            .map_err(start_failure)?;
        let queued = (|| {
            let mut arguments = serde_json::to_value(&request)?;
            if method == "execute_terminal" {
                arguments["inherit_cursor"] = cfg!(windows).into();
            }
            let mut call = host_method_call(
                &execution.origin.caller,
                reference,
                method,
                arguments,
                &dependency,
                completion.clone(),
                self.host_alive.clone(),
            )
            .map_err(start_failure)?;
            call.context = execution
                .origin
                .delegate(&call.reference.provider, &call.signature)
                .map_err(start_failure)?;
            call.completion.lifetimes = call.context.lifetimes.clone();
            self.plugin_services
                .lock()
                .unwrap()
                .enqueue(call)
                .map_err(start_failure)
        })();
        if let Err(error) = queued {
            // Refused admission cannot retain a live delegation token or consume activity capacity.
            execution.execution_alive.store(false, Ordering::Release);
            return Err(error);
        }
        Ok(execution)
    }

    /// Poll until the given request is answered, or until this call's own bound expires.
    ///
    /// A caller that must act on one specific answer — whether a preparation step ended, for example
    /// — should not have to guess how many poll rounds another guest needs. The loop gives the guest
    /// a short window to answer rather than waiting out the request's full timeout, so a caller that
    /// observes every frame keeps making progress while a genuinely unanswerable request is bounded.
    pub fn poll_request<T: Clone>(&mut self, completion: &Completion<T>) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        loop {
            self.poll();
            match completion.status() {
                // Neither an unconsumed nor an in-flight request is an answer: the guest still has
                // work to do, and this loop is what gives it the chance.
                RequestUpdate::Progress { .. } | RequestUpdate::Accepted => {}
                RequestUpdate::Completed { .. } | RequestUpdate::Cancelled { .. } => return,
            }
            if std::time::Instant::now() >= deadline {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    /// Give all programs one shared normal-exit window before force and final instance cleanup.
    ///
    /// Shutdown runs on the actor, while the native window awaits its acknowledgement. Programs stop
    /// concurrently rather than each consuming a full timeout. Provider acceptance never shortens
    /// this wait: only observed termination or revoked ownership ends it.
    pub(super) fn stop_owned_programs(&mut self) {
        // Debuggers own their targets as a separate resource tree; disconnect before parking a scope.
        self.stop_owned_debuggers();
        let active = self
            .host_sessions
            .iter()
            .filter(|execution| {
                execution.origin.caller.scope == self.host_scope() && execution.stoppable()
            })
            .map(|execution| execution.id)
            .collect::<Vec<_>>();
        for session in &active {
            let _ = self.stop_execution(*session);
        }
        for (mode, wait_ms) in [
            (
                plugin_protocol::process::ExitMode::Graceful,
                DEFAULT_STOP_GRACE_MS,
            ),
            (
                plugin_protocol::process::ExitMode::Force,
                EXECUTION_STOP_TIMEOUT_MS,
            ),
        ] {
            if mode == plugin_protocol::process::ExitMode::Force {
                for session in &active {
                    let _ = self.stop_execution_with(
                        *session,
                        StopOptions {
                            mode,
                            ..Default::default()
                        },
                    );
                }
            }
            let deadline =
                std::time::Instant::now() + std::time::Duration::from_millis(wait_ms.into());
            while active.iter().any(|id| {
                self.execution(*id)
                    .is_some_and(|entry| entry.state().is_active())
            }) && std::time::Instant::now() < deadline
            {
                self.poll();
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        }
        // A silent provider cannot defer window shutdown indefinitely. Native instance teardown
        // below releases all remaining jobs; revocation prevents a delayed start from escaping it.
        for session in active.into_iter().filter_map(|id| self.execution(id)) {
            session.execution_alive.store(false, Ordering::Release);
        }
    }

    /// Every installed plugin that declares a run execution contract, with its availability.
    ///
    /// This is descriptive: it never changes which provider is selected and holds no session.
    pub fn execution_providers(&self) -> Vec<ProviderCandidate> {
        self.contract_providers(EXECUTION_CONTRACT, execution_dependency().ok().as_ref())
    }

    /// Every installed plugin that declares one contract, with its availability and the selected one.
    ///
    /// A provider that is installed but unusable is still listed, because its reason is what a user
    /// needs to act on: hiding it would make an incomplete choice look like the only one. The
    /// selection is asked about the same logical scope a session would use, so the answer describes
    /// what would actually happen rather than what some other scope's preference says.
    pub(super) fn contract_providers(
        &self,
        contract: &str,
        dependency: Option<&Dependency>,
    ) -> Vec<ProviderCandidate> {
        let scope = self.host_scope();
        let selected = self
            .plugin_services
            .lock()
            .map(|broker| broker.selected_provider(&scope, contract))
            .ok()
            .flatten();
        let mut candidates = Vec::new();
        // Native participants use the same registry and shape matching as installed providers.
        for provider in self
            .plugin_services
            .lock()
            .unwrap()
            .participants(&scope, contract)
        {
            if self.installed.contains_key(&provider.caller.plugin) {
                continue;
            }
            let unavailable = if !self.trusted || !self.workspace_open {
                Some("Workspace restricted".into())
            } else if dependency
                .is_some_and(|dependency| !dependency.matches(&provider.contracts[contract]))
            {
                Some("Contract methods are incompatible".into())
            } else {
                None
            };
            candidates.push(ProviderCandidate {
                selected: selected.as_deref() == Some(provider.caller.plugin.as_str()),
                unavailable,
                plugin: provider.caller.plugin,
            });
        }
        for installed in self.installed.values() {
            let Some(declaration) = installed.manifest.plugin_services.provides.get(contract)
            else {
                continue;
            };
            let unavailable = if !installed.enabled {
                Some("该插件未启用".to_owned())
            } else if let Some(error) = installed.compatibility_error() {
                Some(format!("与当前宿主不兼容：{error}"))
            } else if let Some(error) = &installed.error {
                Some(format!("插件运行出错：{error}"))
            } else if let Some(dependency) = dependency
                && !dependency.matches(declaration)
            {
                Some(format!("声明的 {contract} 契约与方法不完整"))
            } else {
                None
            };
            let plugin = installed.manifest.id.clone();
            candidates.push(ProviderCandidate {
                selected: selected.as_deref() == Some(plugin.as_str()),
                unavailable,
                plugin,
            });
        }
        candidates.sort_by(|left, right| left.plugin.cmp(&right.plugin));
        candidates
    }
    /// Ask a session's own provider what became of the program it started.
    ///
    /// Returns the request whose answer carries the observation, so a caller can wait for it and
    /// act on a real exit instead of assuming one. The answer is the provider's, never a prediction
    /// derived from elapsed time or from output the host happens to have seen.
    pub fn query_execution(&mut self, session: u64) -> anyhow::Result<Completion<Value>> {
        let execution = self
            .execution(session)
            .ok_or_else(|| anyhow::anyhow!("Unknown execution session {session}"))?;
        anyhow::ensure!(
            execution.provider_active(),
            "Execution session {session} is not running under an available provider"
        );
        let provider_session = execution
            .snapshot()
            .provider_session
            .ok_or_else(|| anyhow::anyhow!("Provider reported no session identity to query"))?;
        let dependency = execution_dependency().map_err(start_failure)?;
        let caller = &execution.origin.caller;
        self.refresh_services();
        let reference = {
            let broker = self.plugin_services.lock().unwrap();
            let reference = broker
                .resolve_pinned(
                    caller,
                    EXECUTION_CONTRACT,
                    &dependency,
                    execution.provider_instance(),
                )
                .map_err(start_failure)?;
            reference
        };
        let mut completion = Completion::new(EXECUTION_STATUS_TIMEOUT_MS);
        completion.lifetimes = execution.origin.lifetimes.clone();
        let mut call = host_method_call(
            caller,
            reference,
            "status",
            serde_json::json!({ "session": provider_session }),
            &dependency,
            completion.clone(),
            self.host_alive.clone(),
        )
        .map_err(start_failure)?;
        call.context = execution
            .origin
            .delegate(&call.reference.provider, &call.signature)
            .map_err(start_failure)?;
        call.completion.lifetimes = call.context.lifetimes.clone();
        if let Err(error) = self.plugin_services.lock().unwrap().enqueue(call) {
            return Err(start_failure(error));
        }
        // A query is not a session: it observes one. The caller owns the gate and reads the answer
        // from it, so a status refresh never appears in the run controls as another program.
        Ok(completion)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plugin_protocol::service::Method;

    /// The host is one more consumer: its declared method shape must equal the maintained provider's.
    ///
    /// A mismatch would be reported to the user as an unavailable capability, so the drift is caught
    /// here instead of after a release.
    #[test]
    fn host_execution_dependency_matches_the_maintained_provider_manifest() {
        let manifest: Value =
            serde_json::from_str(include_str!("../../../../plugins/terminal/manifest.json"))
                .unwrap();
        let declared = &manifest["plugin_services"]["provides"][EXECUTION_CONTRACT];
        let contract: plugin_protocol::service::Contract =
            serde_json::from_value(declared.clone()).unwrap();
        let dependency = execution_dependency().unwrap();
        assert!(
            dependency.matches(&contract),
            "host execution dependency drifted from the maintained provider declaration"
        );
    }

    /// A literal command keeps one identity, and different arguments never collapse into it.
    #[test]
    fn execution_requests_hash_their_literal_arguments() {
        let base = RunRequest {
            program: "cargo.exe".into(),
            args: vec!["run".into()],
            cwd: None,
            name: Some("运行".into()),
            env: Vec::new(),
        };
        let same = base.clone();
        let other = RunRequest {
            args: vec!["test".into()],
            ..base.clone()
        };
        assert_eq!(base.dedup_key(None), same.dedup_key(None));
        assert_ne!(base.dedup_key(None), other.dedup_key(None));
        // A per-configuration directory participates in the identity instead of being ignored.
        assert_ne!(
            base.dedup_key(Some("C:/one")),
            base.dedup_key(Some("C:/two"))
        );
    }

    /// Malformed requests are refused before a provider queue entry can be created.
    #[test]
    fn malformed_execution_requests_are_rejected() {
        let empty = RunRequest {
            program: "  ".into(),
            args: vec![],
            cwd: None,
            name: None,
            env: Vec::new(),
        };
        assert!(empty.validate().is_err());
        let unbounded = RunRequest {
            program: "tool.exe".into(),
            args: vec!["x".repeat(4097)],
            cwd: None,
            name: None,
            env: Vec::new(),
        };
        assert!(unbounded.validate().is_err());
        let control_label = RunRequest {
            program: "tool.exe".into(),
            args: vec![],
            cwd: None,
            name: Some("bad\nlabel".into()),
            env: Vec::new(),
        };
        assert!(control_label.validate().is_err());
    }

    /// The host offers its session contract as a broker participant, for the workspace it owns.
    #[test]
    fn the_host_offers_its_session_contract_for_one_workspace() {
        let alive = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let provider = session_provider("C:/work", alive.clone());
        // The caller identifies the host itself, scoped to one workspace, so two workspaces cannot
        // address each other's sessions.
        assert_eq!(provider.caller.plugin, "me-editor");
        assert_eq!(provider.caller.instance, "host@C:/work");
        assert_eq!(provider.caller.scope, "C:/work");
        let contract = provider
            .contracts
            .get(SESSION_CONTRACT)
            .expect("the host offers the session contract");
        assert_eq!(contract.version.to_string(), SESSION_CONTRACT_VERSION);
        // Launch/control and bounded observation use one source-owned public contract.
        let names = contract.methods.keys().cloned().collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "input",
                "list",
                "locate",
                "next",
                "start",
                "status",
                "stop",
                "subscribe",
                "unsubscribe"
            ]
        );
        for method in contract.methods.values() {
            // A declared shape is what a consumer's dependency is matched against, so an empty one
            // would let a consumer require nothing and still be told it matches.
            assert_ne!(
                serde_json::to_value(&method.parameters).unwrap(),
                serde_json::json!(null),
                "every session method declares its parameters"
            );
            assert_ne!(
                serde_json::to_value(&method.result).unwrap(),
                serde_json::json!(null),
                "every session method declares its result"
            );
        }
        // The registration is only as alive as the runtime that owns the sessions.
        alive.store(false, std::sync::atomic::Ordering::Release);
        assert!(!provider.alive.load(std::sync::atomic::Ordering::Acquire));
    }

    /// The consumer's session surface refuses what it cannot answer, and only for its own workspace.
    ///
    /// This is the boundary a consumer meets before any provider is involved: a launch with nothing
    /// able to serve it is refused rather than recorded, an unknown identity is not answered with a
    /// different session, another workspace's sessions are not addressable, and a method the host does
    /// not offer is not guessed at. That the two entry points then share one session entry is checked
    /// against a real provider in `host_controls_start_once_and_locate_the_retained_session`.
    #[test]
    fn the_consumer_session_surface_refuses_what_it_cannot_answer() {
        use plugin_protocol::api::ErrorCode;
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let mut manager = Manager::open(
            root.path().join("plugins"),
            plugin_protocol::Environment {
                workspace: workspace.display().to_string(),
                os: "windows".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let scope = manager.host_scope().to_owned();
        let context = CallContext {
            caller: host_caller(&scope),
            permissions: host_caller(&scope).permissions,
            lifetimes: vec![manager.host_alive.clone()],
            ancestry: vec![],
        };
        // Nothing can serve this launch, so it is refused instead of leaving a failed session that
        // the title bar would then offer to stop.
        let refused = session_answer(
            &mut manager,
            &context,
            "start",
            &serde_json::json!({"program":"tool.exe","args":[]}),
        )
        .unwrap_err();
        assert_eq!(refused.code, ErrorCode::CapabilityUnavailable);
        assert!(
            manager.executions().is_empty(),
            "a refused launch leaves no session behind"
        );
        // An unknown identity is refused rather than answered with whatever else is there.
        let unknown = session_answer(
            &mut manager,
            &context,
            "status",
            &serde_json::json!({"session": "9999"}),
        )
        .unwrap_err();
        assert_eq!(unknown.code, ErrorCode::InvalidHandle);
        // A missing identity is a malformed request, not an unknown one.
        let missing =
            session_answer(&mut manager, &context, "stop", &serde_json::json!({})).unwrap_err();
        assert_eq!(missing.code, ErrorCode::InvalidRequest);
        // Another workspace cannot address this one's sessions, whatever it asks.
        for method in SESSION_METHODS {
            let foreign = session_answer(
                &mut manager,
                &CallContext {
                    caller: host_caller("C:/somewhere-else"),
                    ..context.clone()
                },
                method,
                &serde_json::json!({}),
            )
            .unwrap_err();
            assert_eq!(foreign.code, ErrorCode::PermissionDenied, "{method}");
        }
        // A method the host does not offer is refused rather than guessed at.
        let unsupported =
            session_answer(&mut manager, &context, "attach", &serde_json::json!({})).unwrap_err();
        assert_eq!(unsupported.code, ErrorCode::UnsupportedOperation);
        // Listing an empty table is an answer, not an error: there is simply nothing running.
        let empty = session_answer(&mut manager, &context, "list", &serde_json::json!({})).unwrap();
        assert_eq!(empty["sessions"].as_array().unwrap().len(), 0);
        manager.shutdown();
    }
}
