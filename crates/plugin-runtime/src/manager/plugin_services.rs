//! Host-owned service provider selection and bounded routing across active logical scopes.
use super::*;
use crate::plugin_services::{Call, Preferences};
use plugin_protocol::service::Choice;
use plugin_protocol::settings::Scope;

impl Manager {
    /// Retry only unavailable dependencies after another package starts; missing/cyclic graphs terminate.
    pub(super) fn activate_saved_plugins(&mut self, mut pending: Vec<(String, bool)>) {
        while !pending.is_empty() {
            let mut retry = Vec::new();
            let mut progress = false;
            for (id, enabled) in pending {
                if self.live.contains_key(&id) {
                    continue;
                }
                let result = self.enable(&id);
                let entry = self.installed.get_mut(&id).unwrap();
                // Activation restores persisted defaults even for project-only instances.
                entry.enabled = enabled;
                match result {
                    Ok(()) => progress = true,
                    Err(error) => {
                        entry.error = Some(format!("{error:#}"));
                        if error.downcast_ref::<api::Failure>().is_some_and(|failure| {
                            matches!(
                                failure.code,
                                api::ErrorCode::CapabilityUnavailable | api::ErrorCode::Conflict
                            )
                        }) {
                            retry.push((id, enabled));
                        }
                    }
                }
            }
            if !progress {
                break;
            }
            pending = retry;
        }
    }
    /// Registry snapshots include exact live incarnations, including parked workspaces and consumers.
    ///
    /// The host is published beside them so a consumer's selection and this registry agree: the
    /// session contract has exactly one provider per workspace, and it is the runtime that owns the
    /// table those sessions live in.
    pub(super) fn refresh_services(&mut self) {
        let providers = self
            .live
            .values()
            .chain(self.parked.values().flat_map(|scope| scope.live.values()))
            .filter_map(Instance::service_provider)
            .collect();
        let scope = self.host_scope().to_owned();
        let host = vec![super::host_services::session_provider(
            &scope,
            self.host_alive.clone(),
        )];
        self.plugin_services
            .lock()
            .unwrap()
            .reconcile(providers, host);
        for instance in self.live.values_mut().chain(
            self.parked
                .values_mut()
                .flat_map(|scope| scope.live.values_mut()),
        ) {
            instance.retire_service_sources();
        }
    }
    pub fn service_choices(&mut self) -> Vec<Choice> {
        self.refresh_services();
        let broker = self.plugin_services.lock().unwrap();
        let mut choices = broker.choices(&scopes::workspace_key(&self.environment.workspace));
        choices.extend(broker.choices("application"));
        choices
    }
    /// A confirmed host selection is persisted outside project files and cannot grant execution rights.
    pub fn set_service_provider(
        &mut self,
        owner: api::InstanceScope,
        scope: Scope,
        contract: &str,
        provider: Option<&str>,
    ) -> anyhow::Result<()> {
        let choices = self.service_choices();
        let choice = choices
            .iter()
            .find(|choice| choice.contract == contract && choice.scope == owner)
            .ok_or_else(|| anyhow::anyhow!("Unknown service contract"))?;
        anyhow::ensure!(
            provider.is_none_or(|id| choice.candidates.iter().any(|candidate| candidate == id)),
            "Provider does not offer this service"
        );
        let mut broker = self.plugin_services.lock().unwrap();
        let mut preferences = broker.preferences.clone();
        anyhow::ensure!(
            owner != api::InstanceScope::Application || scope == Scope::User,
            "Application services have no project overrides"
        );
        let namespace = if owner == api::InstanceScope::Application {
            "application".into()
        } else {
            scopes::workspace_key(&self.environment.workspace)
        };
        let target = match scope {
            Scope::User => &mut preferences.user,
            Scope::Project => preferences
                .projects
                .entry(scopes::workspace_key(&self.environment.workspace))
                .or_default(),
        };
        let key = if scope == Scope::User {
            crate::plugin_services::choice_key(&namespace, contract)
        } else {
            contract.into()
        };
        if let Some(provider) = provider {
            target.insert(key, provider.into());
        } else {
            target.remove(&key);
        }
        let bytes = serde_json::to_vec_pretty(&preferences)?;
        anyhow::ensure!(
            bytes.len() <= 1024 * 1024,
            "Service provider preferences exceed quota"
        );
        atomic_write(&self.root.join("service-providers.json"), &bytes)?;
        broker.preferences = preferences;
        broker.refresh_selections();
        Ok(())
    }
    /// No service callback can recursively invoke another store; callbacks enqueue their next hop.
    pub(super) fn route_services(&mut self) {
        self.refresh_services();
        for (call, result) in self.plugin_services.lock().unwrap().take_completed() {
            match result {
                Ok(value) => self.host_sessions.observe_status(&call, &value),
                Err(error) => self.host_sessions.observe_error(&call, &error),
            }
        }
        let calls = self.plugin_services.lock().unwrap().take_batch();
        for call in calls {
            self.refresh_services();
            if !call.completion.begin() {
                continue;
            }
            let valid = self
                .plugin_services
                .lock()
                .unwrap()
                .validate_reference(&call.context.caller, &call.reference);
            if let Err(error) = valid {
                call.completion.finish(Err(error));
                continue;
            }
            if !call.completion.enter_side_effect() {
                continue;
            }
            let result = self.dispatch_service(&call).and_then(|value| {
                let Some(value) = value else {
                    return Ok(None);
                };
                if serde_json::to_vec(&value).map_or(true, |bytes| bytes.len() > 65536) {
                    return Err(api::Failure::new(
                        api::ErrorCode::LimitExceeded,
                        "Service result exceeds 64 KiB",
                    ));
                }
                call.signature.result.accepts(&value)?;
                Ok(Some(value))
            });
            match &result {
                Ok(Some(value)) => self.host_sessions.observe_status(&call, value),
                Ok(None) => continue,
                Err(failure) => self.host_sessions.observe_error(&call, failure),
            }
            call.completion
                .finish(result.map(|value| value.expect("deferred calls continue above")));
        }
    }
    fn dispatch_service(&mut self, call: &Call) -> Result<Option<serde_json::Value>, api::Failure> {
        // The host is a participant like any provider, so a call addressed to its session contract is
        // answered here instead of being searched for among the running instances. The identity is
        // compared exactly, so a plugin cannot reach the host's session surface by naming it.
        let host = self.host_scope().to_owned();
        if call.reference.provider.caller.instance
            == super::host_services::host_caller(&host).instance
        {
            if call.reference.provider.caller.plugin != "me-editor" {
                return Err(api::Failure::new(
                    api::ErrorCode::InvalidHandle,
                    "Service provider exited",
                ));
            }
            if matches!(call.method.as_str(), "input" | "locate" | "next") {
                self.forward_session_operation(call)?;
                return Ok(None);
            }
            return super::host_services::session_answer(
                self,
                &call.context,
                &call.method,
                &call.arguments,
            )
            .map(Some);
        }
        let provider = self
            .live
            .values_mut()
            .chain(
                self.parked
                    .values_mut()
                    .flat_map(|scope| scope.live.values_mut()),
            )
            .find(|instance| {
                instance.service_provider().is_some_and(|provider| {
                    provider.caller.instance == call.reference.provider.caller.instance
                })
            });
        provider
            .ok_or_else(|| {
                api::Failure::new(api::ErrorCode::InvalidHandle, "Service provider exited")
            })?
            .invoke_service(call)
    }
}

