//! Source-owned bounded subscriptions and asynchronous forwarding to a pinned execution provider.
use super::*;

/// One observer cursor belongs to its original caller, independently of selected UI configuration.
pub(super) struct Subscription {
    pub origin: CallContext,
    pub session: u64,
    pub cursor: u64,
    pub pending: Option<u64>,
}
/// A host gateway result waits on the actual provider reply without blocking the actor.
pub(super) struct Forwarded {
    pub call: Call,
    pub session: u64,
    pub completion: Completion<Value>,
    pub subscription: Option<u64>,
}

/// Additional session.host 2.0 methods have exact public shapes and operation-specific authority.
pub(super) fn methods() -> BTreeMap<String, plugin_protocol::service::Method> {
    use serde_json::json;
    let session = json!({"type":"string","max_bytes":128});
    let state = json!({"type":"string","max_bytes":32});
    let result = json!({"type":"record","fields":{"session":session,"state":state}});
    let sub = json!({"type":"string","max_bytes":128});
    let events =
        json!({"type":"array","max_items":16,"items":plugin_protocol::execution::event_schema()});
    serde_json::from_value(json!({
        "input":{"parameters":{"type":"record","fields":{"session":session,"bytes":{"type":"array","max_items":1024,"items":{"type":"integer","min":0,"max":255}}}},"result":result,"permissions":["process.exec"]},
        "locate":{"parameters":{"type":"record","fields":{"session":session}},"result":result,"permissions":["ui.panels"]},
        "subscribe":{"parameters":{"type":"record","fields":{"session":session}},"result":{"type":"record","fields":{"subscription":sub,"session":session,"state":state}}},
        "next":{"parameters":{"type":"record","fields":{"subscription":sub,"limit":{"type":"integer","min":1,"max":16}}},
            "result":{"type":"record","fields":{"subscription":sub,"session":session,"state":state,"cursor":{"type":"integer","min":0,"max":i64::MAX},"gap":{"type":"boolean"},"events":events}}},
        "unsubscribe":{"parameters":{"type":"record","fields":{"subscription":sub}},"result":{"type":"record","fields":{"subscription":sub}}}
    })).expect("fixed session subscription declarations are valid")
}

