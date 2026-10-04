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
        Arc,
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
/// The host's session contract version. A consumer requires a family of it, such as `^1`, so the
pub const SESSION_CONTRACT_VERSION: &str = "1.0.0";
/// host can answer a later compatible revision without every consumer changing.
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
    /// The request was refused, timed out, or its provider retired before answering.
    Failed,
}

impl ExecutionState {
    /// The name a consumer sees. These are part of the session contract, so they are stated once.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Failed => "failed",
        }
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
    plugin: String,
    /// Exact provider incarnation pinned at start time; its retirement fails this session.
    provider_instance: String,
    provider_alive: Arc<AtomicBool>,
    dedup_key: String,
    request: RunRequest,
    completion: Completion<Value>,
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
        self.completion.status()
    }
    /// The provider incarnation this session is bound to; later selections never retarget it.
    pub fn provider_instance(&self) -> &str {
        &self.provider_instance
    }
    /// Whether the pinned provider is still the live incarnation that answered this session.
    pub fn provider_active(&self) -> bool {
        self.provider_alive.load(Ordering::Acquire)
            && !self.provider_retired.load(Ordering::Acquire)
    }
    /// The provider incarnation this session is pinned to, for diagnostics and host publications.
    pub fn provider_identity(&self) -> (&str, &str) {
        (&self.plugin, &self.provider_instance)
    }
    /// Publishable view for the run controls; derived without mutating the session.
    pub fn snapshot(&self) -> ExecutionSnapshot {
        let (mut state, provider_session, failure) = self.lifecycle();
        // A session outlives its provider only as a visible result: the program it started is no
        // longer managed by this runtime, so it is never reported as still running.
        if state == ExecutionState::Running && !self.provider_active() {
            state = ExecutionState::Failed;
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
        match self.completion.status() {
            RequestUpdate::Accepted | RequestUpdate::Progress { .. } => {
                (ExecutionState::Starting, None, None)
            }
            RequestUpdate::Completed { result: Ok(value) } => (
                // The provider already confirmed program creation; the host adds no stronger claim.
                ExecutionState::Running,
                provider_session_of(&value),
                None,
            ),
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
        let _ = self.completion.cancel(
            plugin_protocol::api::CancelMode::TryTerminate,
            ErrorCode::Cancelled,
        );
    }

    /// Whether a stop is meaningful: the provider confirmed a program and is still present.
    pub fn stoppable(&self) -> bool {
        self.state() == ExecutionState::Running && self.provider_active()
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
}

impl HostSessions {
    pub(crate) fn new(alive: Arc<AtomicBool>) -> Self {
        Self {
            next_id: 1,
            entries: BTreeMap::new(),
            alive,
        }
    }
    /// A session for the same literal command, if one is retained for this scope.
    pub(crate) fn find(&self, dedup_key: &str) -> Option<HostExecution> {
        self.entries
            .values()
            .find(|entry| entry.dedup_key == dedup_key)
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
    ) -> HostExecution {
        // Only finished entries are evicted; a program that is still active is never dropped silently.
        while self.entries.len() >= MAX_HOST_EXECUTIONS {
            let candidate = self
                .entries
                .iter()
                .find(|(_, entry)| entry.snapshot().state != ExecutionState::Starting)
                .map(|(id, _)| *id);
            match candidate {
                Some(id) => {
                    self.entries.remove(&id);
                }
                None => break,
            }
        }
        let id = self.next_id;
        self.next_id += 1;
        let execution = HostExecution {
            id,
            plugin: provider.caller.plugin.clone(),
            provider_instance: provider.caller.instance.clone(),
            provider_alive: provider.alive.clone(),
            dedup_key,
            request,
            completion,
            provider_retired: Arc::new(AtomicBool::new(false)),
        };
        self.entries.insert(id, execution.clone());
        execution
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
    }
}

/// Extract a provider-reported session identity without imposing a provider-specific result shape.
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
        r#"{"version":"1.3.0","methods":{
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
                    "session":{"type":"string","max_bytes":128}}},
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
                    "code":{"type":"integer","min":0,"max":2147483647}},
                    "optional":["code"]},
                "permissions":["process.exec"]}}}"#,
    )
    .expect("execution contract declaration is valid JSON");
    let contract: plugin_protocol::service::Contract = serde_json::from_value(declaration)
        .map_err(|error| Failure::new(ErrorCode::OperationFailed, error.to_string()))?;
    let methods = contract.methods;
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
        version: ">=1.3, <2"
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
                permissions: std::collections::BTreeSet::new(),
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
            "env":{"type":"array","max_items":64,"items":{"type":"record","fields":{
                "name":{"type":"string","max_bytes":128},
                "value":{"type":"string","max_bytes":32768}}}}},
            "optional":["cwd","name","env"]}"#,
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
        r#"{"type":"record","fields":{"session":{"type":"string","max_bytes":128}}}"#,
        r#"{"type":"record","fields":{
            "session":{"type":"string","max_bytes":128},
            "state":{"type":"string","max_bytes":32}}}"#,
    );
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
pub(crate) const SESSION_METHODS: [&str; 4] = ["start", "list", "status", "stop"];

