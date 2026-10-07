//! Independent execution provider owns stdio programs and may acknowledge creation asynchronously.
use plugin_protocol::{
    api::{self, ErrorCode, Failure, ResourceHandle},
    process, service,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Caller data is literal argv and environment; no terminal profile is applied.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Execution {
    program: String,
    args: Vec<String>,
    cwd: Option<String>,
    name: Option<String>,
    #[serde(default)]
    env: Vec<EnvEntry>,
}
/// Environment names and values remain separate throughout process creation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvEntry {
    name: String,
    value: String,
}

/// Retained identity and observations survive the creation receipt.
struct Program {
    owner: String,
    handle: ResourceHandle,
    result: Option<process::Update>,
    observations: plugin_protocol::execution::EventBuffer,
}

/// At most 32 programs and one delayed receipt are retained for SDK demonstrations.
#[derive(Default)]
pub(super) struct Provider {
    programs: BTreeMap<String, Program>,
    defer_next: bool,
    deferred: Option<(ResourceHandle, Value)>,
}
impl Provider {
    /// A delayed receipt demonstrates that cancelling a wait cannot undo a native effect.
    pub fn command(&mut self, id: &str) -> Result<String, Failure> {
        match id {
            "execution-defer-next" => {
                self.defer_next = true;
                Ok("Next receipt deferred".into())
            }
            "execution-release" => {
                let (request, result) = self
                    .deferred
                    .clone()
                    .ok_or_else(|| Failure::new(ErrorCode::InvalidHandle, "No retained receipt"))?;
                service::guest::reply(&request, Ok(result))?;
                self.deferred = None;
                Ok("Receipt released".into())
            }
            _ => Err(Failure::new(
                ErrorCode::UnsupportedOperation,
                "Unknown execution command",
            )),
        }
    }
    /// Controls are source-bound, even if another consumer guesses a provider session ID.
    pub fn call(&mut self, call: service::Invocation) -> Result<Option<Value>, Failure> {
        if call.method == "execute" {
            return self.execute(call);
        }
        let session = call
            .arguments
            .get("session")
            .and_then(Value::as_str)
            .ok_or_else(|| Failure::new(ErrorCode::InvalidRequest, "Expected session"))?;
        let program = self
            .programs
            .get(session)
            .filter(|program| program.owner == call.caller.instance)
            .ok_or_else(|| {
                Failure::new(
                    ErrorCode::InvalidHandle,
                    "Session belongs to another source or ended",
                )
            })?;
        match call.method.as_str() {
            "status" => {
                let mut answer = json!({"session":session,"state":"running"});
                match &program.result {
                    Some(process::Update::Exited { code }) => {
                        answer["state"] = json!("exited");
                        answer["code"] = json!(code);
                    }
                    Some(process::Update::Terminated) => answer["state"] = json!("terminated"),
                    _ => {}
                }
                Ok(Some(answer))
            }
            "stop" => {
                if program.result.is_some() {
                    return Ok(Some(json!({"session":session,"state":"exited"})));
                }
                let mode = call
                    .arguments
                    .get("mode")
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(|error| Failure::new(ErrorCode::InvalidRequest, error.to_string()))?
                    .unwrap_or_default();
                api::guest::request(api::Operation::Process {
                    operation: process::Operation::RequestExit {
                        handle: program.handle.clone(),
                        mode,
                    },
                })?;
                Ok(Some(json!({"session":session,"state":"stopping"})))
            }
            "input" => {
                if program.result.is_some() {
                    return Err(Failure::new(ErrorCode::InvalidState, "Program ended"));
                }
                let bytes: Vec<u8> = serde_json::from_value(call.arguments["bytes"].clone())
                    .map_err(|error| Failure::new(ErrorCode::InvalidRequest, error.to_string()))?;
                api::guest::request(api::Operation::Process {
                    operation: process::Operation::Write {
                        handle: program.handle.clone(),
                        bytes,
                    },
                })?;
                Ok(Some(json!({"session":session,"state":"running"})))
            }
            "locate" => {
                api::guest::EditorTask::start(
                    api::EditorOperation::SetPanelVisibility {
                        panel: "welcome".into(),
                        visible: true,
                    },
                    30000,
                )?;
                Ok(Some(
                    json!({"session":session,"state":if program.result.is_some() {"exited"} else {"running"}}),
                ))
            }
            "events" => {
                let after = call.arguments["after"]
                    .as_u64()
                    .ok_or_else(|| Failure::new(ErrorCode::InvalidRequest, "Expected cursor"))?;
                let limit = call.arguments["limit"].as_u64().ok_or_else(|| {
                    Failure::new(ErrorCode::InvalidRequest, "Expected batch limit")
                })? as usize;
                serde_json::to_value(program.observations.read(session.into(), after, limit)?)
                    .map(Some)
                    .map_err(|error| Failure::new(ErrorCode::OperationFailed, error.to_string()))
            }
            _ => Err(Failure::new(
                ErrorCode::UnsupportedOperation,
                "Unknown execution method",
            )),
        }
    }
    /// Panel admission is asynchronous; the receipt identifies the actual created process.
    fn execute(&mut self, call: service::Invocation) -> Result<Option<Value>, Failure> {
        // Completed receipts retain bounded observations. Evict only the oldest finished program;
        // an active program remains addressable even when the history budget is exhausted.
        while self.programs.len() >= 32 {
            let finished = self
                .programs
                .iter()
                .filter(|(_, program)| program.result.is_some())
                .min_by_key(|(_, program)| program.handle.resource)
                .map(|(id, _)| id.clone());
            let Some(id) = finished else {
                break;
            };
            self.programs.remove(&id);
        }
        if self.programs.len() >= 32 {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Execution quota exceeded",
            ));
        }
        let args: Execution = serde_json::from_value(call.arguments)
            .map_err(|error| Failure::new(ErrorCode::InvalidRequest, error.to_string()))?;
        let panel = api::guest::EditorTask::start(
            api::EditorOperation::SetPanelVisibility {
                panel: "welcome".into(),
                visible: true,
            },
            30_000,
        )?;
        let result = api::guest::request(api::Operation::Process {
            operation: process::Operation::Execute {
                program: args.program,
                args: args.args,
                cwd: args.cwd,
                transport: process::Transport::Stdio,
                env: args
                    .env
                    .into_iter()
                    .map(|entry| (entry.name, entry.value))
                    .collect(),
            },
        });
        let handle = match result {
            Ok(api::Value::Resource(handle)) => handle,
            result => {
                let _ = panel.cancel(api::CancelMode::TryTerminate);
                return Err(result.err().unwrap_or_else(|| {
                    Failure::new(ErrorCode::OperationFailed, "Expected process")
                }));
            }
        };
        // Labels cannot change authority or provider routing.
        let _label = args.name;
        let session = handle.resource.to_string();
        let mut observations = plugin_protocol::execution::EventBuffer::default();
        observations.state("running", None);
        self.programs.insert(
            session.clone(),
            Program {
                owner: call.caller.instance,
                handle,
                result: None,
                observations,
            },
        );
        let answer = json!({"session":session,"state":"started"});
        if self.defer_next {
            let request = call.reply.ok_or_else(|| {
                Failure::new(
                    ErrorCode::CapabilityUnavailable,
                    "Deferred replies unavailable",
                )
            })?;
            self.defer_next = false;
            self.deferred = Some((request, answer));
            Ok(None)
        } else {
            Ok(Some(answer))
        }
    }
    /// Exit observations preserve the full u32 code and do not confuse creation with completion.
    pub fn observe(&mut self, handle: &ResourceHandle, update: &process::Update) {
        if let Some(program) = self
            .programs
            .values_mut()
            .find(|program| &program.handle == handle)
        {
            program.observations.observe(update);
            if !matches!(update, process::Update::Output { .. }) {
                program.result = Some(update.clone());
            }
        }
    }
}
