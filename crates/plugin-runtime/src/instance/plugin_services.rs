//! Instance-owned references and calls preserve authority at the boundary to the shared broker.
use super::*;
use crate::plugin_services::{
    self as broker, Call, Context as CallContext, Pending, Provider, Reference,
};
use api::{ErrorCode, Failure, Value};
use plugin_protocol::service::{self, Caller};
use resource_roots::RootKind;
use std::collections::BTreeMap;

pub(super) struct Services {
    pub alive: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub broker: broker::Shared,
    pub principal: Caller,
    pub declarations: service::Declarations,
    /// Typed commands use the broker's existing bounded call and source authority machinery.
    pub commands: BTreeMap<String, service::Method>,
    pub references: BTreeMap<u64, Reference>,
    pub pending: BTreeMap<u64, Pending>,
    /// At most 32 provider callbacks may await a later event, under their original call authority.
    pub incoming: BTreeMap<u64, super::service_replies::Incoming>,
    pub context: Option<CallContext>,
    pub invoking: bool,
    pub resources: BTreeMap<u64, (api::ResourceHandle, CallContext)>,
    /// Final cleanup notifications retain revoked authority and are drained outside broker reconciliation.
    pub revoked_processes: Vec<(api::ResourceHandle, CallContext)>,
    /// Retired invocation cleanup must also reach the guest without restoring source authority.
    pub revoked_invocations: Vec<(api::ResourceHandle, CallContext, bool)>,
}
impl Default for Services {
    fn default() -> Self {
        Self {
            alive: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
            broker: Default::default(),
            principal: Caller {
                plugin: String::new(),
                instance: String::new(),
                scope: String::new(),
                permissions: Default::default(),
            },
            declarations: Default::default(),
            commands: Default::default(),
            references: Default::default(),
            pending: Default::default(),
            incoming: Default::default(),
            context: None,
            invoking: false,
            resources: Default::default(),
            revoked_processes: Vec::new(),
            revoked_invocations: Vec::new(),
        }
    }
}
impl Services {
    /// Retirement seals clones already queued in the broker before releasing native root slots.
    pub fn clear(&mut self) {
        self.alive
            .store(false, std::sync::atomic::Ordering::Release);
        for pending in self.pending.values() {
            pending.call.completion.retire();
        }
        self.pending.clear();
        for incoming in self.incoming.values() {
            incoming.call.completion.retire();
        }
        self.incoming.clear();
        self.references.clear();
        self.context = None;
        self.resources.clear();
        self.revoked_processes.clear();
        self.revoked_invocations.clear();
    }
}

