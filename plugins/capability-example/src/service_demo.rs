//! Independent service consumer/provider examples depend only on the public SDK and package assets.
use plugin_protocol::{
    api::{self, Failure},
    service::{self, guest},
};

#[derive(Default)]
pub(super) struct Client {
    reference: Option<api::ResourceHandle>,
    task: Option<guest::Task>,
    continuation: Option<api::guest::EditorTask>,
    /// The last answer this consumer received, kept so a caller can read what actually arrived.
    last: Option<String>,
    /// Provider-side invocation is separate from this instance's consumer request handles.
    deferred: Option<api::ResourceHandle>,
    /// Retaining one stale receipt lets the SDK example demonstrate host-side replay rejection.
    retired_reply: Option<api::ResourceHandle>,
}
impl Client {
    /// Defer one declared call until a later command or native notification produces its result.
    pub(super) fn defer(&mut self, call: service::Invocation) -> Result<(), Failure> {
        if self.deferred.is_some() {
            return Err(Failure::new(
                api::ErrorCode::LimitExceeded,
                "One deferred example is already pending",
            ));
        }
        self.deferred = call.reply;
        Ok(())
    }
    /// Async host results must keep the service's restricted source authority during this continuation.
    pub(super) fn editor_update(&mut self, event: &api::Notification) -> Option<String> {
        let update = self.continuation.as_mut()?.update(event)?;
        update.is_terminal().then(|| {
            format!(
                "Service continuation: {:?}",
                api::guest::request(api::Operation::OpenData)
            )
        })
    }

    pub(super) fn provide(
        &mut self,
        call: service::Invocation,
    ) -> Result<serde_json::Value, Failure> {
        if call.method == "reply-retained" {
            // This delegated attempt must not finish a different source's retained invocation.
            let request = self.deferred.as_ref().ok_or_else(|| {
                Failure::new(api::ErrorCode::InvalidHandle, "No deferred invocation")
            })?;
            return Ok(serde_json::json!(format!(
                "{:?}",
                guest::reply(request, Ok(call.arguments))
            )));
        }
        if call.method == "editor-continuation" || call.method == "editor-cancel" {
            let task = api::guest::EditorTask::start(api::EditorOperation::ActiveDirectory, 30000)?;
            if call.method == "editor-cancel" {
                return Ok(serde_json::json!(format!(
                    "{:?}",
                    task.cancel(api::CancelMode::TryTerminate)?
                )));
            }
            self.continuation = Some(task);
            return Ok(serde_json::json!("Editor continuation accepted"));
        }
        provide(call)
    }
    pub(super) fn command(
        &mut self,
        id: &str,
        args: Option<serde_json::Value>,
    ) -> Result<String, Failure> {
        let args = args.unwrap_or_default();
        match id {
            "service-forge-host-reference" => {
                // A hostile guest can manufacture public handle bytes, but cannot create the
                // host's private authority record or transfer an instance-owned service root.
                let original = guest::open("session.host")?;
                let mut forged = original.clone();
                forged.instance = format!("host@{}", forged.scope);
                let result = guest::Task::start(&forged, "list", serde_json::json!({}), 1000);
                api::guest::close_resource(original)?;
                Ok(match result {
                    Ok(_) => "unexpected forged authority".into(),
                    Err(error) => format!("{:?}: {}", error.code, error.message),
                })
            }
            "service-reply-deferred" => {
                let request = self.deferred.as_ref().ok_or_else(|| {
                    Failure::new(api::ErrorCode::InvalidHandle, "No deferred invocation")
                })?;
                api::guest::request(api::Operation::Service {
                    operation: service::Operation::Reply {
                        request: request.clone(),
                        result: Ok(args),
                    },
                })?;
                self.retired_reply = self.deferred.take();
                Ok("Replied".into())
            }
            "service-replay-reply" => {
                let request = self.retired_reply.as_ref().ok_or_else(|| {
                    Failure::new(api::ErrorCode::InvalidHandle, "No retired receipt")
                })?;
                guest::reply(request, Ok(args))?;
                Ok("Unexpected replay".into())
            }
            "service-open" => {
                if let Some(handle) = self.reference.take() {
                    api::guest::close_resource(handle)?;
                }
                self.reference = Some(guest::open(args.as_str().unwrap_or("example.echo"))?);
                Ok("Service opened".into())
            }
            "service-call" => {
                let reference = self.reference.as_ref().ok_or_else(|| {
                    Failure::new(api::ErrorCode::InvalidHandle, "Open the service first")
                })?;
                self.task = Some(guest::Task::start(
                    reference,
                    args["method"].as_str().unwrap_or("echo"),
                    args["value"].clone(),
                    args["timeout_ms"].as_u64().unwrap_or(30000) as u32,
                )?);
                Ok("Accepted".into())
            }
            // Call a contract by name rather than one fixed contract, so this consumer can exercise
            // any versioned service the host offers — including the host's own session contract —
            // through the same public path a real plugin would use.
            "service-call-contract" => {
                if let Some(handle) = self.reference.take() {
                    api::guest::close_resource(handle)?;
                }
                let contract = args["contract"].as_str().ok_or_else(|| {
                    Failure::new(api::ErrorCode::InvalidRequest, "A contract is required")
                })?;
                let reference = guest::open(contract)?;
                self.reference = Some(reference.clone());
                self.task = Some(guest::Task::start(
                    &reference,
                    args["method"].as_str().unwrap_or("list"),
                    args["value"].clone(),
                    args["timeout_ms"].as_u64().unwrap_or(30000) as u32,
                )?);
                Ok("Accepted".into())
            }
            "service-cancel" => Ok(format!(
                "{:?}",
                self.task
                    .as_ref()
                    .ok_or_else(|| Failure::new(api::ErrorCode::InvalidState, "No service call"))?
                    .cancel(api::CancelMode::TryTerminate)?
            )),
            // A command's answer is what the host shows, so a check can read what this consumer
            // received without reaching into guest state.
            "service-last-answer" => Ok(self.last.clone().unwrap_or_default()),
            _ => Ok(String::new()),
        }
    }
    pub(super) fn update(&mut self, event: &service::Notification) -> Option<String> {
        if let service::Notification::InvocationCancelled { request, .. } = event {
            if self.deferred.as_ref() == Some(request) {
                self.retired_reply = self.deferred.take();
            }
            return None;
        }
        let text = self
            .task
            .as_mut()?
            .update(event)
            .map(|update| serde_json::to_string(&update).unwrap())?;
        self.last = Some(text.clone());
        Some(text)
    }

