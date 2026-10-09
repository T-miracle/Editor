//! Native execution is an ordinary, revocable provider of the public interactive contract.
//! The runtime owns system resources; native terminal views own VT interpretation and tab policy.

use super::*;
use crate::{
    native_processes::{NativeLaunch, NativeProcessEvent, NativeProcessGroup},
    plugin_services::{Call, Context, Provider},
};
use host_services::{execution_dependency, host_caller};
use plugin_protocol::{
    api::{ErrorCode, Failure},
    execution::EventBuffer,
    process::{Transport, Update},
    service::Contract,
};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;

/// Bounded native publications retain the exact provider session and literal launch, not a UI model.
#[derive(Clone, Debug)]
pub struct NativeExecutionUpdate {
    /// Provider-local handles are meaningful only together with their creating incarnation.
    pub provider_instance: String,
    pub session: String,
    pub request: RunRequest,
    /// A location request or accepted creation opens/selects the owning native view.
    pub locate: bool,
    pub updates: Vec<Update>,
    pub failure: Option<String>,
}

struct Entry {
    provider_instance: String,
    origin: Context,
    request: RunRequest,
    pending: Option<Call>,
    state: &'static str,
    code: Option<u32>,
    observations: EventBuffer,
    /// Stop/retirement remains pending when the bounded supervisor queue is temporarily full.
    retiring: bool,
    cleanup_sent: bool,
}

pub(super) struct Executions {
    group: NativeProcessGroup,
    entries: BTreeMap<u64, Entry>,
    next: u64,
    publications: Vec<NativeExecutionUpdate>,
}

