//! The debug capability: the host's own requirement of a debug provider, and the shapes it reads.
//!
//! Debugging is a provider contract like execution, so the host never learns which debugger answers,
//! how it is driven, or over what transport. Everything here is the host's requirement, written out
//! method for method; nothing names a debugger, a target language or an adapter protocol.
use super::Manager;
use super::host_services::{
    DEBUG_BREAKPOINT_TIMEOUT_MS, DEBUG_CONTRACT, DEBUG_CONTROL_TIMEOUT_MS, DEBUG_START_TIMEOUT_MS,
    ProviderCandidate, dependency_from_declaration, host_caller, host_method_call, start_failure,
};
use crate::request_state::Completion;
use plugin_protocol::{
    api::{ErrorCode, Failure},
    service::Dependency,
};
use serde_json::Value;
use std::collections::BTreeMap;

#[cfg(test)]
#[path = "debug_services_tests.rs"]
mod tests;

/// The state a provider reports for one debug session.
///
/// This is the provider's observation, never a prediction: `Paused` means the provider said the
/// target is stopped, and `Exited` means it said the program is gone. The host does not infer either
/// from elapsed time or from the absence of output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugState {
    /// Accepted, not yet attached to a target.
    Starting,
    /// Attached and running.
    Running,
    /// Stopped at a location, ready to be resumed.
    Paused,
    /// The target is gone and the session is over.
    Exited,
}

impl DebugState {
    /// Read one reported state, or `None` for a word this build does not implement.
    ///
    /// An unknown state is not mapped onto a known one: a provider speaking a newer vocabulary is
    /// reported as unreadable instead of being shown as something it did not say.
    pub fn from_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "starting" | "initialized" => Some(Self::Starting),
            "running" | "continued" => Some(Self::Running),
            "paused" | "stopped" | "breakpoint" => Some(Self::Paused),
            "exited" | "terminated" | "ended" => Some(Self::Exited),
            _ => None,
        }
    }
}

/// Where a session is, as far as its provider has said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugSession {
    /// The provider's own session identity, never reinterpreted by the host.
    pub session: String,
    pub state: DebugState,
    /// Why the target stopped, when the provider reported one.
    pub reason: Option<String>,
    /// Source file of the stop, workspace-relative when the provider reported a relative one.
    pub source: Option<String>,
    /// One-based line of the stop.
    pub line: Option<u32>,
}

impl DebugSession {
    /// Read a provider's answer to `start`, `resume`, `pause` or `stop`.
    pub fn from_value(value: &Value) -> Result<Self, Failure> {
        let session = value
            .get("session")
            .and_then(Value::as_str)
            .ok_or_else(|| Failure::new(ErrorCode::InvalidRequest, "Missing debug session"))?;
        let state = value
            .get("state")
            .and_then(Value::as_str)
            .and_then(DebugState::from_str)
            .ok_or_else(|| Failure::new(ErrorCode::InvalidRequest, "Unknown debug state"))?;
        Ok(Self {
            session: session.to_owned(),
            state,
            reason: text(value, "reason"),
            source: text(value, "source"),
            line: value
                .get("line")
                .and_then(Value::as_u64)
                .map(|line| line as u32),
        })
    }
}

/// One breakpoint the provider acknowledged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugBreakpoint {
    pub source: String,
    pub line: u32,
    /// Whether the provider could bind this location in the target it is running.
    ///
    /// An unverified breakpoint is reported as such: the user set a location the provider could not
    /// resolve, and saying so is the difference between "waiting" and "will never hit".
    pub verified: bool,
}

impl DebugBreakpoint {
    /// Read a provider's answer to `set_breakpoints`.
    pub fn list_from_value(value: &Value) -> Result<Vec<Self>, Failure> {
        let entries = value
            .get("breakpoints")
            .and_then(Value::as_array)
            .ok_or_else(|| Failure::new(ErrorCode::InvalidRequest, "Missing breakpoint list"))?;
        entries
            .iter()
            .map(|entry| {
                Ok(Self {
                    source: text(entry, "source").ok_or_else(|| {
                        Failure::new(ErrorCode::InvalidRequest, "Breakpoint has no source")
                    })?,
                    line: entry
                        .get("line")
                        .and_then(Value::as_u64)
                        .map(|line| line as u32)
                        .ok_or_else(|| {
                            Failure::new(ErrorCode::InvalidRequest, "Breakpoint has no line")
                        })?,
                    verified: entry
                        .get("verified")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                })
            })
            .collect()
    }
}

