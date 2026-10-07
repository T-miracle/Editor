//! Literal input, view restoration and output pulls address only the caller's managed session.
use super::*;
use api::{ErrorCode, Failure};
use plugin_protocol::service::Invocation;
use serde_json::{Value, json};
impl Terminal {
    /// The same source check protects reads, writes and presentation, including after a tab was hidden.
    pub(super) fn observation_call(&mut self, call: Invocation) -> Result<Value, Failure> {
        let id = call.arguments["session"]
            .as_str()
            .and_then(|text| text.parse::<u64>().ok())
            .ok_or_else(|| Failure::new(ErrorCode::InvalidRequest, "Expected session identity"))?;
        let index = self
            .tabs
            .iter()
            .position(|tab| {
                tab.id == id && tab.service_owner.as_deref() == Some(&call.caller.instance)
            })
            .ok_or_else(|| Failure::new(ErrorCode::InvalidHandle, "Unknown or foreign session"))?;
        match call.method.as_str() {
            "input" => {
                let bytes: Vec<u8> = serde_json::from_value(call.arguments["bytes"].clone())
                    .map_err(|error| Failure::new(ErrorCode::InvalidRequest, error.to_string()))?;
                let tab = &self.tabs[index];
                let handle = tab
                    .handle
                    .as_ref()
                    .filter(|_| !tab.exited)
                    .cloned()
                    .ok_or_else(|| Failure::new(ErrorCode::InvalidState, "Program ended"))?;
                host::call(api::Operation::Process {
                    operation: process::Operation::Write { handle, bytes },
                })?;
                Ok(json!({"session":id.to_string(),"state":"running"}))
            }
            "locate" => {
                // Panel visibility is a bounded async editor request; the provider selects its own tab.
                self.editor_request(
                    api::EditorOperation::SetPanelVisibility {
                        panel: "terminal".into(),
                        visible: true,
                    },
                    commands::PendingEditor::Effect,
                )
                .map_err(|message| Failure::new(ErrorCode::OperationFailed, message))?;
                self.tabs[index].hidden = false;
                self.active = index;
                Ok(
                    json!({"session":id.to_string(),"state":if self.tabs[index].exited {"exited"} else {"running"}}),
                )
            }
            "events" => {
                let after = call.arguments["after"]
                    .as_u64()
                    .ok_or_else(|| Failure::new(ErrorCode::InvalidRequest, "Expected cursor"))?;
                let limit = call.arguments["limit"].as_u64().ok_or_else(|| {
                    Failure::new(ErrorCode::InvalidRequest, "Expected batch limit")
                })? as usize;
                serde_json::to_value(self.tabs[index].observations.read(
                    id.to_string(),
                    after,
                    limit,
                )?)
                .map_err(|error| Failure::new(ErrorCode::OperationFailed, error.to_string()))
            }
            _ => Err(Failure::new(
                ErrorCode::UnsupportedOperation,
                "Unknown observation method",
            )),
        }
    }
}
