//! Provider-side deferred replies keep deadlines, signatures and source ownership through native events.
use super::*;
use crate::plugin_services::Call;
use api::{ErrorCode, Failure, ResourceHandle, Value};
use plugin_protocol::service;
use resource_roots::RootKind;

/// The provider receives its own opaque slot, never the consumer's private request handle.
#[derive(Clone)]
pub(super) struct Incoming {
    pub handle: ResourceHandle,
    pub call: Call,
}

impl State {
    /// Validate the exact retained invocation before publishing its one immutable terminal result.
    pub(super) fn finish_service_reply(
        &mut self,
        request: ResourceHandle,
        result: Result<serde_json::Value, Failure>,
    ) -> Result<Value, Failure> {
        if !self
            .api
            .capabilities
            .get("plugin.services")
            .is_some_and(|version| *version >= semver::Version::new(1, 1, 0))
            && !self.api.capabilities.contains_key("plugin.commands")
        {
            return Err(Failure::new(
                ErrorCode::CapabilityUnavailable,
                "plugin.services 1.1 is required",
            ));
        }
        if !matches!(self.roots.resolve(&request)?, RootKind::ServiceInvocation) {
            return Err(Failure::new(
                ErrorCode::InvalidHandle,
                "Not a deferred invocation",
            ));
        }
        let incoming = self
            .plugin_services
            .incoming
            .get(&request.resource)
            .cloned()
            .ok_or_else(|| Failure::new(ErrorCode::InvalidHandle, "Invocation already ended"))?;
        if self
            .plugin_services
            .context
            .as_ref()
            .is_some_and(|current| {
                current.caller.instance != incoming.call.context.caller.instance
                    || current.caller.scope != incoming.call.context.caller.scope
            })
        {
            return Err(Failure::new(
                ErrorCode::PermissionDenied,
                "Reply belongs to another service source",
            ));
        }
        self.plugin_services
            .broker
            .lock()
            .unwrap()
            .complete_deferred(incoming.call, result)?;
        self.plugin_services.incoming.remove(&request.resource);
        self.plugin_services.resources.remove(&request.resource);
        self.roots.remove(&request);
        Ok(Value::Unit)
    }
}

impl Instance {
    /// Invoke once under source authority, retaining a bounded slot only when the provider defers.
    pub(crate) fn invoke_service(
        &mut self,
        call: &Call,
    ) -> Result<Option<serde_json::Value>, Failure> {
        let mut native_context = call.context.clone();
        native_context.native_waits.push(call.completion.clone());
        let reply = {
            let state = self.store.data_mut();
            if !state
                .api
                .capabilities
                .get("plugin.services")
                .is_some_and(|version| *version >= semver::Version::new(1, 1, 0))
                && !(call
                    .reference
                    .contract
                    .starts_with(plugin_protocol::commands::CONTRACT_PREFIX)
                    && state.api.capabilities.contains_key("plugin.commands"))
            {
                return Err(Failure::new(
                    ErrorCode::CapabilityUnavailable,
                    "Current service callbacks require plugin.services 1.1",
                ));
            }
            if state.plugin_services.incoming.len() >= 32 {
                return Err(Failure::new(
                    ErrorCode::LimitExceeded,
                    "Deferred invocation quota exceeded",
                ));
            }
            let Value::Resource(handle) = state.roots.open(RootKind::ServiceInvocation)? else {
                unreachable!()
            };
            state.plugin_services.incoming.insert(
                handle.resource,
                Incoming {
                    handle: handle.clone(),
                    call: call.clone(),
                },
            );
            state
                .plugin_services
                .resources
                .insert(handle.resource, (handle.clone(), native_context.clone()));
            state.plugin_services.invoking = true;
            state.plugin_services.context = Some(native_context);
            handle
        };
        let mut caller = call.context.caller.clone();
        caller.permissions = call.context.permissions.clone();
        let event = if call
            .reference
            .contract
            .starts_with(plugin_protocol::commands::CONTRACT_PREFIX)
        {
            api::Notification::CommandInvocation(plugin_protocol::commands::Invocation {
                id: call.reference.contract.rsplit('/').next().unwrap().into(),
                arguments: call.arguments.clone(),
                context: call.context.menu.clone(),
                caller,
                reply: reply.clone(),
            })
        } else {
            api::Notification::Service(service::Notification::Invoke(service::Invocation {
                caller,
                contract: call.reference.contract.clone(),
                method: call.method.clone(),
                arguments: call.arguments.clone(),
                reply: Some(reply.clone()),
            }))
        };
        let result = self.call(api::Input::Event { panel: None, event });
        // Restore private authority even if the guest traps. Returning None preserves the original
        // completion gate; its deadline and source chain continue to govern the deferred reply.
        self.store.data_mut().plugin_services.context = None;
        self.store.data_mut().plugin_services.invoking = false;
        let deferred = result
            .as_ref()
            .is_ok_and(|output| output.service_reply.is_none());
        if !deferred {
            let state = self.store.data_mut();
            state.plugin_services.incoming.remove(&reply.resource);
            state.plugin_services.resources.remove(&reply.resource);
            state.roots.remove(&reply);
        }
        match result {
            Ok(output) => output.service_reply.transpose(),
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

    /// Expired/cancelled invocations relinquish quota before informing the guest's local bookkeeping.
    pub(super) fn poll_service_replies(&mut self) -> anyhow::Result<()> {
        let ended = self
            .store
            .data()
            .plugin_services
            .incoming
            .values()
            .filter_map(|incoming| {
                let reason = match incoming.call.completion.status() {
                    api::RequestUpdate::Cancelled { reason, .. } => {
                        Failure::new(reason, "Deferred invocation ended")
                    }
                    api::RequestUpdate::Completed { .. } => {
                        Failure::new(ErrorCode::InvalidHandle, "Deferred invocation completed")
                    }
                    _ => return None,
                };
                Some((
                    incoming.handle.clone(),
                    incoming.call.context.clone(),
                    reason,
                    incoming
                        .call
                        .reference
                        .contract
                        .starts_with(plugin_protocol::commands::CONTRACT_PREFIX),
                ))
            })
            .collect::<Vec<_>>();
        for (request, context, reason, command) in ended {
            let state = self.store.data_mut();
            state.plugin_services.incoming.remove(&request.resource);
            state.plugin_services.resources.remove(&request.resource);
            state.roots.remove(&request);
            // A living source may handle cancellation; retired sources receive no further effects.
            self.call_for_retirement(
                context,
                api::Input::Event {
                    panel: None,
                    event: if command {
                        api::Notification::CommandCancelled { request, reason }
                    } else {
                        api::Notification::Service(service::Notification::InvocationCancelled {
                            request,
                            reason,
                        })
                    },
                },
            )?;
        }
        Ok(())
    }
}