/// A separate participant avoids a session.host → execution self-cycle while retaining one scope.
pub(super) fn provider(
    scope: &str,
    alive: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Provider {
    let dependency = execution_dependency().expect("native execution declaration is valid");
    let mut caller = host_caller(scope);
    caller.plugin = "nanobug.execution".into();
    caller.instance = format!("native-execution@{scope}");
    let mut methods = dependency.methods;
    methods.insert("resize".into(), plugin_protocol::execution::resize_method());
    methods.insert(
        "execute_terminal".into(),
        plugin_protocol::execution::terminal_execute_method(),
    );
    Provider {
        caller,
        alive,
        contracts: [(
            host_services::EXECUTION_CONTRACT.into(),
            Contract {
                version: "2.2.0".parse().unwrap(),
                methods,
            },
        )]
        .into_iter()
        .collect(),
    }
}

impl Executions {
    pub(super) fn new(trusted: bool) -> Self {
        Self {
            group: NativeProcessGroup::new(trusted),
            entries: BTreeMap::new(),
            next: 1,
            publications: vec![],
        }
    }

    /// Trust revocation is synchronous; queued native launches retain a generation checked again at spawn.
    pub(super) fn set_trusted(&self, trusted: bool) {
        let _ = self.group.set_trusted(trusted);
    }

    /// Native calls still pass broker negotiation, shrinking permission and incarnation validation.
    pub(super) fn answer(
        &mut self,
        call: &Call,
        workspace: &str,
        trusted: bool,
    ) -> Result<Option<Value>, Failure> {
        if !trusted
            || !call
                .context
                .lifetimes
                .iter()
                .all(|alive| alive.load(Ordering::Acquire))
        {
            return Err(Failure::new(
                ErrorCode::PermissionDenied,
                "Execution source or workspace was revoked",
            ));
        }
        if matches!(call.method.as_str(), "execute" | "execute_terminal") {
            let mut arguments = call.arguments.clone();
            let inherit_cursor = arguments
                .as_object_mut()
                .unwrap()
                .remove("inherit_cursor")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            let request: RunRequest = serde_json::from_value(arguments).map_err(invalid)?;
            if self.entries.len() >= 64 {
                let finished = self
                    .entries
                    .iter()
                    .find(|(_, entry)| matches!(entry.state, "exited" | "terminated" | "failed"))
                    .map(|(id, _)| *id);
                if let Some(id) = finished {
                    self.entries.remove(&id);
                }
            }
            if self.entries.len() >= 64 {
                return Err(Failure::new(
                    ErrorCode::LimitExceeded,
                    "Native execution capacity reached",
                ));
            }
            let id = self.next;
            self.next = id.checked_add(1).ok_or_else(|| {
                Failure::new(ErrorCode::LimitExceeded, "Execution identity exhausted")
            })?;
            let launch = NativeLaunch {
                program: request.program.clone(),
                args: request.args.clone(),
                cwd: request
                    .cwd
                    .clone()
                    .filter(|path| !path.is_empty())
                    .unwrap_or_else(|| workspace.into()),
                env: request
                    .env
                    .iter()
                    .map(|entry| (entry.name.clone(), entry.value.clone()))
                    .collect(),
                transport: Transport::Pty {
                    columns: 80,
                    rows: 24,
                    inherit_cursor,
                },
            };
            self.group.launch(id, launch).map_err(native_failure)?;
            self.entries.insert(
                id,
                Entry {
                    provider_instance: call.reference.provider.caller.instance.clone(),
                    origin: call.context.clone(),
                    request,
                    pending: Some(call.clone()),
                    state: "starting",
                    code: None,
                    observations: EventBuffer::default(),
                    retiring: false,
                    cleanup_sent: false,
                },
            );
            // Completion is deferred until real creation, so failure cannot masquerade as a running child.
            return Ok(None);
        }
        let id = call.arguments["session"]
            .as_str()
            .and_then(|id| id.parse::<u64>().ok())
            .ok_or_else(|| {
                Failure::new(ErrorCode::InvalidRequest, "Expected execution identity")
            })?;
        let entry = self
            .entries
            .get_mut(&id)
            .filter(|entry| {
                entry.origin.caller.instance == call.context.caller.instance
                    && entry.origin.caller.scope == call.context.caller.scope
                    && entry
                        .origin
                        .lifetimes
                        .iter()
                        .all(|alive| alive.load(Ordering::Acquire))
            })
            .ok_or_else(|| {
                Failure::new(ErrorCode::InvalidHandle, "Unknown or foreign execution")
            })?;
        match call.method.as_str() {
            "status" => {
                let mut value = json!({"session":id.to_string(), "state":entry.state});
                if let Some(code) = entry.code {
                    value["code"] = json!(code);
                }
                Ok(Some(value))
            }
            "stop" => {
                if matches!(
                    entry.state,
                    "starting" | "running" | "stopping" | "terminating"
                ) {
                    let force = call.arguments["mode"].as_str() == Some("force");
                    self.group.stop(id, force).map_err(native_failure)?;
                    entry.state = if force { "terminating" } else { "stopping" };
                }
                Ok(Some(json!({"session":id.to_string(), "state":entry.state})))
            }
            "input" => {
                if !matches!(entry.state, "running" | "stopping" | "terminating") {
                    return Err(Failure::new(
                        ErrorCode::InvalidState,
                        "Program is not running",
                    ));
                }
                let bytes: Vec<u8> =
                    serde_json::from_value(call.arguments["bytes"].clone()).map_err(invalid)?;
                self.group.write(id, bytes).map_err(native_failure)?;
                Ok(Some(json!({"session":id.to_string(), "state":entry.state})))
            }
            "resize" => {
                self.group
                    .resize(
                        id,
                        call.arguments["columns"].as_u64().unwrap_or(0) as u16,
                        call.arguments["rows"].as_u64().unwrap_or(0) as u16,
                    )
                    .map_err(native_failure)?;
                Ok(Some(json!({"session":id.to_string(), "state":entry.state})))
            }
            "events" => {
                let batch = entry.observations.read(
                    id.to_string(),
                    call.arguments["after"].as_u64().unwrap_or(u64::MAX),
                    call.arguments["limit"].as_u64().unwrap_or(0) as usize,
                )?;
                Ok(Some(serde_json::to_value(batch).map_err(invalid)?))
            }
            "locate" => {
                self.publications.push(NativeExecutionUpdate {
                    provider_instance: entry.provider_instance.clone(),
                    session: id.to_string(),
                    request: entry.request.clone(),
                    locate: true,
                    updates: vec![],
                    failure: None,
                });
                Ok(Some(json!({"session":id.to_string(), "state":entry.state})))
            }
            _ => Err(Failure::new(
                ErrorCode::UnsupportedOperation,
                "Unknown native execution method",
            )),
        }
    }

    /// Output backpressure never delays revocation; source lifetime retirement always schedules tree cleanup.
    pub(super) fn poll(&mut self, broker: &crate::plugin_services::Shared) {
        for (id, entry) in &mut self.entries {
            if !entry
                .origin
                .lifetimes
                .iter()
                .all(|alive| alive.load(Ordering::Acquire))
                || entry
                    .pending
                    .as_ref()
                    .is_some_and(|call| call.completion.status().is_terminal())
            {
                entry.retiring = !entry.cleanup_sent
                    && matches!(
                        entry.state,
                        "starting" | "running" | "stopping" | "terminating"
                    );
            }
            if entry.retiring && self.group.close(*id).is_ok() {
                entry.retiring = false;
                entry.cleanup_sent = true;
                entry.state = "terminating";
                // Admission is not exit. The supervisor emits Terminated after joining cleanup.
            }
        }
        if self.publications.len() >= 512 {
            return;
        }
        for event in self.group.poll() {
            let id = match &event {
                NativeProcessEvent::Started { session }
                | NativeProcessEvent::Update { session, .. }
                | NativeProcessEvent::Failed { session, .. } => *session,
            };
            let Some(entry) = self.entries.get_mut(&id) else {
                continue;
            };
            let mut publication = NativeExecutionUpdate {
                provider_instance: entry.provider_instance.clone(),
                session: id.to_string(),
                request: entry.request.clone(),
                locate: false,
                updates: vec![],
                failure: None,
            };
            match event {
                NativeProcessEvent::Started { .. } => {
                    if !entry.cleanup_sent {
                        entry.state = "running";
                    }
                    if let Some(call) = entry.pending.take() {
                        let answer = json!({"session":id.to_string(),"state":"running"});
                        if broker
                            .lock()
                            .unwrap()
                            .complete_deferred(call, Ok(answer))
                            .is_err()
                        {
                            entry.retiring = true;
                        } else {
                            publication.locate = true;
                        }
                    }
                }
                NativeProcessEvent::Update { update, .. } => {
                    entry.observations.observe(&update);
                    match update {
                        Update::Exited { code } => {
                            entry.state = "exited";
                            entry.code = Some(code);
                        }
                        Update::Terminated => entry.state = "terminated",
                        _ => {}
                    }
                    publication.updates.push(update);
                }
                NativeProcessEvent::Failed {
                    message, launch, ..
                } => {
                    if launch {
                        entry.state = "failed";
                        if let Some(call) = entry.pending.take() {
                            let _ = broker.lock().unwrap().complete_deferred(
                                call,
                                Err(Failure::new(ErrorCode::OperationFailed, message.clone())),
                            );
                        }
                    }
                    publication.failure = Some(message);
                }
            }
            if publication.locate
                || !publication.updates.is_empty()
                || publication.failure.is_some()
            {
                self.publications.push(publication);
            }
        }
    }

    /// A confirmed workspace/window close waits for the same supervisor cleanup acknowledgement.
    pub(super) fn shutdown(&mut self) {
        if let Ok(acknowledge) = self.group.shutdown() {
            if acknowledge
                .recv_timeout(std::time::Duration::from_secs(10))
                .is_ok()
            {
                // Only the native cleanup acknowledgement releases shutdown diagnostics.
                for entry in self.entries.values_mut() {
                    entry.state = "terminated";
                }
            }
        }
    }

    /// Ended observations remain readable but do not count as live native process resources.
    pub(super) fn resource_count(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| {
                matches!(
                    entry.state,
                    "starting" | "running" | "stopping" | "terminating"
                )
            })
            .count()
    }
}

