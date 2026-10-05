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
    /// Execution 2.0 entries applied over the environment the child inherits.
    #[serde(default)]
    env: Vec<EnvEntry>,
}

/// One caller-supplied environment entry, so a variable name is never parsed from a joined string.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvEntry {
    name: String,
    value: String,
}

/// Execution 2.0 names the owned session and distinguishes normal exit from immediate termination.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Stop {
    /// Session identity this provider returned when the program was started.
    session: String,
    /// The default allows cleanup; force explicitly asks to terminate the owned process tree.
    #[serde(default)]
    mode: process::ExitMode,
}

/// Execution contract 2.0 asks what became of one session, including unsigned native exit statuses.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Status {
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
            "status" => self.status_call(call),
            "input" | "locate" | "events" => self.observation_call(call),
            _ => Err(Failure::new(
                ErrorCode::UnsupportedOperation,
                "Unknown execution method",
            )),
        }
    }

    /// Remember the exit code the host reports, so a status query answers from observation rather
    /// than from an assumption about how long a program usually takes.
    pub(super) fn note_process_update(
        &mut self,
        handle: &api::ResourceHandle,
        update: &process::Update,
    ) {
        if let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|tab| tab.handle.as_ref() == Some(handle))
        {
            tab.observations.observe(update);
            if matches!(update, process::Update::Output { .. }) {
                return;
            }
            // Windows exit statuses occupy the entire u32 range, including CTRL+C (0xc000013a).
            // Reserving a sentinel in that range would confuse a program result with forceful exit.
            tab.exit_code = match update {
                process::Update::Exited { code } => Some(*code),
                _ => None,
            };
            // A force request retains its provenance when Windows reports a numeric exit code.
            // The process event confirms completion; its shape does not undo the admitted mode.
            tab.terminated |= matches!(update, process::Update::Terminated);
        }
    }

    /// Report what became of one delegated session.
    ///
    /// The answer is an observation, never a prediction: a session whose program is still running
    /// reports `running`, and one whose exit the provider has seen reports the code it saw.
    fn status_call(
        &mut self,
        call: plugin_protocol::service::Invocation,
    ) -> Result<serde_json::Value, Failure> {
        if call.contract != "interactive.execute" {
            return Err(Failure::new(
                ErrorCode::UnsupportedOperation,
                "Unknown execution contract",
            ));
        }
        let request: Status = serde_json::from_value(call.arguments)
            .map_err(|error| Failure::new(ErrorCode::InvalidRequest, error.to_string()))?;
        let session = request
            .session
            .parse::<u64>()
            .map_err(|_| Failure::new(ErrorCode::InvalidRequest, "Unknown session identity"))?;
        let tab = self
            .tabs
            .iter()
            .find(|tab| {
                tab.id == session && tab.service_owner.as_deref() == Some(&call.caller.instance)
            })
            .ok_or_else(|| {
                Failure::new(ErrorCode::InvalidHandle, "Session is no longer present")
            })?;
        let state = match (tab.exited || tab.handle.is_none(), tab.exit_code) {
            (false, _) => "running",
            (true, _) if tab.terminated => "terminated",
            (true, None) => "ended",
            (true, Some(_)) => "exited",
        };
        // Only the keys that describe this answer are present: an absent exit status is left out
        // rather than sent as a null the declared schema never describes.
        let mut answer = serde_json::json!({
            "session": request.session,
            "state": state,
        });
        if let Some(code) = tab.exit_code {
            answer["code"] = serde_json::json!(code);
        }
        Ok(answer)
    }

    /// Stop the program a delegated session owns.
    ///
    /// Keep the process and its output attached while it handles normal or forced exit.
    /// Admission cannot replace the final native observation of the program's completion.
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
            .position(|tab| {
                tab.id == session && tab.service_owner.as_deref() == Some(&call.caller.instance)
            })
            .ok_or_else(|| {
                Failure::new(ErrorCode::InvalidHandle, "Session is no longer present")
            })?;
        let tab = &mut self.tabs[index];
        match tab.handle.clone() {
            Some(handle) => {
                host::call(api::Operation::Process {
                    operation: process::Operation::RequestExit {
                        handle,
                        mode: request.mode,
                    },
                })?;
                // Record only a successfully admitted force request, while still waiting for exit.
                tab.terminated |= request.mode == process::ExitMode::Force;
                // A delegated program is never restarted with this provider's private authority.
                tab.resumable = false;
                tab.observations.state(
                    if request.mode == process::ExitMode::Force {
                        "terminating"
                    } else {
                        "stopping"
                    },
                    None,
                );
                Ok(serde_json::json!({"session": request.session, "state": "stopping"}))
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
        // Retained output is bounded history, not a permanent slot reservation. Active sessions and
        // private shells are never evicted to admit another delegated program.
        while self.tabs.len() >= 32 {
            let Some(index) = self
                .tabs
                .iter()
                .position(|tab| tab.exited && tab.service_owner.is_some())
            else {
                break;
            };
            self.tabs.remove(index);
            if self.active > index && self.active != usize::MAX {
                self.active -= 1;
            } else if self.active == index {
                self.active = self
                    .tabs
                    .iter()
                    .position(|tab| !tab.hidden)
                    .unwrap_or(usize::MAX);
            }
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
                // The caller's environment belongs to the program it asked for; the provider forwards
                // it unchanged and never reads or logs the values.
                env: request
                    .env
                    .iter()
                    .map(|entry| (entry.name.clone(), entry.value.clone()))
                    .collect(),
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
        tab.service_owner = Some(call.caller.instance);
        tab.observations.state("running", None);
        tab.resumable = false;
        self.pending_editor
            .insert(visibility.resource, commands::PendingEditor::Effect);
        self.error = None;
        self.menu = None;
        Ok(serde_json::json!({"session":id.to_string(), "state":"started"}))
    }
}