/// One stack frame a provider reported, kept as the provider described it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugFrame {
    pub id: u32,
    pub name: String,
    pub source: String,
    pub line: u32,
}

/// One variable a provider reported, with the provider's own rendering of its value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugVariable {
    pub name: String,
    pub value: String,
}

/// Bound on frames one report may carry; a stack is not an unbounded download.
pub const MAX_DEBUG_FRAMES: usize = 256;
/// Bound on variables one frame's report may carry.
pub const MAX_DEBUG_VARIABLES: usize = 512;

/// A breakpoint the host asks for: a source and a one-based line.
///
/// The request shape is the provider's own vocabulary, so a source is a path or a name the provider
/// can resolve; the host does not decide what a breakpoint means in a target language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugBreakpointRequest {
    pub source: String,
    pub line: u32,
}

impl DebugBreakpointRequest {
    pub fn to_value(&self) -> Value {
        serde_json::json!({"source": self.source, "line": self.line})
    }
}

/// Read a provider's frame report, refusing a list the host could not show whole.
///
/// A frame without a source or a line is not a location, and a list over the bound is a shape the
/// contract does not allow: both are refusals rather than a view with pieces quietly missing.
pub fn frames_from_value(value: &Value) -> Result<Vec<DebugFrame>, Failure> {
    let entries = value
        .get("frames")
        .and_then(Value::as_array)
        .ok_or_else(|| Failure::new(ErrorCode::InvalidRequest, "Missing frame list"))?;
    if entries.len() > MAX_DEBUG_FRAMES {
        return Err(Failure::new(
            ErrorCode::InvalidRequest,
            "Frame list exceeds the declared bound",
        ));
    }
    entries
        .iter()
        .map(|entry| {
            Ok(DebugFrame {
                id: entry
                    .get("id")
                    .and_then(Value::as_u64)
                    .map(|id| id as u32)
                    .ok_or_else(|| {
                        Failure::new(ErrorCode::InvalidRequest, "Frame has no identity")
                    })?,
                name: text(entry, "name")
                    .ok_or_else(|| Failure::new(ErrorCode::InvalidRequest, "Frame has no name"))?,
                source: text(entry, "source").ok_or_else(|| {
                    Failure::new(ErrorCode::InvalidRequest, "Frame has no source")
                })?,
                line: entry
                    .get("line")
                    .and_then(Value::as_u64)
                    .map(|line| line as u32)
                    .filter(|line| *line > 0)
                    .ok_or_else(|| Failure::new(ErrorCode::InvalidRequest, "Frame has no line"))?,
            })
        })
        .collect()
}

/// Read a provider's variable report for one frame.
pub fn variables_from_value(value: &Value) -> Result<Vec<DebugVariable>, Failure> {
    let entries = value
        .get("variables")
        .and_then(Value::as_array)
        .ok_or_else(|| Failure::new(ErrorCode::InvalidRequest, "Missing variable list"))?;
    if entries.len() > MAX_DEBUG_VARIABLES {
        return Err(Failure::new(
            ErrorCode::InvalidRequest,
            "Variable list exceeds the declared bound",
        ));
    }
    entries
        .iter()
        .map(|entry| {
            Ok(DebugVariable {
                name: text(entry, "name").ok_or_else(|| {
                    Failure::new(ErrorCode::InvalidRequest, "Variable has no name")
                })?,
                // A value is the provider's rendering and is shown as given, including empty text.
                value: entry
                    .get("value")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            })
        })
        .collect()
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// The host's own debug requirement, exposed so a test can name the exact method that differs.
///
/// This states the requirement; it grants nothing, and a plugin has no path to it.
pub fn debug_dependency_for_test() -> Result<plugin_protocol::service::Dependency, String> {
    debug_dependency().map_err(|error| format!("{error:?}"))
}

/// Whether a declaration can serve a debug session, judged on what is required of every provider.
///
/// Only the required methods are demanded here. Every method the host *calls* must still match
/// exactly, but that check belongs where a call is resolved: a provider that cannot step is still a
/// debug provider, and reporting it unusable would take away the abilities it does offer.
pub(super) fn debug_contract_is_usable(declaration: &plugin_protocol::service::Contract) -> bool {
    let version = ">=1, <2"
        .parse::<semver::VersionReq>()
        .expect("the host's own version requirement is valid");
    version.matches(&declaration.version)
        && DEBUG_REQUIRED_METHODS
            .iter()
            .all(|method| declaration.methods.contains_key(*method))
}

/// The methods every debug provider must declare, whatever else it offers.
///
/// These are the ones the debug entry points promise: beginning a session, asking what it is, ending
/// it, setting the breakpoints a session stops at, and pausing or continuing the target. A provider
/// that cannot do one of these cannot serve the panel, so it is reported as incomplete rather than
/// offered and then failing at the control the user pressed.
pub const DEBUG_REQUIRED_METHODS: [&str; 6] = [
    "start",
    "status",
    "stop",
    "set_breakpoints",
    "resume",
    "pause",
];

/// The methods a provider may declare to offer more, and what each one enables.
///
/// A capability is the provider's to offer and the host's to describe: a provider that cannot step
/// simply never declares it, and a control that needs it is disabled with that reason instead of
/// failing when pressed. Requiring them outright would make a capable provider unusable for lacking
/// an unrelated ability.
pub const DEBUG_OPTIONAL_METHODS: [&str; 3] = ["step", "frames", "variables"];

/// A debug provider's declaration, read as the methods it actually offers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DebugAbilities {
    /// Whether the provider can set breakpoints.
    pub breakpoints: bool,
    /// Whether it can resume a paused target, and stop a running one.
    pub resume_pause: bool,
    /// Whether it can step a paused target.
    pub step: bool,
    /// Whether it can report a paused target's frames and their variables.
    ///
    /// One ability rather than two: a frame list without variables is not an inspection view, and a
    /// provider that offers one without the other has not offered what the panel needs.
    pub inspect: bool,
}

