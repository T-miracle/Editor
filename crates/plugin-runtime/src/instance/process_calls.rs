//! Execution authority is checked before resolution and remains attached to the owned process slot.
use super::*;
use api::{ErrorCode, Failure, Value};
use plugin_protocol::process::{Operation, Service, Transport, Update};
use resource_roots::RootKind;

impl State {
    /// Negotiation does not imply consent, and preparation never starts native programs.
    fn process_authority(&self, permission: &str) -> Result<(), Failure> {
        if !self.active || self.roots.retired {
            return Err(Failure::new(
                ErrorCode::InvalidState,
                "Instance is not active",
            ));
        }
        if !self
            .api
            .as_ref()
            .is_some_and(|api| api.capabilities.contains_key("process"))
        {
            return Err(Failure::new(
                ErrorCode::CapabilityUnavailable,
                "process was not negotiated",
            ));
        }
        if !self.permissions.contains(permission) {
            return Err(Failure::new(
                ErrorCode::PermissionDenied,
                format!("{permission} permission required"),
            ));
        }
        Ok(())
    }

    /// Service calls cannot provide a program, arguments, cwd, environment, or installer step.
    pub(super) fn process_request(&mut self, operation: Operation) -> Result<Value, Failure> {
        match operation {
            Operation::StartService { service } => {
                let permission = format!("process.service.{service}");
                self.process_authority(&permission)?;
                let command = self
                    .services
                    .get(&service)
                    .cloned()
                    .ok_or_else(|| Failure::new(ErrorCode::NotFound, "Undeclared service"))?;
                self.start_process(command, Transport::Stdio, permission)
            }
            Operation::Execute {
                program,
                args,
                transport,
            } => {
                self.process_authority("process.exec")?;
                self.start_process(
                    Service {
                        program,
                        args,
                        installation: None,
                    },
                    transport,
                    "process.exec".into(),
                )
            }
            Operation::Write { handle, bytes } => {
                let id = self.process_id(&handle)?;
                if bytes.len() > 1024 * 1024 {
                    return Err(Failure::new(
                        ErrorCode::LimitExceeded,
                        "Process input quota exceeded",
                    ));
                }
                self.processes.write(id, &bytes).map_err(process_failure)?;
                Ok(Value::Unit)
            }
            Operation::Resize {
                handle,
                columns,
                rows,
            } => {
                let id = self.process_id(&handle)?;
                self.processes
                    .resize(id, columns, rows)
                    .map_err(process_failure)?;
                Ok(Value::Unit)
            }
            Operation::Terminate { handle } => {
                let id = self.process_id(&handle)?;
                // A natural exit may already be queued; explicit close still retires pending delivery.
                let _ = self.processes.close(id);
                self.process_handles.remove(&id);
                self.process_dependencies.remove(&id);
                self.roots.remove(&handle);
                // Termination is synchronous ownership release; it does not undo native side effects.
                Ok(Value::Process(Update::Terminated))
            }
        }
    }

    fn process_id(&self, handle: &api::ResourceHandle) -> Result<u64, Failure> {
        let RootKind::Process(id) = self.roots.resolve(handle)? else {
            return Err(Failure::new(
                ErrorCode::InvalidHandle,
                "Not a process handle",
            ));
        };
        let (_, permission) = self
            .process_handles
            .get(&id)
            .ok_or_else(|| Failure::new(ErrorCode::InvalidHandle, "Process has exited"))?;
        self.process_authority(permission)?;
        Ok(id)
    }

    /// Resolve a fixed declaration through the toolchain boundary, then bind its OS resource to this owner.
    fn start_process(
        &mut self,
        command: Service,
        transport: Transport,
        permission: String,
    ) -> Result<Value, Failure> {
        if self.processes.len() >= 32 {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Process quota exceeded",
            ));
        }
        if !command.valid() {
            return Err(Failure::new(
                ErrorCode::InvalidRequest,
                "Invalid executable or arguments",
            ));
        }
        // No guest callback or await occurs between this reservation check and handle allocation.
        self.roots.ensure_capacity()?;
        let prepared = command
            .installation
            .as_ref()
            .map(|plan| {
                anyhow::ensure!(
                    self.permissions.contains("dependencies.prepare"),
                    "Dependency permission required"
                );
                let root = self
                    .assets
                    .ancestors()
                    .nth(3)
                    .ok_or_else(|| anyhow::anyhow!("Missing managed package root"))?;
                crate::dependencies::cached(root, plan)
            })
            .transpose()
            .map_err(process_failure)?;
        let program = if let Some(prepared) = &prepared {
            prepared.program.clone()
        } else {
            crate::toolchains::resolve(&command.program).map_err(process_failure)?
        };
        let args = if let Some(prepared) = &prepared {
            prepared.args(&command.args).map_err(process_failure)?
        } else {
            command.args
        };
        let cwd = if self.roots.application || self.workspace.as_os_str().is_empty() {
            &self.data
        } else {
            &self.workspace
        };
        let id = match transport {
            Transport::Stdio => self.processes.spawn_stdio(&program, &args, cwd),
            Transport::Pty { columns, rows } => self.processes.spawn(
                program.display().to_string(),
                args,
                cwd.display().to_string(),
                columns,
                rows,
                false,
            ),
        }
        .map_err(process_failure)?;
        let value = match self.roots.open(RootKind::Process(id)) {
            Ok(value) => value,
            Err(error) => {
                let _ = self.processes.close(id);
                return Err(error);
            }
        };
        if let Value::Resource(handle) = &value {
            if let Some(prepared) = prepared {
                self.process_dependencies.insert(id, prepared.locks);
            }
            self.process_handles
                .insert(id, (handle.clone(), permission));
        }
        Ok(value)
    }

    /// Native output is drained before exit, and notifications expose no transferable OS handles.
    pub(super) fn poll_processes(&mut self) -> anyhow::Result<Vec<Event>> {
        if self.api.is_none() {
            return self.processes.poll();
        }
        let mut events = Vec::new();
        for (id, update) in self.processes.poll_native()? {
            if let Some((handle, _)) = self.process_handles.get(&id).cloned() {
                events.push(Event::Capability(api::Notification::Process {
                    handle,
                    update,
                }));
            }
        }
        Ok(events)
    }

    /// Revalidate each delivery because a previous output callback can close this same handle.
    pub(super) fn accept_process_event(&mut self, event: &Event) -> bool {
        let Event::Capability(api::Notification::Process { handle, update }) = event else {
            return true;
        };
        let Ok(RootKind::Process(id)) = self.roots.resolve(handle) else {
            return false;
        };
        if matches!(update, Update::Exited { .. }) {
            self.process_dependencies.remove(&id);
            self.process_handles.remove(&id);
            self.roots.remove(handle);
        }
        true
    }
}

/// Native I/O failure preserves a typed failure without pretending a side effect was undone.
fn process_failure(error: anyhow::Error) -> Failure {
    Failure::new(ErrorCode::OperationFailed, error.to_string())
}
