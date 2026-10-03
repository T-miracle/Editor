//! Versioned execution service uses the caller's argv and delegated authority, never a private shell profile.
use super::*;
use api::{ErrorCode, Failure};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Execution {
    program: String,
    args: Vec<String>,
    cwd: Option<String>,
    name: Option<String>,
}

impl Terminal {
    /// Completion acknowledges process creation; it does not claim exit, rollback, or completed panel display.
    pub(super) fn execute_service(
        &mut self,
        call: plugin_protocol::service::Invocation,
    ) -> Result<serde_json::Value, Failure> {
        if call.contract != "interactive.execute" || call.method != "execute" {
            return Err(Failure::new(
                ErrorCode::UnsupportedOperation,
                "Unknown execution method",
            ));
        }
        let request: Execution = serde_json::from_value(call.arguments)
            .map_err(|error| Failure::new(ErrorCode::InvalidRequest, error.to_string()))?;
        if !self.settings.enabled {
            return Err(Failure::new(
                ErrorCode::InvalidState,
                "Terminal sessions are disabled",
            ));
        }
        if self.tabs.len() >= 32 {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Terminal session limit reached",
            ));
        }
        // Visibility is an ordinary async editor operation and retains this service's source context.
        let api::Value::Accepted(visibility) = host::call(api::Operation::Editor {
            operation: api::EditorOperation::SetPanelVisibility {
                panel: "terminal".into(),
                visible: true,
            },
            timeout_ms: 30_000,
        })?
        else {
            return Err(Failure::new(
                ErrorCode::OperationFailed,
                "Expected panel request",
            ));
        };
        let extent = self.extent();
        let started = host::call(api::Operation::Process {
            operation: process::Operation::Execute {
                program: request.program.clone(),
                args: request.args.clone(),
                cwd: request.cwd.clone(),
                transport: process::Transport::Pty {
                    columns: extent.columns as u16,
                    rows: extent.rows as u16,
                },
            },
        });
        let handle = match started {
            Ok(api::Value::Resource(handle)) => handle,
            result => {
                // A rejected process must not leave an unrelated request to open an empty panel.
                let _ = host::call(api::Operation::CloseResource { handle: visibility });
                return Err(result.err().unwrap_or_else(|| {
                    Failure::new(ErrorCode::OperationFailed, "Expected process resource")
                }));
            }
        };
        let name = request
            .name
            .unwrap_or_else(|| request.program.clone())
            .chars()
            .filter(|ch| !ch.is_control())
            .take(80)
            .collect::<String>();
        let id = self.next_id;
        self.next_id += 1;
        self.restore_tab(SavedTab {
            exited: false,
            id,
            name: if name.trim().is_empty() {
                "执行".into()
            } else {
                name
            },
            profile: Profile {
                name: "Service execution".into(),
                program: request.program,
                args: request.args,
            },
            cwd: request.cwd.unwrap_or_else(|| self.env.workspace.clone()),
            output: String::new(),
            display: None,
        });
        self.active = self.tabs.len() - 1;
        let tab = &mut self.tabs[self.active];
        tab.handle = Some(handle);
        tab.resumable = false;
        self.pending_editor
            .insert(visibility.resource, commands::PendingEditor::Effect);
        self.error = None;
        self.menu = None;
        Ok(serde_json::json!({"session":id.to_string(), "state":"started"}))
    }
}