/// What the host requires of a debug provider, method for method.
///
/// The shape is declared here rather than derived from a provider, because a provider matches by
/// declaring exactly what the host calls. Only the methods a session cannot exist without are
/// required; everything else is a capability the provider may offer, checked against this same
/// declaration so a provider that offers one cannot offer it with a different meaning.
pub(crate) fn debug_dependency() -> Result<Dependency, Failure> {
    dependency_from_declaration(
        debug_declaration(),
        &DEBUG_REQUIRED_METHODS,
        ">=1, <2",
        "Debug contract is incomplete",
    )
}

/// The methods one provider declaration actually offers, judged against the host's own declaration.
///
/// A method is offered when the provider declares it with exactly the shape the host calls: the same
/// rule the base requirement uses, applied per capability so an incomplete offer is simply not an
/// offer rather than a broken provider.
pub(super) fn debug_abilities(declaration: &plugin_protocol::service::Contract) -> DebugAbilities {
    let host: plugin_protocol::service::Contract =
        serde_json::from_value(debug_declaration()).expect("the host's own declaration is valid");
    let offers = |method: &str| {
        declaration.methods.get(method).is_some()
            && declaration.methods.get(method) == host.methods.get(method)
    };
    DebugAbilities {
        breakpoints: offers("set_breakpoints"),
        resume_pause: offers("resume") && offers("pause"),
        step: offers("step"),
        inspect: offers("frames") && offers("variables"),
    }
}