impl State {
    pub(super) fn service_request(
        &mut self,
        operation: service::Operation,
    ) -> Result<Value, Failure> {
        if !self.active || self.roots.retired {
            return Err(Failure::new(
                ErrorCode::InvalidState,
                "Service calls require an active instance",
            ));
        }
        if !self.api.capabilities.contains_key("plugin.services") {
            return Err(Failure::new(
                ErrorCode::CapabilityUnavailable,
                "plugin.services was not negotiated",
            ));
        }
        if !matches!(operation, service::Operation::Reply { .. })
            && !self.permissions.contains("services.call")
        {
            return Err(Failure::new(
                ErrorCode::PermissionDenied,
                "services.call permission required",
            ));
        }
        match operation {
            service::Operation::Reply { request, result } => {
                self.finish_service_reply(request, result)
            }
            service::Operation::Open { contract } => {
                let dependency = self
                    .plugin_services
                    .declarations
                    .requires
                    .get(&contract)
                    .ok_or_else(|| {
                        Failure::new(
                            ErrorCode::PermissionDenied,
                            "Service dependency was not declared",
                        )
                    })?;
                let reference = self.plugin_services.broker.lock().unwrap().resolve(
                    &self.plugin_services.principal,
                    &contract,
                    dependency,
                )?;
                let Value::Resource(handle) = self.roots.open(RootKind::ServiceReference)? else {
                    unreachable!()
                };
                self.plugin_services
                    .references
                    .insert(handle.resource, reference);
                Ok(Value::Resource(handle))
            }
            service::Operation::Call {
                reference,
                method,
                arguments,
                timeout_ms,
            } => {
                self.roots.resolve(&reference)?;
                let reference = self
                    .plugin_services
                    .references
                    .get(&reference.resource)
                    .cloned()
                    .ok_or_else(|| {
                        Failure::new(ErrorCode::InvalidHandle, "Not a service reference")
                    })?;
                self.plugin_services
                    .broker
                    .lock()
                    .unwrap()
                    .validate_reference(&self.plugin_services.principal, &reference)?;
                if timeout_ms == 0
                    || timeout_ms > 300_000
                    || self.plugin_services.pending.len() >= 32
                    || serde_json::to_vec(&arguments)
                        .map_err(|e| Failure::new(ErrorCode::InvalidRequest, e.to_string()))?
                        .len()
                        > 65536
                {
                    return Err(Failure::new(
                        ErrorCode::LimitExceeded,
                        "Invalid service deadline or queue/payload quota",
                    ));
                }
                let signature = reference
                    .dependency
                    .methods
                    .get(&method)
                    .cloned()
                    .ok_or_else(|| {
                        Failure::new(
                            ErrorCode::UnsupportedOperation,
                            "Method is not part of the declared dependency",
                        )
                    })?;
                signature.parameters.accepts(&arguments)?;
                let mut context =
                    self.plugin_services
                        .context
                        .clone()
                        .unwrap_or_else(|| CallContext {
                            native_waits: Vec::new(),
                            menu: None,
                            origin: crate::plugin_services::InvocationOrigin::Delegated,
                            lifetimes: vec![self.plugin_services.alive.clone()],
                            caller: self.plugin_services.principal.clone(),
                            ancestry: vec![self.plugin_services.principal.instance.clone()],
                            permissions: self.permissions.clone(),
                        });
                // A peer call receives its own parameters, never another command's native target.
                context.menu = None;
                let context = context.delegate(&reference.provider, &signature)?;
                let Value::Resource(handle) = self.roots.open(RootKind::ServiceRequest)? else {
                    unreachable!()
                };
                let mut completion = crate::request_state::Completion::new(timeout_ms);
                // The callee may trap while producing the final failure; only the requesting chain owns this wait.
                completion.lifetimes = context.lifetimes[..context.lifetimes.len() - 1].to_vec();
                let call = Call {
                    handle: handle.clone(),
                    reference,
                    method,
                    signature,
                    arguments,
                    context,
                    completion,
                };
                if let Err(error) = self
                    .plugin_services
                    .broker
                    .lock()
                    .unwrap()
                    .enqueue(call.clone())
                {
                    self.roots.remove(&handle);
                    return Err(error);
                }
                self.plugin_services.pending.insert(
                    handle.resource,
                    Pending {
                        call,
                        reported: 0,
                        return_context: self.plugin_services.context.clone(),
                    },
                );
                Ok(Value::Accepted(handle))
            }
        }
    }
    /// Delegation never borrows private resources; a genuine direct host command may use its own selection.
    pub(super) fn check_service_authority(
        &self,
        operation: &api::Operation,
    ) -> Result<(), Failure> {
        let Some(context) = &self.plugin_services.context else {
            return Ok(());
        };
        // Cleanup is allowed for the source's own resources even after its authority is revoked.
        let owned = |handle: &api::ResourceHandle| {
            self.roots.resolve(handle).is_ok()
                && self
                    .plugin_services
                    .resources
                    .get(&handle.resource)
                    .is_some_and(|(_, owner)| owner.caller.instance == context.caller.instance)
        };
        if matches!(operation, api::Operation::CloseResource { handle }
            | api::Operation::CancelRequest { handle, .. } if owned(handle))
        {
            return Ok(());
        }
        // This is checked by an internal Manager origin and exact one-hop instance identity,
        // not by the guest-visible caller name. Selected roots remain bound to this provider.
        if context.direct_host_selection(&self.plugin_services.principal.instance) {
            match operation {
                api::Operation::ReadFile { handle, .. }
                | api::Operation::WriteFile { handle, .. }
                | api::Operation::CloseResource { handle }
                    if matches!(self.roots.resolve(handle)?, RootKind::Selected) =>
                {
                    return self.check_selection_authority();
                }
                api::Operation::Editor {
                    operation:
                        api::EditorOperation::Interaction {
                            operation: plugin_protocol::interaction::Operation::Select { .. },
                        },
                    ..
                } => return self.check_selection_authority(),
                _ => {}
            }
        }
        // A normal preparation stop keeps existing processes owned until actual exit, while sealing
        // new allocations. Existing protocol writes let an approved DAP disconnect settle normally.
        if self.host_resources.preparations.sealed(&context.lifetimes)
            && !matches!(
                operation,
                api::Operation::Service {
                    operation: service::Operation::Reply { .. }
                } | api::Operation::Process {
                    operation: process::Operation::RequestExit { .. }
                        | process::Operation::Terminate { .. }
                        | process::Operation::Write { .. }
                }
            )
        {
            return Err(Failure::new(
                ErrorCode::Cancelled,
                "Preparation is stopping; new effects are sealed",
            ));
        }
        if !self
            .plugin_services
            .broker
            .lock()
            .unwrap()
            .context_alive(context)
        {
            return Err(Failure::new(
                ErrorCode::InvalidHandle,
                "Service source was retired",
            ));
        }
        let permission = match operation {
            api::Operation::Commands {
                operation: plugin_protocol::commands::Operation::Reply { .. },
            } => return Ok(()),
            api::Operation::Commands { .. } => "commands.call",
            api::Operation::ReadAsset { .. }
            | api::Operation::Service { .. }
            | api::Operation::DescribeSdk => return Ok(()),
            api::Operation::OpenWorkspace => "workspace.read",
            api::Operation::ReadFile { handle, .. } | api::Operation::FindFiles { handle, .. }
                if matches!(self.roots.resolve(handle)?, RootKind::Workspace) =>
            {
                "workspace.read"
            }
            api::Operation::Editor { operation, .. } => match operation {
                api::EditorOperation::OpenDocument {
                    resource: api::ResourceIdentity::Local { .. },
                } => {
                    if !context.permissions.contains("editor.read") {
                        return Err(Failure::new(
                            ErrorCode::PermissionDenied,
                            "Service source did not delegate editor.read",
                        ));
                    }
                    "workspace.read"
                }
                api::EditorOperation::Interaction {
                    operation: plugin_protocol::interaction::Operation::Select { .. },
                } => {
                    return Err(Failure::new(
                        ErrorCode::PermissionDenied,
                        "User selected file authority cannot be delegated",
                    ));
                }
                api::EditorOperation::Interaction { .. } => "ui.interaction",
                api::EditorOperation::NavigateDocument { target, .. } => {
                    // A service source must delegate both base editor access and the target grant.
                    // External navigation remains unavailable through the current service whitelist.
                    if !context.permissions.contains("editor.read") {
                        return Err(Failure::new(
                            ErrorCode::PermissionDenied,
                            "Service source did not delegate editor.read",
                        ));
                    }
                    super::editor_requests::navigation_permission(target)
                }
                api::EditorOperation::SaveImageInput { .. } => {
                    return Err(Failure::new(
                        ErrorCode::PermissionDenied,
                        "Native image offers cannot be delegated through services",
                    ));
                }
                api::EditorOperation::ReadClipboard
                | api::EditorOperation::WriteClipboard { .. } => "clipboard",
                api::EditorOperation::OpenDataFile { .. } => {
                    return Err(Failure::new(
                        ErrorCode::PermissionDenied,
                        "Service calls cannot open private provider files",
                    ));
                }
                // Delegated range edits cannot borrow the provider's stronger write grant.
                api::EditorOperation::SaveDocument { .. }
                | api::EditorOperation::ReplaceDocumentRange { .. } => "editor.write",
                api::EditorOperation::SetPanelVisibility { .. } => "ui.panels",
                _ => "editor.read",
            },
            api::Operation::Process {
                operation:
                    process::Operation::Execute { .. }
                    | process::Operation::StartService { .. }
                    | process::Operation::Resolve { .. },
            } => "process.exec",
            api::Operation::Process {
                operation:
                    process::Operation::Write { handle, .. }
                    | process::Operation::Resize { handle, .. }
                    | process::Operation::RequestExit { handle, .. }
                    | process::Operation::Terminate { handle },
            } if owned(handle) => "process.exec",
            _ => {
                return Err(Failure::new(
                    ErrorCode::PermissionDenied,
                    "Service invocation cannot borrow private provider resources",
                ));
            }
        };
        if !context.permissions.contains(permission) {
            return Err(Failure::new(
                ErrorCode::PermissionDenied,
                format!("Service source did not delegate {permission}"),
            ));
        }
        Ok(())
    }
}

