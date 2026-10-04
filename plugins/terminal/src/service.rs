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

/// Execution contract 1.1 names the session whose program should stop.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Stop {
    /// Session identity this provider returned when the program was started.
    session: String,
}

impl Terminal {
    /// Completion acknowledges process creation; it does not claim exit, rollback, or completed panel display.
    pub(super) fn execute_service(
        &mut self,
        call: plugin_protocol::service::Invocation,
    ) -> Result<serde_json::Value, Failure> {
        match call.method.as_str() {
            "execute" => self.execute_call(call),
            "stop" => self.stop_call(call),
            _ => Err(Failure::new(
                ErrorCode::UnsupportedOperation,
                "Unknown execution method",
            )),
        }
    }

    /// Stop the program a delegated session owns.
    ///
    /// The provider offers no interactive keystroke path here: a caller asked for a program to end,
    /// so the owned process is terminated. The request is acknowledged once termination was issued,
    /// which is not a claim that the program has already exited.
    fn stop_call(
        &mut self,
        call: plugin_protocol::service::Invocation,
    ) -> Result<serde_json::Value, Failure> {
        if call.contract != "interactive.execute" {
            return Err(Failure::new(
                ErrorCode::UnsupportedOperation,
                "Unknown execution contract",
            ));
        }
        let request: Stop = serde_json::from_value(call.arguments)
            .map_err(|error| Failure::new(ErrorCode::InvalidRequest, error.to_string()))?;
        let session = request
            .session
            .parse::<u64>()
            .map_err(|_| Failure::new(ErrorCode::InvalidRequest, "Unknown session identity"))?;
        let index = self
            .tabs
            .iter()
            .position(|tab| tab.id == session)
            .ok_or_else(|| Failure::new(ErrorCode::InvalidHandle, "Session is no longer present"))?;
        let tab = &mut self.tabs[index];
        match tab.handle.take() {
            Some(handle) => {
                // A refused termination is reported as a failed stop, never as a stopped program.
                if let Err(message) = host::process(process::Operation::Terminate {
                    handle: handle.clone(),
                }) {
                    tab.handle = Some(handle);
                    return Err(Failure::new(ErrorCode::OperationFailed, message));
                }
                // A delegated program is never restarted with this provider's private authority.
                tab.resumable = false;
                tab.exited = true;
                Ok(serde_json::json!({"session": request.session, "state": "stopped"}))
            }
            // An exited session has nothing left to stop; reporting failure would be misleading.
            None => Ok(serde_json::json!({"session": request.session, "state": "exited"})),
        }
    }

    fn execute_call(
        &mut self,
        call: plugin_protocol::service::Invocation,
    ) -> Result<serde_json::Value, Failure> {
        if call.contract != "interactive.execute" {
            return Err(Failure::new(
                ErrorCode::UnsupportedOperation,
                "Unknown execution contract",
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
                    // New execution views have no existing cursor or transcript to inherit.
                    inherit_cursor: false,
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