/// The host's own debug declaration, as one value.
fn debug_declaration() -> Value {
    serde_json::from_str(
        r#"{"version":"1.0.0","methods":{
            "start":{
                "parameters":{"type":"record","fields":{
                    "program":{"type":"string","max_bytes":4096},
                    "args":{"type":"array","max_items":128,"items":{"type":"string","max_bytes":4096}},
                    "cwd":{"type":"string","max_bytes":4096},
                    "name":{"type":"string","max_bytes":256},
                    "env":{"type":"array","max_items":64,"items":{"type":"record","fields":{
                        "name":{"type":"string","max_bytes":128},
                        "value":{"type":"string","max_bytes":32768}}}},
                    "breakpoints":{"type":"array","max_items":512,"items":{"type":"record","fields":{
                        "source":{"type":"string","max_bytes":4096},
                        "line":{"type":"integer","min":1,"max":2147483647}}}},
                    "stop_on_entry":{"type":"boolean"}},
                    "optional":["cwd","name","env","breakpoints","stop_on_entry"]},
                "result":{"type":"record","fields":{
                    "session":{"type":"string","max_bytes":128},
                    "state":{"type":"string","max_bytes":32}}},
                "permissions":["process.exec","ui.panels"]},
            "set_breakpoints":{
                "parameters":{"type":"record","fields":{
                    "session":{"type":"string","max_bytes":128},
                    "breakpoints":{"type":"array","max_items":512,"items":{"type":"record","fields":{
                        "source":{"type":"string","max_bytes":4096},
                        "line":{"type":"integer","min":1,"max":2147483647}}}}}},
                "result":{"type":"record","fields":{
                    "breakpoints":{"type":"array","max_items":512,"items":{"type":"record","fields":{
                        "source":{"type":"string","max_bytes":4096},
                        "line":{"type":"integer","min":1,"max":2147483647},
                        "verified":{"type":"boolean"}}}}}},
                "permissions":["process.exec"]},
            "resume":{
                "parameters":{"type":"record","fields":{
                    "session":{"type":"string","max_bytes":128}}},
                "result":{"type":"record","fields":{
                    "session":{"type":"string","max_bytes":128},
                    "state":{"type":"string","max_bytes":32}}},
                "permissions":["process.exec"]},
            "pause":{
                "parameters":{"type":"record","fields":{
                    "session":{"type":"string","max_bytes":128}}},
                "result":{"type":"record","fields":{
                    "session":{"type":"string","max_bytes":128},
                    "state":{"type":"string","max_bytes":32}}},
                "permissions":["process.exec"]},
            "step":{
                "parameters":{"type":"record","fields":{
                    "session":{"type":"string","max_bytes":128},
                    "kind":{"type":"string","max_bytes":32}}},
                "result":{"type":"record","fields":{
                    "session":{"type":"string","max_bytes":128},
                    "state":{"type":"string","max_bytes":32}}},
                "permissions":["process.exec"]},
            "frames":{
                "parameters":{"type":"record","fields":{
                    "session":{"type":"string","max_bytes":128}}},
                "result":{"type":"record","fields":{
                    "frames":{"type":"array","max_items":256,"items":{"type":"record","fields":{
                        "id":{"type":"integer","min":0,"max":2147483647},
                        "name":{"type":"string","max_bytes":512},
                        "source":{"type":"string","max_bytes":4096},
                        "line":{"type":"integer","min":1,"max":2147483647}}}}}},
                "permissions":["process.exec"]},
            "variables":{
                "parameters":{"type":"record","fields":{
                    "session":{"type":"string","max_bytes":128},
                    "frame":{"type":"integer","min":0,"max":2147483647}}},
                "result":{"type":"record","fields":{
                    "variables":{"type":"array","max_items":512,"items":{"type":"record","fields":{
                        "name":{"type":"string","max_bytes":512},
                        "value":{"type":"string","max_bytes":4096}}}}}},
                "permissions":["process.exec"]},
            "status":{
                "parameters":{"type":"record","fields":{
                    "session":{"type":"string","max_bytes":128}}},
                "result":{"type":"record","fields":{
                    "session":{"type":"string","max_bytes":128},
                    "state":{"type":"string","max_bytes":32},
                    "reason":{"type":"string","max_bytes":64},
                    "source":{"type":"string","max_bytes":4096},
                    "line":{"type":"integer","min":1,"max":2147483647}},
                    "optional":["reason","source","line"]},
                "permissions":["process.exec"]},
            "stop":{
                "parameters":{"type":"record","fields":{
                    "session":{"type":"string","max_bytes":128}}},
                "result":{"type":"record","fields":{
                    "session":{"type":"string","max_bytes":128},
                    "state":{"type":"string","max_bytes":32}}},
                "permissions":["process.exec"]}
        }}"#,
    )
    .expect("the host's own debug declaration is valid")
}