/// Missing local preferences start with no arbitrary choice; malformed files fail explicitly.
pub(super) fn read_preferences(root: &Path) -> anyhow::Result<Preferences> {
    let path = root.join("service-providers.json");
    if !path.exists() {
        return Ok(Preferences::default());
    }
    anyhow::ensure!(
        path.metadata()?.len() <= 1024 * 1024,
        "Service provider preferences exceed quota"
    );
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request_state::Completion;
    use plugin_protocol::{
        api::{ErrorCode, RequestUpdate},
        service::Caller,
    };

    /// A consumer's call on the session contract is answered by the host, through the real queue.
    ///
    /// This is the route, not the rule: a call is resolved, enqueued and dispatched exactly as a
    /// plugin's `service::guest::Task` would be, and the answer must therefore come from the host's
    /// own session table rather than from any running instance. What it deliberately does not do is
    /// fake a guest: the caller here stands in for a consumer, so what is under test is the host's
    /// side of the seam.
    #[test]
    fn a_session_call_reaches_the_hosts_own_table() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let mut manager = Manager::open(
            root.path().join("plugins"),
            plugin_protocol::Environment {
                workspace: workspace.display().to_string(),
                os: "windows".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let scope = manager.host_scope().to_owned();
        let consumer = Caller {
            plugin: "consumer".into(),
            instance: "consumer@1".into(),
            scope: scope.clone(),
            permissions: Default::default(),
        };
        let dependency = crate::manager::host_services::session_dependency().unwrap();
        manager.refresh_services();
        // Resolution finds the host itself as the session contract's provider for this workspace.
        let reference = manager
            .plugin_services
            .lock()
            .unwrap()
            .resolve(
                &consumer,
                crate::manager::host_services::SESSION_CONTRACT,
                &dependency,
            )
            .expect("the host offers the session contract in its own workspace");
        assert_eq!(
            reference.provider.caller.plugin, "me-editor",
            "the provider a consumer reaches is the host, not a guest"
        );
        // A list is the least eventful call: it must answer with the host's table, which is empty.
        let completion = Completion::new(5_000);
        let signature = dependency.methods["list"].clone();
        let call = Call {
            handle: plugin_protocol::api::ResourceHandle {
                instance: reference.provider.caller.instance.clone(),
                scope: scope.clone(),
                resource: 0,
            },
            reference,
            method: "list".into(),
            signature,
            arguments: serde_json::json!({}),
            context: crate::plugin_services::Context {
                native_waits: Vec::new(),
                menu: None,
                origin: crate::plugin_services::InvocationOrigin::Delegated,
                lifetimes: vec![manager.host_alive.clone()],
                caller: consumer,
                ancestry: Vec::new(),
                permissions: Default::default(),
            },
            completion: completion.clone(),
        };
        manager
            .plugin_services
            .lock()
            .unwrap()
            .enqueue(call)
            .expect("the call is queued");
        manager.route_services();
        match completion.status() {
            RequestUpdate::Completed { result } => {
                let value = result.expect("the host answered");
                assert_eq!(
                    value["sessions"].as_array().map(Vec::len),
                    Some(0),
                    "the answer is the host's own session table: {value}"
                );
            }
            other => panic!("the host did not answer through the queue: {other:?}"),
        }
        manager.shutdown();
    }
}