    /// The last answer this consumer received, or `None` while it is still waiting for one.
    pub(super) fn last_answer(&self) -> Option<&str> {
        self.last.as_deref()
    }
}

/// The provider's identity and formatting come from package data, never a host-side branch.
pub(super) fn provide(call: service::Invocation) -> Result<serde_json::Value, Failure> {
    let label = String::from_utf8(api::guest::read_asset("service-label.txt")?).unwrap();
    match call.method.as_str() {
        "echo" => Ok(serde_json::json!(format!(
            "{label}:{}",
            call.arguments.as_str().unwrap_or_default()
        ))),
        "probe" => {
            let operation = serde_json::from_str(call.arguments.as_str().unwrap_or_default())
                .map_err(|error| Failure::new(api::ErrorCode::InvalidRequest, error.to_string()))?;
            Ok(serde_json::json!(
                serde_json::to_string(&api::guest::request(operation)).unwrap()
            ))
        }
        "source" => Ok(serde_json::json!(format!(
            "{}|{}|{:?}",
            call.caller.plugin, call.caller.scope, call.caller.permissions
        ))),
        "cycle" => {
            let reference = guest::open("example.echo")?;
            let result = guest::Task::start(&reference, "echo", serde_json::json!("nested"), 1000);
            api::guest::close_resource(reference)?;
            Ok(serde_json::json!(match result {
                Ok(_) => "unexpected success".into(),
                Err(error) => format!("{error}"),
            }))
        }
        "read-close" => {
            // More iterations than the host handle quota demonstrate explicit ownership cleanup.
            for _ in 0..160 {
                let handle = api::guest::open_workspace()?;
                api::guest::close_resource(handle)?;
                let reference = guest::open("example.echo")?;
                api::guest::close_resource(reference)?;
            }
            Ok(serde_json::json!("Resources released"))
        }
        "bad-result" => Ok(serde_json::json!(42)),
        "trap" => {
            // The real SDK fixture exposes both WASI stdout and panic stderr before host fault retirement.
            println!("service fixture stdout before trap");
            panic!("service fixture trap")
        }
        _ => Err(Failure::new(
            api::ErrorCode::UnsupportedOperation,
            "Unknown fixture method",
        )),
    }
}
