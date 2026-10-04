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
}
impl Client {
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