/// Answer one call on the host's session contract from the table the title bar reads.
///
/// A consumer and the title bar therefore see one session, not two: `start` locates an existing
/// session with the same identity instead of creating a second one, which is the rule the title bar
/// already follows, and `list`, `status` and `stop` address that same entry. Nothing here decides
/// which provider answers — the session records the incarnation it was started under, and stopping
/// is refused once that incarnation is gone.
pub(crate) fn session_answer(
    manager: &mut Manager,
    caller_scope: &str,
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
            .ok_or_else(|| Failure::new(ErrorCode::InvalidHandle, "Unknown session"))
    };
    // A caller may only address the workspace it is scoped to; two workspaces never share a table.
    if caller_scope != manager.host_scope() {
        return Err(Failure::new(
            ErrorCode::PermissionDenied,
            "Session belongs to another workspace",
        ));
    }
    match method {
        "start" => {
            let request: RunRequest = serde_json::from_value(arguments.clone())
                .map_err(|error| Failure::new(ErrorCode::InvalidRequest, error.to_string()))?;
            request
                .validate()
                .map_err(|error| Failure::new(ErrorCode::InvalidRequest, error.message))?;
            // The rule the title bar follows: a launch that is already known locates its session.
            let dedup_key = request.dedup_key(None);
            if let Some(existing) = manager.host_sessions.find(&dedup_key) {
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
                .start_execution(request)
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
        "stop" => {
            let session = session_of(manager, arguments)?;
            manager
                .stop_execution(session.id())
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
    };
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
        self.host_sessions.iter().cloned().collect()
    }
    /// Look up one session by host identity, including one that has already finished.
    pub fn execution(&self, id: u64) -> Option<HostExecution> {
        self.host_sessions.get(id)
    }
    /// A retained session for the same literal command, so repeat clicks reveal instead of duplicating.
    pub fn execution_for(&self, request: &RunRequest, cwd: Option<&str>) -> Option<HostExecution> {
        self.host_sessions.find(&request.dedup_key(cwd))
    }
    /// Begin one execution through the selected compatible provider.
    ///
    /// Returns after the request is queued: an accepted provider answer is program creation, not
    /// process exit. Provider selection is by contract and scope, so no language or tool identity
    /// enters host code, and a missing or ambiguous provider fails before anything is started.
    pub fn start_execution(&mut self, request: RunRequest) -> anyhow::Result<HostExecution> {
        request.validate().map_err(start_failure)?;
        let dependency = execution_dependency().map_err(start_failure)?;
        let scope = self.host_scope();
        let caller = host_caller(&scope);
        self.refresh_services();
        let (reference, provider) = {
            let broker = self.plugin_services.lock().unwrap();
            let reference = broker
                .resolve(&caller, EXECUTION_CONTRACT, &dependency)
                .map_err(start_failure)?;
            let provider = reference.provider.clone();
            (reference, provider)
        };
        let completion = Completion::new(EXECUTION_START_TIMEOUT_MS);
        let call = host_call(
            &caller,
            reference,
            &request,
            &dependency,
            completion.clone(),
            self.host_alive.clone(),
        )
        .map_err(start_failure)?;
        let dedup_key = request.dedup_key(None);
        if let Err(error) = self.plugin_services.lock().unwrap().enqueue(call) {
            // A refused queue entry must not leave a session that never ran.
            return Err(start_failure(error));
        }
        Ok(self
            .host_sessions
            .insert(provider, request, dedup_key, completion))
    }

    /// Ask the session's own provider to stop the program it started.
    ///
    /// The request is addressed to the pinned provider incarnation and carries the session identity
    /// that provider returned. An acknowledgement means termination was issued, never that the
    /// program has already exited, and the host borrows no provider-private resource handle.
    pub fn stop_execution(&mut self, session: u64) -> anyhow::Result<()> {
        let execution = self
            .host_sessions
            .get(session)
            .ok_or_else(|| anyhow::anyhow!("Unknown execution session {session}"))?;
        anyhow::ensure!(
            execution.stoppable(),
            "Execution session {session} is not running under an available provider"
        );
        let provider_session = execution
            .snapshot()
            .provider_session
            .ok_or_else(|| anyhow::anyhow!("Provider reported no session identity to stop"))?;
        let dependency = execution_dependency().map_err(start_failure)?;
        let scope = self.host_scope();
        let caller = host_caller(&scope);
        self.refresh_services();
        let (reference, provider) = {
            let broker = self.plugin_services.lock().unwrap();
            // The session is pinned to the incarnation that answered it, so a replacement provider
            // can never be asked to stop a program it did not start.
            let reference = broker
                .resolve(&caller, EXECUTION_CONTRACT, &dependency)
                .map_err(start_failure)?;
            if reference.provider.caller.instance != execution.provider_instance() {
                return Err(anyhow::anyhow!(
                    "Execution session {session} belongs to a provider that is no longer selected"
                ));
            }
            let provider = reference.provider.clone();
            (reference, provider)
        };
        // Stopping waits less than a start: a provider that cannot acknowledge promptly is reported
        // instead of leaving the controls waiting on an unreachable session.
        let completion = Completion::new(EXECUTION_STOP_TIMEOUT_MS);
        let call = host_method_call(
            &caller,
            reference,
            "stop",
            serde_json::json!({ "session": provider_session }),
            &dependency,
            completion,
            self.host_alive.clone(),
        )
        .map_err(start_failure)?;
        if let Err(error) = self.plugin_services.lock().unwrap().enqueue(call) {
            return Err(start_failure(error));
        }
        let _ = provider;
        Ok(())
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

    /// Ask every session that still owns a program to stop it, before the runtime goes away.
    ///
    /// Shutting down must not silently abandon programs, so each active session is asked through the
    /// provider that started it, and each answer is given a short window rather than waited on
    /// forever: a provider that cannot acknowledge promptly still must not hold up closing the
    /// window. Nothing is retried and nothing is claimed — the request means "terminate", never "it
    /// has exited" — so a provider that does not answer leaves its program's fate where it was.
    ///
    /// This covers the target a session owns. A debugger or adapter a provider started for itself is
    /// the provider's own process and is released with the provider's instance, which shutdown stops
    /// immediately afterwards.
    pub(super) fn stop_owned_programs(&mut self) {
        let active = self
            .host_sessions
            .iter()
            .filter(|execution| execution.stoppable())
            .map(|execution| execution.id)
            .collect::<Vec<_>>();
        for session in active {
            if self.stop_execution(session).is_err() {
                continue;
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(200);
            while std::time::Instant::now() < deadline {
                self.poll();
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
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
            .host_sessions
            .get(session)
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
        let scope = self.host_scope();
        let caller = host_caller(&scope);
        self.refresh_services();
        let reference = {
            let broker = self.plugin_services.lock().unwrap();
            let reference = broker
                .resolve(&caller, EXECUTION_CONTRACT, &dependency)
                .map_err(start_failure)?;
            if reference.provider.caller.instance != execution.provider_instance() {
                return Err(anyhow::anyhow!(
                    "Execution session {session} belongs to a provider that is no longer selected"
                ));
            }
            reference
        };
        let completion = Completion::new(EXECUTION_STATUS_TIMEOUT_MS);
        let call = host_method_call(
            &caller,
            reference,
            "status",
            serde_json::json!({ "session": provider_session }),
            &dependency,
            completion.clone(),
            self.host_alive.clone(),
        )
        .map_err(start_failure)?;
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
        let version: semver::Version = serde_json::from_value(declared["version"].clone()).unwrap();
        let execute: Method =
            serde_json::from_value(declared["methods"]["execute"].clone()).unwrap();
        let stop: Method = serde_json::from_value(declared["methods"]["stop"].clone()).unwrap();
        let status: Method = serde_json::from_value(declared["methods"]["status"].clone()).unwrap();
        let contract = plugin_protocol::service::Contract {
            version,
            // Every method the host requires is declared here; a missing one is the drift under test.
            methods: BTreeMap::from([
                ("execute".to_owned(), execute),
                ("stop".to_owned(), stop),
                ("status".to_owned(), status),
            ]),
        };
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
        // Exactly the operations the ticket names, each with a declared shape.
        let names = contract.methods.keys().cloned().collect::<Vec<_>>();
        assert_eq!(names, ["list", "start", "status", "stop"]);
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
        // Nothing can serve this launch, so it is refused instead of leaving a failed session that
        // the title bar would then offer to stop.
        let refused = session_answer(
            &mut manager,
            &scope,
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
            &scope,
            "status",
            &serde_json::json!({"session": "9999"}),
        )
        .unwrap_err();
        assert_eq!(unknown.code, ErrorCode::InvalidHandle);
        // A missing identity is a malformed request, not an unknown one.
        let missing =
            session_answer(&mut manager, &scope, "stop", &serde_json::json!({})).unwrap_err();
        assert_eq!(missing.code, ErrorCode::InvalidRequest);
        // Another workspace cannot address this one's sessions, whatever it asks.
        for method in SESSION_METHODS {
            let foreign = session_answer(
                &mut manager,
                "C:/somewhere-else",
                method,
                &serde_json::json!({}),
            )
            .unwrap_err();
            assert_eq!(foreign.code, ErrorCode::PermissionDenied, "{method}");
        }
        // A method the host does not offer is refused rather than guessed at.
        let unsupported =
            session_answer(&mut manager, &scope, "attach", &serde_json::json!({})).unwrap_err();
        assert_eq!(unsupported.code, ErrorCode::UnsupportedOperation);
        // Listing an empty table is an answer, not an error: there is simply nothing running.
        let empty = session_answer(&mut manager, &scope, "list", &serde_json::json!({})).unwrap();
        assert_eq!(empty["sessions"].as_array().unwrap().len(), 0);
        manager.shutdown();
    }
}
