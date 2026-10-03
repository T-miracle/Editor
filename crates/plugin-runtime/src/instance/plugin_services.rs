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
    pub references: BTreeMap<u64, Reference>,
    pub pending: BTreeMap<u64, Pending>,
    pub context: Option<CallContext>,
    pub invoking: bool,
    pub resources: BTreeMap<u64, (api::ResourceHandle, CallContext)>,
    /// Final cleanup notifications retain revoked authority and are drained outside broker reconciliation.
    pub revoked_processes: Vec<(api::ResourceHandle, CallContext)>,
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
            references: Default::default(),
            pending: Default::default(),
            context: None,
            invoking: false,
            resources: Default::default(),
            revoked_processes: Vec::new(),
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
        self.references.clear();
        self.context = None;
        self.resources.clear();
        self.revoked_processes.clear();
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
        if !self
            .api
            .as_ref()
            .is_some_and(|api| api.capabilities.contains_key("plugin.services"))
        {
            return Err(Failure::new(
                ErrorCode::CapabilityUnavailable,
                "plugin.services was not negotiated",
            ));
        }
        if !self.permissions.contains("services.call") {
            return Err(Failure::new(
                ErrorCode::PermissionDenied,
                "services.call permission required",
            ));
        }
        match operation {
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
                            lifetimes: vec![self.plugin_services.alive.clone()],
                            caller: self.plugin_services.principal.clone(),
                            ancestry: vec![self.plugin_services.principal.instance.clone()],
                            permissions: self.permissions.clone(),
                        });
                if context.ancestry.len() >= 8
                    || context
                        .ancestry
                        .contains(&reference.provider.caller.instance)
                {
                    return Err(Failure::new(
                        ErrorCode::Conflict,
                        "Service call cycle or depth limit detected",
                    ));
                }
                if !signature.permissions.is_subset(&context.permissions)
                    || !signature
                        .permissions
                        .is_subset(&reference.provider.caller.permissions)
                {
                    return Err(Failure::new(
                        ErrorCode::PermissionDenied,
                        "Caller and provider must both authorize the service method",
                    ));
                }
                context.permissions = signature.permissions.clone();
                context
                    .ancestry
                    .push(reference.provider.caller.instance.clone());
                context.lifetimes.push(reference.provider.alive.clone());
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
    /// Delegation never grants access to a provider's private files or its pre-existing resource handles.
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
                api::EditorOperation::ReadClipboard
                | api::EditorOperation::WriteClipboard { .. } => "clipboard",
                api::EditorOperation::OpenDataFile { .. } => {
                    return Err(Failure::new(
                        ErrorCode::PermissionDenied,
                        "Service calls cannot open private provider files",
                    ));
                }
                api::EditorOperation::SaveDocument { .. } => "editor.write",
                api::EditorOperation::SetPanelVisibility { .. } => "ui.panels",
                _ => "editor.read",
            },
            api::Operation::Process {
                operation: process::Operation::Execute { .. },
            } => "process.exec",
            api::Operation::Process {
                operation:
                    process::Operation::Write { handle, .. }
                    | process::Operation::Resize { handle, .. }
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
            let process = matches!(
                self.store.data().roots.resolve(&handle),
                Ok(RootKind::Process(_))
            );
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
            }
        }
    }
    /// A final Terminated update may update local UI, but every new host effect still sees the dead source.
    pub(super) fn poll_service_revocations(&mut self) -> anyhow::Result<bool> {
        let revoked = std::mem::take(&mut self.store.data_mut().plugin_services.revoked_processes);
        let changed = !revoked.is_empty();
        for (handle, context) in revoked {
            // Bypass only the ordinary late-event suppression, never the authority check on host imports.
            let previous = self
                .store
                .data_mut()
                .plugin_services
                .context
                .replace(context);
            let result = self.call(Message::Event(Event::Capability(
                api::Notification::Process {
                    handle,
                    update: process::Update::Terminated,
                },
            )));
            self.store.data_mut().plugin_services.context = previous;
            result?;
        }
        Ok(changed)
    }
    /// Async continuations inherit their source context instead of regaining provider privileges.
    pub(super) fn call_with_service_context(
        &mut self,
        context: Option<CallContext>,
        message: Message,
    ) -> anyhow::Result<Reply> {
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
            return Ok(Reply::default());
        }
        let previous =
            std::mem::replace(&mut self.store.data_mut().plugin_services.context, context);
        let result = self.call(message);
        self.store.data_mut().plugin_services.context = previous;
        result
    }
    pub(crate) fn service_provider(&self) -> Option<Provider> {
        (!self.store.data().roots.retired && self.store.data().active).then(|| Provider {
            alive: self.store.data().plugin_services.alive.clone(),
            caller: self.store.data().plugin_services.principal.clone(),
            contracts: self
                .store
                .data()
                .plugin_services
                .declarations
                .provides
                .clone(),
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
    /// Call under a shrinking source context and always restore normal authority, including error paths.
    pub(crate) fn invoke_service(&mut self, call: &Call) -> Result<serde_json::Value, Failure> {
        self.store.data_mut().plugin_services.invoking = true;
        self.store.data_mut().plugin_services.context = Some(call.context.clone());
        let mut caller = call.context.caller.clone();
        caller.permissions = call.context.permissions.clone();
        let result = self.call(Message::Event(Event::Capability(
            api::Notification::Service(service::Notification::Invoke(service::Invocation {
                caller,
                contract: call.reference.contract.clone(),
                method: call.method.clone(),
                arguments: call.arguments.clone(),
            })),
        )));
        self.store.data_mut().plugin_services.context = None;
        self.store.data_mut().plugin_services.invoking = false;
        match result {
            Ok(reply) => reply.service_reply.ok_or_else(|| {
                Failure::new(
                    ErrorCode::InvalidRequest,
                    "Service provider omitted its result",
                )
            })?,
            Err(error) => {
                if let Some(failure) = error.downcast_ref::<Failure>() {
                    return Err(failure.clone());
                }
                self.stop();
                Err(Failure::new(
                    ErrorCode::OperationFailed,
                    format!("Service provider failed: {error:#}"),
                ))
            }
        }
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
                ))
            })
            .collect::<Vec<_>>();
        for (slot, handle, update, context) in updates {
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
                Message::Event(Event::Capability(api::Notification::Service(
                    service::Notification::Request { handle, update },
                ))),
            )?;
        }
        Ok(())
    }
}
