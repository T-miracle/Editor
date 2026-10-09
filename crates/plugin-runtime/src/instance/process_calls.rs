//! Execution authority is checked before resolution and remains attached to the owned process slot.
use super::*;
use api::{ErrorCode, Failure, Value};
use plugin_protocol::process::{Operation, Service, Transport, Update};
use resource_roots::RootKind;

impl State {
    /// Quiescing must capture observation before the service table is cleared during an update.
    pub(super) fn retire_native_processes(&mut self) {
        for (id, (handle, _)) in self.process_handles.clone() {
            let observer = self.native_observer(&handle);
            if let Err(error) = self.processes.close_observed(id, observer) {
                eprintln!("Native revocation failed: {error:#}");
            }
        }
    }

    /// Negotiation does not imply consent, and preparation never starts native programs.
    fn process_authority(&self, permission: &str) -> Result<(), Failure> {
        if !self.active || self.roots.retired {
            return Err(Failure::new(
                ErrorCode::InvalidState,
                "Instance is not active",
            ));
        }
        if !self.api.capabilities.contains_key("process") {
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
            Operation::PresentTerminal { handle, title } => {
                let id = self.presentation_authority(&handle)?;
                if !self.processes.is_pty(id) {
                    return Err(Failure::new(
                        ErrorCode::InvalidRequest,
                        "Only an owned PTY can be presented",
                    ));
                }
                let lifetimes = self
                    .plugin_services
                    .resources
                    .get(&handle.resource)
                    .map(|(_, context)| context.lifetimes.clone())
                    .unwrap_or_default();
                let owner = self.host_resources.preparations.terminal_owner(&lifetimes);
                self.host_resources.terminals.present(
                    &handle,
                    title,
                    owner,
                    true,
                    self.process_display_scopes[&id].clone(),
                )?;
                Ok(Value::Unit)
            }
            Operation::TerminalOutput {
                handle,
                title,
                bytes,
            } => {
                let id = self.presentation_authority(&handle)?;
                if bytes.len() > 16 * 1024 {
                    return Err(Failure::new(
                        ErrorCode::LimitExceeded,
                        "Decoded output exceeds 16 KiB",
                    ));
                }
                let lifetimes = self
                    .plugin_services
                    .resources
                    .get(&handle.resource)
                    .map(|(_, context)| context.lifetimes.clone())
                    .unwrap_or_default();
                let owner = self.host_resources.preparations.terminal_owner(&lifetimes);
                self.host_resources.terminals.present(
                    &handle,
                    title,
                    owner,
                    false,
                    self.process_display_scopes[&id].clone(),
                )?;
                self.host_resources.terminals.output(&handle, bytes)?;
                Ok(Value::Unit)
            }
            Operation::Resolve { program } => {
                self.process_authority("process.exec")?;
                if !self
                    .api
                    .capabilities
                    .get("process")
                    .is_some_and(|version| *version >= semver::Version::new(1, 6, 0))
                {
                    return Err(Failure::new(
                        ErrorCode::CapabilityUnavailable,
                        "process 1.6 is required for tool resolution",
                    ));
                }
                let declaration = Service {
                    program: program.clone(),
                    args: vec![],
                    search_paths: vec![],
                    check_args: vec![],
                    installation: None,
                };
                if !declaration.valid() {
                    return Err(Failure::new(
                        ErrorCode::InvalidRequest,
                        "Expected a bare tool name or absolute executable path",
                    ));
                }
                // Never probe/start candidates, install tools, or reserve a process slot here.
                let path = crate::toolchains::resolve(&program)
                    .map_err(|error| Failure::new(ErrorCode::NotFound, error.to_string()))?;
                Ok(Value::ResolvedProgram {
                    program: path.display().to_string(),
                })
            }
            Operation::StartService { service } => {
                // Delegated creation additionally requires the original caller's process.exec.
                // This installed service grant approves fixed bytes/argv; callers cannot override it.
                let permission = format!("process.service.{service}");
                self.process_authority(&permission)?;
                let command = self
                    .services
                    .get(&service)
                    .cloned()
                    .ok_or_else(|| Failure::new(ErrorCode::NotFound, "Undeclared service"))?;
                self.start_process(
                    command,
                    Transport::Stdio,
                    permission,
                    None,
                    Default::default(),
                )
            }
            Operation::Execute {
                program,
                args,
                transport,
                cwd,
                env,
            } => {
                self.process_authority("process.exec")?;
                // Environment overrides are process 1.4; a package that did not negotiate it cannot
                // silently reach a child with variables its declared version never promised.
                if !env.is_empty()
                    && !self
                        .api
                        .capabilities
                        .get("process")
                        .is_some_and(|version| *version >= semver::Version::new(1, 4, 0))
                {
                    return Err(Failure::new(
                        ErrorCode::CapabilityUnavailable,
                        "process 1.4 is required for environment overrides",
                    ));
                }
                self.start_process(
                    Service {
                        program,
                        args,
                        search_paths: Vec::new(),
                        check_args: Vec::new(),
                        installation: None,
                    },
                    transport,
                    "process.exec".into(),
                    cwd,
                    env,
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
            Operation::RequestExit { handle, mode } => {
                let id = self.process_id(&handle)?;
                if !self
                    .api
                    .capabilities
                    .get("process")
                    .is_some_and(|version| *version >= semver::Version::new(1, 5, 0))
                {
                    return Err(Failure::new(
                        ErrorCode::CapabilityUnavailable,
                        "process 1.5 is required for observed exit",
                    ));
                }
                match mode {
                    plugin_protocol::process::ExitMode::Graceful => {
                        if !self.processes.request_exit(id).map_err(process_failure)? {
                            return Err(Failure::new(
                                ErrorCode::UnsupportedOperation,
                                "This transport has no normal-exit operation",
                            ));
                        }
                    }
                    plugin_protocol::process::ExitMode::Force => {
                        self.processes.terminate(id).map_err(process_failure)?;
                    }
                }
                // Ownership and pending output survive this admission acknowledgement. The native
                // completion event is the only evidence on which a caller may start a replacement.
                Ok(Value::Unit)
            }
            Operation::Terminate { handle } => {
                let id = self.process_id(&handle)?;
                // Resource release revokes guest delivery, while the original preparation waits for
                // the real tree/EOF receipt and retains tail output through its authenticated reaper.
                let observer = self.native_observer(&handle);
                self.processes
                    .close_observed(id, observer)
                    .map_err(process_failure)?;
                self.process_handles.remove(&id);
                self.process_display_scopes.remove(&id);
                self.process_dependencies.remove(&id);
                self.plugin_services.resources.remove(&handle.resource);
                self.roots.remove(&handle);
                // Termination is synchronous ownership release; it does not undo native side effects.
                Ok(Value::Process(Update::Terminated))
            }
        }
    }

    /// Both interactive presentation and decoded diagnostics retain the original execution grants.
    fn presentation_authority(&self, handle: &api::ResourceHandle) -> Result<u64, Failure> {
        self.process_authority("process.exec")?;
        let id = self.process_id(handle)?;
        if !self
            .api
            .capabilities
            .get("process")
            .is_some_and(|v| *v >= semver::Version::new(1, 7, 0))
        {
            return Err(Failure::new(
                ErrorCode::CapabilityUnavailable,
                "process 1.7 is required for terminal presentation",
            ));
        }
        if !self.permissions.contains("ui.panels") {
            return Err(Failure::new(
                ErrorCode::PermissionDenied,
                "ui.panels permission required",
            ));
        }
        Ok(id)
    }
    pub(super) fn process_id(&self, handle: &api::ResourceHandle) -> Result<u64, Failure> {
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
        cwd: Option<String>,
        // Caller-supplied environment applied over what the child inherits; empty for a start that
        // has no such request.
        env: std::collections::BTreeMap<String, String>,
    ) -> Result<Value, Failure> {
        // Cursor inheritance is a negotiated transport option, independent of package identity.
        if matches!(
            transport,
            Transport::Pty {
                inherit_cursor: true,
                ..
            }
        ) {
            if !self
                .api
                .capabilities
                .get("process")
                .is_some_and(|version| *version >= semver::Version::new(1, 3, 0))
            {
                return Err(Failure::new(
                    ErrorCode::CapabilityUnavailable,
                    "process 1.3 is required for cursor inheritance",
                ));
            }
            if !cfg!(windows) {
                return Err(Failure::new(
                    ErrorCode::UnsupportedOperation,
                    "Cursor inheritance is only available for Windows PTYs",
                ));
            }
        }
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
        let mut command = command;
        if permission == "process.exec"
            && !Path::new(&command.program).is_absolute()
            && let Some(path) = env.get("PATH").or_else(|| env.get("Path"))
        {
            // Explicit granted environment overrides also choose the executable, before host PATH.
            // Relative directories remain excluded so project files cannot become implicit tools.
            let name = if cfg!(windows) && Path::new(&command.program).extension().is_none() {
                format!("{}.exe", command.program)
            } else {
                command.program.clone()
            };
            command.search_paths = std::env::split_paths(path)
                .filter(|directory| directory.is_absolute())
                .take(32)
                .map(|directory| directory.join(&name).display().to_string())
                .collect();
        }
        let program = if let Some(prepared) = &prepared {
            prepared.program.clone()
        } else {
            crate::toolchains::resolve_service_until(&command, self.call_deadline)
                .map_err(process_failure)?
        };
        let args = if let Some(prepared) = &prepared {
            prepared.args(&command.args).map_err(process_failure)?
        } else {
            command.args
        };
        // Arbitrary execution already grants native filesystem authority; service-only calls cannot set cwd.
        let cwd = match cwd {
            Some(cwd) => {
                if !self
                    .api
                    .capabilities
                    .get("process")
                    .is_some_and(|version| *version >= semver::Version::new(1, 2, 0))
                {
                    return Err(Failure::new(
                        ErrorCode::CapabilityUnavailable,
                        "process 1.2 is required for cwd",
                    ));
                }
                let path = PathBuf::from(&cwd);
                if cwd.len() > 4096 || cwd.contains('\0') || !path.is_absolute() || !path.is_dir() {
                    return Err(Failure::new(
                        ErrorCode::InvalidPath,
                        "Process cwd must be an existing absolute directory",
                    ));
                }
                path
            }
            None if self.roots.application || self.workspace.as_os_str().is_empty() => {
                self.data.clone()
            }
            None => self.workspace.clone(),
        };
        let id = match transport {
            Transport::Stdio => self.processes.spawn_stdio(&program, &args, &cwd, &env),
            Transport::Pty {
                columns,
                rows,
                inherit_cursor,
            } => self.processes.spawn(
                program.display().to_string(),
                args,
                cwd.display().to_string(),
                columns,
                rows,
                inherit_cursor,
                env.clone(),
                self.native_diagnostics.reporter(
                    &self.plugin_services.principal.plugin,
                    &self.plugin_services.principal.scope,
                ),
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
            let source_scope = self
                .plugin_services
                .context
                .as_ref()
                .map(|context| context.caller.scope.as_str())
                .unwrap_or(&handle.scope);
            let scope = if source_scope == "application" {
                self.host_resources.terminals.current_scope()
            } else {
                source_scope.into()
            };
            self.process_display_scopes.insert(id, scope);
        }
        Ok(value)
    }

    /// Native output is drained before exit, and notifications expose no transferable OS handles.
    pub(super) fn poll_processes(&mut self) -> anyhow::Result<Vec<api::Notification>> {
        let controls = self
            .process_handles
            .iter()
            .filter_map(|(id, (handle, _))| {
                let (_, context) = self.plugin_services.resources.get(&handle.resource)?;
                self.host_resources
                    .preparations
                    .control(&context.lifetimes, handle)
                    .map(|mode| (*id, mode, context.lifetimes.clone()))
            })
            .collect::<Vec<_>>();
        for (id, mode, lifetimes) in controls {
            match mode {
                process::ExitMode::Graceful => {
                    if !self.processes.request_exit(id).unwrap_or(false) {
                        self.host_resources.preparations.escalate(&lifetimes);
                        self.processes.terminate(id)?;
                    }
                }
                process::ExitMode::Force => self.processes.terminate(id)?,
            }
        }
        let mut events = Vec::new();
        let handles = &self.process_handles;
        let projections = &self.host_resources.terminals;
        for (id, result) in self.processes.poll_native_ready(|id| {
            handles
                .get(&id)
                .is_none_or(|(handle, _)| projections.ready(handle))
        }) {
            if let Some((handle, _)) = self.process_handles.get(&id).cloned() {
                // Read the entire supervisor batch. One PTY error must not lose another owner's
                // already-read bytes or final receipt after its native slot has been removed.
                let update = match result {
                    Ok(update) => update,
                    Err(error) => {
                        self.host_resources
                            .terminals
                            .fail(&handle, error.to_string());
                        if let Some((_, context)) =
                            self.plugin_services.resources.get(&handle.resource)
                        {
                            let mut observer = self
                                .host_resources
                                .preparations
                                .closing(&context.lifetimes, &handle);
                            observer(Err(error.to_string()));
                        }
                        continue;
                    }
                };
                self.host_resources.terminals.update(&handle, &update);
                if let Some((_, context)) = self.plugin_services.resources.get(&handle.resource) {
                    self.host_resources
                        .preparations
                        .update(&context.lifetimes, &handle, &update);
                }
                events.push(api::Notification::Process { handle, update });
            }
        }
        Ok(events)
    }

    /// Revalidate each delivery because a previous output callback can close this same handle.
    pub(super) fn accept_process_event(&mut self, event: &api::Notification) -> bool {
        let api::Notification::Process { handle, update } = event else {
            return true;
        };
        let Ok(RootKind::Process(id)) = self.roots.resolve(handle) else {
            return false;
        };
        if matches!(update, Update::Exited { .. } | Update::Terminated) {
            self.process_dependencies.remove(&id);
            self.process_handles.remove(&id);
            self.process_display_scopes.remove(&id);
            self.roots.remove(handle);
        }
        true
    }
}

impl State {
    /// Closing delivery keeps both lifecycle and presented tail observations after guest revocation.
    fn native_observer(
        &self,
        handle: &api::ResourceHandle,
    ) -> Box<dyn FnMut(Result<Update, String>) + Send> {
        let lifetimes = self
            .plugin_services
            .resources
            .get(&handle.resource)
            .map(|(_, context)| context.lifetimes.clone())
            .unwrap_or_default();
        let mut lifecycle = self.host_resources.preparations.closing(&lifetimes, handle);
        let terminals = self.host_resources.terminals.clone();
        let handle = handle.clone();
        Box::new(move |result| {
            match &result {
                Ok(update) => terminals.update(&handle, update),
                Err(error) => terminals.fail(&handle, error.clone()),
            }
            lifecycle(result);
        })
    }
}

/// Native I/O failure preserves a typed failure without pretending a side effect was undone.
pub(super) fn process_failure(error: anyhow::Error) -> Failure {
    Failure::new(ErrorCode::OperationFailed, error.to_string())
}