impl Instance {
    /// Source revocation cancels delegated work and releases its resources before any late delivery.
    pub(crate) fn retire_service_sources(&mut self) {
        let services = &self.store.data().plugin_services;
        let broker = services.broker.lock().unwrap();
        let retired = services
            .resources
            .values()
            .filter(|(_, context)| !broker.context_alive(context))
            .map(|(handle, context)| (handle.clone(), context.clone()))
            .collect::<Vec<_>>();
        drop(broker);
        for (handle, context) in retired {
            let kind = self.store.data().roots.resolve(&handle);
            let process = matches!(kind, Ok(RootKind::Process(_)));
            let invocation = matches!(kind, Ok(RootKind::ServiceInvocation));
            let command = self
                .store
                .data()
                .plugin_services
                .incoming
                .get(&handle.resource)
                .is_some_and(|incoming| {
                    incoming
                        .call
                        .reference
                        .contract
                        .starts_with(plugin_protocol::commands::CONTRACT_PREFIX)
                });
            let released = self
                .store
                .data_mut()
                .resource_request(api::Operation::CloseResource {
                    handle: handle.clone(),
                });
            if process && released.is_ok() {
                self.store
                    .data_mut()
                    .plugin_services
                    .revoked_processes
                    .push((handle, context));
            } else if invocation && released.is_ok() {
                self.store
                    .data_mut()
                    .plugin_services
                    .revoked_invocations
                    .push((handle, context, command));
            }
        }
    }
    /// A final Terminated update may update local UI, but every new host effect still sees the dead source.
    pub(super) fn poll_service_revocations(&mut self) -> anyhow::Result<bool> {
        let revoked = std::mem::take(&mut self.store.data_mut().plugin_services.revoked_processes);
        let invocations =
            std::mem::take(&mut self.store.data_mut().plugin_services.revoked_invocations);
        let changed = !revoked.is_empty() || !invocations.is_empty();
        for (handle, context) in revoked {
            self.call_for_retirement(
                context,
                api::Input::Event {
                    panel: None,
                    event: api::Notification::Process {
                        handle,
                        update: process::Update::Terminated,
                    },
                },
            )?;
        }
        for (request, context, command) in invocations {
            self.call_for_retirement(
                context,
                api::Input::Event {
                    panel: None,
                    event: if command {
                        api::Notification::CommandCancelled {
                            request,
                            reason: Failure::new(
                                ErrorCode::InvalidHandle,
                                "Command source retired",
                            ),
                        }
                    } else {
                        api::Notification::Service(service::Notification::InvocationCancelled {
                            request,
                            reason: Failure::new(
                                ErrorCode::InvalidHandle,
                                "Service source retired",
                            ),
                        })
                    },
                },
            )?;
        }
        Ok(changed)
    }
    /// Final bookkeeping notifications bypass late delivery suppression with a sealed authority token.
    /// The token applies only to this callback; cancelling a wait cannot revoke already owned programs.
    pub(super) fn call_for_retirement(
        &mut self,
        mut context: CallContext,
        message: api::Input,
    ) -> anyhow::Result<api::Output> {
        context
            .lifetimes
            .push(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
                false,
            )));
        let previous = self
            .store
            .data_mut()
            .plugin_services
            .context
            .replace(context);
        let result = self.call(message);
        self.store.data_mut().plugin_services.context = previous;
        result
    }
    /// Async continuations inherit their source context instead of regaining provider privileges.
    pub(super) fn call_with_service_context(
        &mut self,
        context: Option<CallContext>,
        message: api::Input,
    ) -> anyhow::Result<api::Output> {
        if context.as_ref().is_some_and(|context| {
            !self
                .store
                .data()
                .plugin_services
                .broker
                .lock()
                .unwrap()
                .context_alive(context)
        }) {
            return Ok(api::Output::default());
        }
        let previous =
            std::mem::replace(&mut self.store.data_mut().plugin_services.context, context);
        let result = self.call(message);
        self.store.data_mut().plugin_services.context = previous;
        result
    }
    pub(crate) fn service_provider(&self) -> Option<Provider> {
        (!self.store.data().roots.retired && self.store.data().active).then(|| {
            let state = self.store.data();
            let mut contracts = state.plugin_services.declarations.provides.clone();
            for (id, signature) in &state.plugin_services.commands {
                contracts.insert(
                    plugin_protocol::commands::contract(
                        &state.plugin_services.principal.plugin,
                        id,
                    ),
                    service::Contract {
                        version: semver::Version::new(1, 0, 0),
                        methods: BTreeMap::from([("invoke".into(), signature.clone())]),
                    },
                );
            }
            Provider {
                alive: self.store.data().plugin_services.alive.clone(),
                caller: self.store.data().plugin_services.principal.clone(),
                contracts,
            }
        })
    }
    /// Required dependencies fail before activation; optional dependencies remain discoverable fallbacks.
    pub(crate) fn connect_services(&mut self, broker: broker::Shared) -> anyhow::Result<()> {
        let services = &mut self.store.data_mut().plugin_services;
        services.broker = broker;
        for (id, dependency) in &services.declarations.requires {
            if !dependency.optional {
                services
                    .broker
                    .lock()
                    .unwrap()
                    .resolve(&services.principal, id, dependency)?;
            }
        }
        Ok(())
    }
    pub(super) fn poll_service_requests(&mut self) -> anyhow::Result<()> {
        let updates = self
            .store
            .data_mut()
            .plugin_services
            .pending
            .iter_mut()
            .filter_map(|(slot, pending)| {
                let (version, update) = pending.call.completion.update();
                if version == pending.reported {
                    return None;
                }
                pending.reported = version;
                Some((
                    *slot,
                    pending.call.handle.clone(),
                    update,
                    pending.return_context.clone(),
                    pending
                        .call
                        .reference
                        .contract
                        .starts_with(plugin_protocol::commands::CONTRACT_PREFIX),
                ))
            })
            .collect::<Vec<_>>();
        for (slot, handle, update, context, command) in updates {
            if !self
                .store
                .data()
                .plugin_services
                .pending
                .contains_key(&slot)
            {
                continue;
            }
            if update.is_terminal() {
                self.store.data_mut().plugin_services.pending.remove(&slot);
                self.store
                    .data_mut()
                    .plugin_services
                    .resources
                    .remove(&slot);
                self.store.data_mut().roots.remove(&handle);
            }
            self.call_with_service_context(
                context,
                api::Input::Event {
                    panel: None,
                    event: if command {
                        api::Notification::CommandRequest { handle, update }
                    } else {
                        api::Notification::Service(service::Notification::Request {
                            handle,
                            update,
                        })
                    },
                },
            )?;
        }
        Ok(())
    }
}