impl Manager {
    /// Subscribe to retained history and future output; allocation itself grants no execution rights.
    pub(super) fn subscribe_execution(
        &mut self,
        context: &CallContext,
        session: u64,
    ) -> Result<Value, Failure> {
        let execution = self
            .host_sessions
            .get(session)
            .filter(|entry| entry.visible_to(&context.caller))
            .ok_or_else(|| Failure::new(ErrorCode::InvalidHandle, "Unknown or foreign session"))?;
        self.retire_session_subscriptions();
        let owned = self
            .host_sessions
            .subscriptions
            .values()
            .filter(|entry| entry.origin.caller.instance == context.caller.instance)
            .count();
        if owned >= 32 || self.host_sessions.subscriptions.len() >= 128 {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Session subscription quota exceeded",
            ));
        }
        let id = self.host_sessions.next_subscription;
        self.host_sessions.next_subscription = id.checked_add(1).ok_or_else(|| {
            Failure::new(
                ErrorCode::LimitExceeded,
                "Subscription identities exhausted",
            )
        })?;
        self.host_sessions.subscriptions.insert(
            id,
            Subscription {
                origin: context.clone(),
                session,
                cursor: 0,
                pending: None,
            },
        );
        Ok(
            serde_json::json!({"subscription":id.to_string(),"session":session.to_string(),"state":execution.snapshot().state.as_str()}),
        )
    }

    /// Caller scope and incarnation are checked on every cursor operation, including unsubscribe.
    fn owned_subscription(&self, context: &CallContext, arguments: &Value) -> Result<u64, Failure> {
        let id = arguments["subscription"]
            .as_str()
            .and_then(|id| id.parse::<u64>().ok())
            .ok_or_else(|| {
                Failure::new(ErrorCode::InvalidRequest, "Expected subscription identity")
            })?;
        self.host_sessions
            .subscriptions
            .get(&id)
            .filter(|entry| {
                entry.origin.caller.instance == context.caller.instance
                    && entry.origin.caller.scope == context.caller.scope
                    && entry
                        .origin
                        .lifetimes
                        .iter()
                        .all(|alive| alive.load(Ordering::Acquire))
            })
            .map(|_| id)
            .ok_or_else(|| {
                Failure::new(ErrorCode::InvalidHandle, "Unknown or foreign subscription")
            })
    }

    /// Unsubscribing cancels an outstanding pull and releases only that observer, never its program.
    pub(super) fn unsubscribe_execution(
        &mut self,
        context: &CallContext,
        arguments: &Value,
    ) -> Result<Value, Failure> {
        let id = self.owned_subscription(context, arguments)?;
        if let Some(pending) = self
            .host_sessions
            .subscriptions
            .remove(&id)
            .and_then(|entry| entry.pending)
        {
            if let Some(operation) = self.host_sessions.operations.remove(&pending) {
                operation.completion.retire();
                operation.call.completion.finish(Err(Failure::new(
                    ErrorCode::Cancelled,
                    "Observer unsubscribed",
                )));
            }
        }
        Ok(serde_json::json!({"subscription":id.to_string()}))
    }

    /// Native clients select the provider's own retained view by its pinned session identity.
    pub fn locate_execution(&mut self, session: u64) -> anyhow::Result<Completion<Value>> {
        self.execution_operation(session, "locate", serde_json::json!({}), 15000, None)
            .map_err(start_failure)
    }

    /// All operations retain the original resource owner and provider; later preferences cannot retarget them.
    pub(in crate::manager) fn execution_operation(
        &mut self,
        session: u64,
        method: &str,
        mut arguments: Value,
        timeout: u32,
        invocation: Option<&Call>,
    ) -> Result<Completion<Value>, Failure> {
        let execution = self
            .execution(session)
            .ok_or_else(|| Failure::new(ErrorCode::InvalidHandle, "Unknown execution"))?;
        let snapshot = execution.snapshot();
        if !execution.provider_active() {
            return Err(Failure::new(
                ErrorCode::InvalidHandle,
                "Execution provider retired",
            ));
        }
        let identity = snapshot.provider_session.ok_or_else(|| {
            Failure::new(
                ErrorCode::InvalidState,
                "Program creation has no receipt yet",
            )
        })?;
        let mut dependency = execution_dependency()?;
        if method == "resize" {
            // Geometry is additive in 2.1: only callers that need it require the exact extra shape.
            dependency
                .methods
                .insert("resize".into(), plugin_protocol::execution::resize_method());
        }
        self.refresh_services();
        let reference = self.plugin_services.lock().unwrap().resolve_pinned(
            &execution.origin.caller,
            EXECUTION_CONTRACT,
            &dependency,
            execution.provider_instance(),
        )?;
        arguments["session"] = serde_json::json!(identity);
        let mut context = execution.origin.clone();
        if let Some(invocation) = invocation {
            // Resource ownership stays with the launch. Every intermediary of this new operation
            // and its cancellation gate must still be alive when the provider enters the callback.
            context
                .lifetimes
                .extend(invocation.context.lifetimes.iter().cloned());
            context
                .lifetimes
                .push(invocation.completion.wait_lifetime());
            for participant in &invocation.context.ancestry {
                if !context.ancestry.contains(participant) {
                    context.ancestry.push(participant.clone());
                }
            }
        }
        let mut completion = Completion::new(timeout);
        if let Some(invocation) = invocation {
            completion.constrain_deadline(&invocation.completion);
        }
        completion.lifetimes = context.lifetimes.clone();
        let mut call = host_method_call(
            &execution.origin.caller,
            reference,
            method,
            arguments,
            &dependency,
            completion.clone(),
            self.host_alive.clone(),
        )?;
        call.context = context.delegate(&call.reference.provider, &call.signature)?;
        call.completion.lifetimes = call.context.lifetimes.clone();
        self.plugin_services.lock().unwrap().enqueue(call)?;
        Ok(completion)
    }

    /// Forwarding returns admission only; the outer service task completes from a later actor tick.
    pub(crate) fn forward_session_operation(&mut self, call: &Call) -> Result<(), Failure> {
        if call.context.caller.scope != self.host_scope()
            || !call
                .context
                .lifetimes
                .iter()
                .all(|alive| alive.load(Ordering::Acquire))
        {
            return Err(Failure::new(
                ErrorCode::InvalidHandle,
                "Session source retired or changed scope",
            ));
        }
        if self.host_sessions.operations.len() >= 128 {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Session operation quota exceeded",
            ));
        }
        let (session, method, arguments, subscription) = if call.method == "next" {
            let id = self.owned_subscription(&call.context, &call.arguments)?;
            let entry = &self.host_sessions.subscriptions[&id];
            if entry.pending.is_some() {
                return Err(Failure::new(
                    ErrorCode::Conflict,
                    "A subscription pull is already pending",
                ));
            }
            (
                entry.session,
                "events",
                serde_json::json!({"after":entry.cursor,"limit":call.arguments["limit"]}),
                Some(id),
            )
        } else {
            let session = call.arguments["session"]
                .as_str()
                .and_then(|id| id.parse::<u64>().ok())
                .ok_or_else(|| {
                    Failure::new(ErrorCode::InvalidRequest, "Expected session identity")
                })?;
            let execution = self
                .host_sessions
                .get(session)
                .filter(|entry| entry.visible_to(&call.context.caller))
                .ok_or_else(|| {
                    Failure::new(ErrorCode::InvalidHandle, "Unknown or foreign session")
                })?;
            let permission = if call.method == "input" {
                "process.exec"
            } else {
                "ui.panels"
            };
            if !call.context.permissions.contains(permission) {
                return Err(Failure::new(
                    ErrorCode::PermissionDenied,
                    "Session operation requires delegated authority",
                ));
            }
            if call.method == "input" && !execution.snapshot().state.is_active() {
                return Err(Failure::new(ErrorCode::InvalidState, "Program ended"));
            }
            let mut arguments = call.arguments.clone();
            arguments.as_object_mut().unwrap().remove("session");
            (session, call.method.as_str(), arguments, None)
        };
        let id = self.host_sessions.next_operation;
        let next_id = id.checked_add(1).ok_or_else(|| {
            Failure::new(
                ErrorCode::LimitExceeded,
                "Session operation identities exhausted",
            )
        })?;
        let completion = self.execution_operation(session, method, arguments, 5000, Some(call))?;
        self.host_sessions.next_operation = next_id;
        self.host_sessions.operations.insert(
            id,
            Forwarded {
                call: call.clone(),
                session,
                completion,
                subscription,
            },
        );
        if let Some(subscription) = subscription {
            self.host_sessions
                .subscriptions
                .get_mut(&subscription)
                .unwrap()
                .pending = Some(id);
        }
        Ok(())
    }

    /// Removing a dead source seals its cursors before any late completion can publish output.
    fn retire_session_subscriptions(&mut self) {
        let entries = &self.host_sessions.entries;
        self.host_sessions.subscriptions.retain(|_, entry| {
            entry
                .origin
                .lifetimes
                .iter()
                .all(|alive| alive.load(Ordering::Acquire))
                && entries
                    .get(&entry.session)
                    .is_some_and(|execution| execution.provider_active())
        });
    }

    /// Completed replies advance a cursor once; cancellation never consumes unread output.
    pub(super) fn poll_session_operations(&mut self) {
        self.retire_session_subscriptions();
        let operations = std::mem::take(&mut self.host_sessions.operations);
        for (id, operation) in operations {
            if operation.call.completion.status().is_terminal() {
                operation.completion.retire();
                self.release_subscription_pull(operation.subscription, id);
                continue;
            }
            let result = match operation.completion.status() {
                RequestUpdate::Accepted | RequestUpdate::Progress { .. } => {
                    self.host_sessions.operations.insert(id, operation);
                    continue;
                }
                RequestUpdate::Completed { result } => result,
                RequestUpdate::Cancelled { reason, .. } => {
                    Err(Failure::new(reason, "Execution operation ended"))
                }
            };
            let result = result.and_then(|mut value| {
                let execution = self
                    .host_sessions
                    .get(operation.session)
                    .filter(|entry| entry.visible_to(&operation.call.context.caller))
                    .ok_or_else(|| Failure::new(ErrorCode::InvalidHandle, "Session retired"))?;
                if value["session"].as_str() != execution.snapshot().provider_session.as_deref() {
                    return Err(Failure::new(
                        ErrorCode::InvalidRequest,
                        "Provider replied for another session",
                    ));
                }
                value["session"] = serde_json::json!(operation.session.to_string());
                if let Some(subscription) = operation.subscription {
                    let entry = self
                        .host_sessions
                        .subscriptions
                        .get(&subscription)
                        .ok_or_else(|| {
                            Failure::new(ErrorCode::InvalidHandle, "Subscription retired")
                        })?;
                    let cursor = value["cursor"].as_u64().ok_or_else(|| {
                        Failure::new(ErrorCode::InvalidRequest, "Missing observation cursor")
                    })?;
                    let mut last = entry.cursor;
                    for event in value["events"].as_array().ok_or_else(|| {
                        Failure::new(ErrorCode::InvalidRequest, "Missing observations")
                    })? {
                        let sequence = event["sequence"].as_u64().ok_or_else(|| {
                            Failure::new(ErrorCode::InvalidRequest, "Missing event sequence")
                        })?;
                        if sequence <= last || sequence > cursor {
                            return Err(Failure::new(
                                ErrorCode::InvalidRequest,
                                "Out-of-order execution observations",
                            ));
                        }
                        last = sequence;
                    }
                    if cursor != last {
                        return Err(Failure::new(
                            ErrorCode::InvalidRequest,
                            "Cursor did not match published observations",
                        ));
                    }
                    value["subscription"] = serde_json::json!(subscription.to_string());
                    value["state"] = serde_json::json!(execution.snapshot().state.as_str());
                }
                operation.call.signature.result.accepts(&value)?;
                Ok(value)
            });
            let cursor = result
                .as_ref()
                .ok()
                .and_then(|value| value["cursor"].as_u64());
            // The provider may evict completed history. Expiry releases its observer rather than
            // keeping an unusable cursor in the source's subscription quota indefinitely.
            if result
                .as_ref()
                .is_err_and(|error| error.code == ErrorCode::InvalidHandle)
            {
                if let Some(subscription) = operation.subscription {
                    self.host_sessions.subscriptions.remove(&subscription);
                }
            }
            operation.call.completion.finish(result);
            if matches!(
                operation.call.completion.status(),
                RequestUpdate::Completed { result: Ok(_) }
            ) {
                if let (Some(subscription), Some(cursor)) = (operation.subscription, cursor) {
                    if let Some(entry) = self.host_sessions.subscriptions.get_mut(&subscription) {
                        entry.cursor = cursor;
                    }
                }
            }
            self.release_subscription_pull(operation.subscription, id);
        }
    }
    fn release_subscription_pull(&mut self, subscription: Option<u64>, operation: u64) {
        if let Some(entry) =
            subscription.and_then(|id| self.host_sessions.subscriptions.get_mut(&id))
        {
            if entry.pending == Some(operation) {
                entry.pending = None;
            }
        }
    }
}
