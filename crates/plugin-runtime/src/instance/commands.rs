//! Typed commands use the established broker while preserving command-specific admission.
use super::*;
use crate::plugin_services::{Call, Context as CallContext, Pending};
use api::{ErrorCode, Failure, Value};
use plugin_protocol::commands;
use resource_roots::RootKind;

impl State {
    /// Discover without executing guests; invocation shrinks source authority at every hop.
    pub(super) fn command_request(
        &mut self,
        operation: commands::Operation,
    ) -> Result<Value, Failure> {
        if !self.active
            || self.roots.retired
            || !self.api.capabilities.contains_key("plugin.commands")
        {
            return Err(Failure::new(
                ErrorCode::CapabilityUnavailable,
                "An active plugin.commands instance is required",
            ));
        }
        match operation {
            commands::Operation::Reply { request, result } => {
                self.finish_service_reply(request, result)
            }
            commands::Operation::Discover => Ok(Value::Commands(
                self.plugin_services
                    .broker
                    .lock()
                    .unwrap()
                    .commands(&self.plugin_services.principal),
            )),
            commands::Operation::Invoke {
                plugin,
                command,
                arguments,
                timeout_ms,
            } => {
                if !self.permissions.contains("commands.call") {
                    return Err(Failure::new(
                        ErrorCode::PermissionDenied,
                        "commands.call permission required",
                    ));
                }
                let reference = self
                    .plugin_services
                    .broker
                    .lock()
                    .unwrap()
                    .command_reference(&self.plugin_services.principal, &plugin, &command)?;
                if timeout_ms == 0
                    || timeout_ms > 300_000
                    || self.plugin_services.pending.len() >= 32
                    || serde_json::to_vec(&arguments).map_or(true, |value| value.len() > 65536)
                {
                    return Err(Failure::new(
                        ErrorCode::LimitExceeded,
                        "Invalid command deadline, payload or request quota",
                    ));
                }
                let signature = reference.dependency.methods["invoke"].clone();
                signature.parameters.accepts(&arguments)?;
                let mut source =
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
                // Native menu metadata belongs only to the directly clicked instance.
                source.menu = None;
                let context = source.delegate(&reference.provider, &signature)?;
                let Value::Resource(handle) = self.roots.open(RootKind::ServiceRequest)? else {
                    unreachable!()
                };
                let mut completion = crate::Completion::new(timeout_ms);
                completion.lifetimes = source.lifetimes.clone();
                let call = Call {
                    handle: handle.clone(),
                    reference,
                    method: "invoke".into(),
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
}

impl Instance {
    /// A user-initiated host command runs under this exact target's installed grants and lifetime.
    pub(crate) fn queue_host_command(
        &mut self,
        command: &str,
        arguments: serde_json::Value,
        timeout_ms: u32,
        menu: Option<commands::Context>,
    ) -> Result<crate::Completion<serde_json::Value>, Failure> {
        let state = self.store.data();
        // Application instances never inherit a workspace menu path from a host call.
        let menu = if state.roots.application { None } else { menu };
        let signature = state
            .plugin_services
            .commands
            .get(command)
            .cloned()
            .ok_or_else(|| {
                Failure::new(
                    ErrorCode::UnsupportedOperation,
                    "Command has no typed signature",
                )
            })?;
        signature.parameters.accepts(&arguments)?;
        if timeout_ms == 0
            || timeout_ms > 300000
            || serde_json::to_vec(&arguments).map_or(true, |bytes| bytes.len() > 65536)
        {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Invalid host command payload or deadline",
            ));
        }
        let mut caller = state.plugin_services.principal.clone();
        caller.plugin = "@host".into();
        caller.instance = "@host".into();
        let reference = state
            .plugin_services
            .broker
            .lock()
            .unwrap()
            .command_reference(&caller, &state.plugin_services.principal.plugin, command)?;
        let source = CallContext {
            native_waits: Vec::new(),
            menu,
            origin: crate::plugin_services::InvocationOrigin::HostCommand {
                target: state.plugin_services.principal.instance.clone(),
            },
            lifetimes: vec![state.plugin_services.alive.clone()],
            caller,
            ancestry: Vec::new(),
            permissions: state.permissions.clone(),
        };
        let context = source.delegate(&reference.provider, &signature)?;
        let mut completion = crate::Completion::new(timeout_ms);
        completion.lifetimes = vec![state.plugin_services.alive.clone()];
        // Host waits have no guest root slot. Reconciliation anchors them to their exact target,
        // while the callback still reports the host caller and cannot borrow unrelated resources.
        let call = Call {
            handle: api::ResourceHandle {
                instance: state.plugin_services.principal.instance.clone(),
                scope: source.caller.scope,
                resource: 0,
            },
            reference,
            method: "invoke".into(),
            signature,
            arguments,
            context,
            completion: completion.clone(),
        };
        state.plugin_services.broker.lock().unwrap().enqueue(call)?;
        Ok(completion)
    }
}