fn invalid(error: serde_json::Error) -> Failure {
    Failure::new(ErrorCode::InvalidRequest, error.to_string())
}
fn native_failure(error: anyhow::Error) -> Failure {
    Failure::new(ErrorCode::OperationFailed, format!("{error:#}"))
}

impl Manager {
    /// Literal stdin reaches only the original execution's pinned provider; returns an async receipt.
    pub fn input_execution(
        &mut self,
        session: u64,
        bytes: Vec<u8>,
    ) -> anyhow::Result<crate::Completion<Value>> {
        self.execution_operation(session, "input", json!({"bytes":bytes}), 5000, None)
            .map_err(host_services::start_failure)
    }
    /// Negotiate optional 2.1 geometry against that execution's provider without retargeting resources.
    pub fn resize_execution(
        &mut self,
        session: u64,
        columns: u16,
        rows: u16,
    ) -> anyhow::Result<crate::Completion<Value>> {
        self.execution_operation(
            session,
            "resize",
            json!({"columns":columns,"rows":rows}),
            5000,
            None,
        )
        .map_err(host_services::start_failure)
    }
    /// Native presentation consumes bounded ordered events; this neither grants permissions nor launches children.
    pub fn take_native_execution_updates(&mut self) -> Vec<NativeExecutionUpdate> {
        std::mem::take(&mut self.native_executions.publications)
    }
}