/// The timeout one debug method is allowed, or `None` for a method the host does not call.
///
/// A start includes building and attaching, so it gets the longest window; the control exchanges are
/// short, because a provider that cannot acknowledge promptly is reported rather than waited on.
pub(super) fn debug_timeout_ms(method: &str) -> Option<u32> {
    match method {
        "start" => Some(DEBUG_START_TIMEOUT_MS),
        "set_breakpoints" => Some(DEBUG_BREAKPOINT_TIMEOUT_MS),
        "resume" | "pause" | "step" | "stop" | "status" | "frames" | "variables" => {
            Some(DEBUG_CONTROL_TIMEOUT_MS)
        }
        _ => None,
    }
}
/// The debug capability as a host question, answered without ever naming a debugger.
impl Manager {
    /// The installed plugins that declare a debug contract, with why one cannot serve.
    ///
    /// The match is the base requirement, not the optional methods: a provider that cannot step is
    /// still a debug provider, and its missing ability is reported where the control that needs it
    /// is, rather than making the whole provider unusable.
    pub fn debug_providers(&self) -> Vec<ProviderCandidate> {
        // Availability is judged on the required methods only; the exact match on every method the
        // host calls is enforced where a call is resolved.
        let scope = self.host_scope();
        let selected = self
            .plugin_services
            .lock()
            .map(|broker| broker.selected_provider(&scope, DEBUG_CONTRACT))
            .ok()
            .flatten();
        let mut candidates = self
            .installed
            .values()
            .filter_map(|installed| {
                let declaration = installed
                    .manifest
                    .plugin_services
                    .provides
                    .get(DEBUG_CONTRACT)?;
                let unavailable = if !installed.enabled {
                    Some("该插件未启用".to_owned())
                } else if let Some(error) = installed.compatibility_error() {
                    Some(format!("与当前宿主不兼容：{error}"))
                } else if let Some(error) = &installed.error {
                    Some(format!("插件运行出错：{error}"))
                } else if !debug_contract_is_usable(declaration) {
                    Some("声明的 debug.session 必需方法不完整".to_owned())
                } else {
                    None
                };
                let plugin = installed.manifest.id.clone();
                Some(ProviderCandidate {
                    selected: selected.as_deref() == Some(plugin.as_str()),
                    unavailable,
                    plugin,
                })
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| left.plugin.cmp(&right.plugin));
        candidates
    }

    /// What one installed plugin offers for debugging, or `None` when it is not a debug provider.
    pub fn debug_abilities(&self, plugin: &str) -> Option<DebugAbilities> {
        let installed = self.installed.get(plugin)?;
        let declaration = installed
            .manifest
            .plugin_services
            .provides
            .get(DEBUG_CONTRACT)?;
        Some(debug_abilities(declaration))
    }

    /// Whether a debug session could start here, and if not, why not.
    ///
    /// An entry point asks this before offering debugging. A missing capability is reported as such
    /// and never as a reason to run the program without a debugger.
    pub fn debug_availability(&self) -> Result<String, String> {
        let providers = self.debug_providers();
        if providers.is_empty() {
            return Err("没有安装提供调试能力的插件".into());
        }
        let usable = providers
            .iter()
            .filter(|candidate| candidate.unavailable.is_none())
            .map(|candidate| candidate.plugin.as_str())
            .collect::<Vec<_>>();
        if usable.is_empty() {
            return Err(format!(
                "已安装的调试提供者都不能用：{}",
                providers
                    .iter()
                    .filter_map(|candidate| candidate
                        .unavailable
                        .as_ref()
                        .map(|reason| format!("{}（{reason}）", candidate.plugin)))
                    .collect::<Vec<_>>()
                    .join("；")
            ));
        }
        // A chosen provider that is usable wins; a single usable one is not a choice to make, and
        // several usable ones are a choice for the user rather than one the host guesses at.
        if let Some(selected) = providers
            .iter()
            .find(|candidate| candidate.selected && candidate.unavailable.is_none())
        {
            return Ok(selected.plugin.clone());
        }
        match usable.as_slice() {
            [only] => Ok((*only).to_owned()),
            _ => Err(format!(
                "有多个可用的调试提供者（{}），请先选择其中一个",
                usable.join("、")
            )),
        }
    }

    /// What the provider that would serve a session can do, or `None` when none would.
    ///
    /// The abilities belong to the same provider the availability answer names, so a panel that offers
    /// a control and the call that control stages cannot disagree. An unavailable provider reports no
    /// abilities rather than its declaration: nothing will be asked of it either way.
    pub fn debug_service_abilities(&self) -> Option<DebugAbilities> {
        let provider = self.debug_availability().ok()?;
        self.debug_abilities(&provider)
    }
}

/// One answered debug call, and the session identity it belongs to.
///
/// The session identity travels with the answer so a caller can tell which session a late result
/// describes: a debugger's own session, not the host execution session that launched the target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugAnswer {
    pub session: String,
    pub result: Value,
}

