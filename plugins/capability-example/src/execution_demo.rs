//! Independent execution provider fixture uses stdio instead of a terminal-owned PTY or private profile.
use plugin_protocol::{
    api::{self, ErrorCode, Failure},
    process, service,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Execution {
    program: String,
    args: Vec<String>,
    cwd: Option<String>,
    /// Accepted as portable presentation metadata, with no authority or executable meaning.
    #[serde(rename = "name")]
    _name: Option<String>,
}

/// Return the same creation receipt as any 1.0 provider, without waiting for process exit.
pub(super) fn execute(call: service::Invocation) -> Result<serde_json::Value, Failure> {
    if call.method != "execute" {
        return Err(Failure::new(
            ErrorCode::UnsupportedOperation,
            "Unknown execution method",
        ));
    }
    let args: Execution = serde_json::from_value(call.arguments)
        .map_err(|error| Failure::new(ErrorCode::InvalidRequest, error.to_string()))?;
    let task = api::guest::EditorTask::start(
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
        },
    });
    match result {
        Ok(api::Value::Resource(handle)) => {
            Ok(serde_json::json!({"session":handle.resource.to_string(), "state":"started"}))
        }
        result => {
            // Process rejection also cancels the still-pending presentation request.
            let _ = task.cancel(api::CancelMode::TryTerminate);
            Err(result.err().unwrap_or_else(|| {
                Failure::new(ErrorCode::OperationFailed, "Expected process handle")
            }))
        }
    }
}
