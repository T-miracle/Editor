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
                if serde_json::to_vec(&value).map_or(true, |bytes| bytes.len() > 65536) {
                    return Err(api::Failure::new(
                        api::ErrorCode::LimitExceeded,
                        "Service result exceeds 64 KiB",
                    ));
                }
                call.signature.result.accepts(&value)?;
                Ok(value)
            });
            call.completion.finish(result);
        }
    }
    fn dispatch_service(&mut self, call: &Call) -> Result<serde_json::Value, api::Failure> {
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