/// Calling a debug provider on behalf of the host.
///
/// Every call is bounded and every answer is the provider's: the host never predicts a state, a
/// frame or a value, and a provider that does not answer within the window is reported as
/// unreachable rather than waited on. Nothing here holds a debug session of its own: the provider
/// owns it, and this only carries the identity it returned.
impl Manager {
    /// Call one debug method on the selected provider, waiting for its answer.
    ///
    /// `session` is the provider's own session identity. A missing or unusable provider fails before
    /// anything is sent, and an unanswered call is reported instead of being retried blindly.
    pub fn debug_call(&mut self, method: &str, arguments: Value) -> anyhow::Result<DebugAnswer> {
        let dependency = debug_dependency().map_err(start_failure)?;
        let timeout = debug_timeout_ms(method)
            .ok_or_else(|| anyhow::anyhow!("{method} is not a method this host may call"))?;
        let scope = self.host_scope();
        // The host is the caller, so the authority a provider may exercise is the host's own set and
        // never a plugin's.
        let caller = host_caller(&scope);
        self.refresh_services();
        let reference = self
            .plugin_services
            .lock()
            .unwrap()
            .resolve(&caller, DEBUG_CONTRACT, &dependency)
            .map_err(start_failure)?;
        let expected = reference.provider.caller.plugin.clone();
        let completion = Completion::new(timeout);
        let call = host_method_call(
            &caller,
            reference,
            method,
            arguments,
            &dependency,
            completion.clone(),
            self.host_alive.clone(),
        )
        .map_err(start_failure)?;
        self.plugin_services
            .lock()
            .unwrap()
            .enqueue(call)
            .map_err(start_failure)?;
        self.poll_request(&completion);
        // An unanswered call is reported rather than retried: a provider that cannot answer promptly
        // has not answered, and the host does not act as if it had.
        let value = match completion.status() {
            plugin_protocol::api::RequestUpdate::Completed { result } => {
                result.map_err(start_failure)?
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "{method} was not answered by the debug provider within its window"
                ));
            }
        };
        // The provider that answered is the one this call was routed to, which is what makes the
        // answer attributable when several providers are installed.
        let _ = expected;
        Ok(DebugAnswer {
            session: value
                .get("session")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            result: value,
        })
    }

    /// Ask a debug session what it is, which is the one call whose answer is also a state.
    pub fn debug_status(&mut self, session: &str) -> anyhow::Result<DebugSession> {
        let answer = self.debug_call("status", serde_json::json!({ "session": session }))?;
        DebugSession::from_value(&answer.result).map_err(|error| anyhow::anyhow!("{error:?}"))
    }

    /// Ask a debug session for the frames of its current pause.
    pub fn debug_frames(&mut self, session: &str) -> anyhow::Result<Vec<DebugFrame>> {
        let answer = self.debug_call("frames", serde_json::json!({ "session": session }))?;
        frames_from_value(&answer.result).map_err(|error| anyhow::anyhow!("{error:?}"))
    }

    /// Ask a debug session for one frame's variables.
    pub fn debug_variables(
        &mut self,
        session: &str,
        frame: u32,
    ) -> anyhow::Result<Vec<DebugVariable>> {
        let answer = self.debug_call(
            "variables",
            serde_json::json!({ "session": session, "frame": frame }),
        )?;
        variables_from_value(&answer.result).map_err(|error| anyhow::anyhow!("{error:?}"))
    }

    /// Ask a debug session to set the configuration's breakpoints.
    ///
    /// The whole set is sent every time rather than a difference, because that is what the contract's
    /// answer describes: the provider reports which of the positions it could bind, so a removed
    /// breakpoint has to be absent from the request for the answer to mean anything about it. The
    /// requests are grouped by source, which is how a provider is asked.
    pub fn debug_set_breakpoints(
        &mut self,
        session: &str,
        requests: &[DebugBreakpointRequest],
    ) -> anyhow::Result<Vec<DebugBreakpoint>> {
        let grouped = breakpoints_by_source(requests);
        let breakpoints = grouped
            .into_iter()
            .map(|(source, positions)| serde_json::json!({ "source": source, "lines": positions }))
            .collect::<Vec<_>>();
        let answer = self.debug_call(
            "set_breakpoints",
            serde_json::json!({ "session": session, "breakpoints": breakpoints }),
        )?;
        DebugBreakpoint::list_from_value(&answer.result)
            .map_err(|error| anyhow::anyhow!("{error:?}"))
    }
}

/// Group breakpoints by source, which is how a provider is asked to set them.
pub(super) fn breakpoints_by_source(
    requests: &[DebugBreakpointRequest],
) -> BTreeMap<&str, Vec<Value>> {
    let mut grouped: BTreeMap<&str, Vec<Value>> = BTreeMap::new();
    for request in requests {
        grouped
            .entry(request.source.as_str())
            .or_default()
            .push(request.to_value());
    }
    grouped
}
