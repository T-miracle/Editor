//! Dynamic discovery/build requests pin provider instances and independent preparation resource roots.
use super::{
    Manager,
    host_services::{host_caller, host_method_call, start_failure},
};
use crate::{Completion, plugin_services::Context};
use plugin_protocol::{api::RequestUpdate, service::Dependency, targets};
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// One host-owned wait. Cancelling a preparation revokes its own native work, never unrelated targets.
pub struct TargetRequest {
    completion: Completion<Value>,
    alive: Arc<AtomicBool>,
    native: Arc<std::sync::Mutex<crate::native_work::NativeWork>>,
    provider: String,
    instance: String,
    scope: String,
}
impl TargetRequest {
    /// Final receipts and deadlines release the preparation root; the final executable is not started.
    pub fn status(&self) -> RequestUpdate<Value> {
        let mut status = self.completion.status();
        let native = self.native.lock().unwrap();
        if let Some(message) = native.failure() {
            self.alive.store(false, Ordering::Release);
            self.completion.retire();
            return RequestUpdate::Completed {
                result: Err(plugin_protocol::api::Failure::new(
                    plugin_protocol::api::ErrorCode::OperationFailed,
                    message,
                )),
            };
        }
        if native.stopped() && native.drained() {
            self.alive.store(false, Ordering::Release);
            self.completion.retire();
            status = self.completion.status();
        } else if status.is_terminal() {
            self.alive.store(false, Ordering::Release);
            // Do not publish an artifact/control receipt while owned native work still needs cleanup.
            if !native.drained() {
                return RequestUpdate::Accepted;
            }
        }
        status
    }
    /// Caller identity remains valid only in the original workspace and provider incarnation.
    pub fn valid_for(&self, manager: &Manager) -> bool {
        manager.trusted
            && manager.workspace_open
            && manager.host_scope() == self.scope
            && manager
                .live
                .get(&self.provider)
                .is_some_and(|instance| instance.instance_id() == Some(self.instance.as_str()))
    }
    /// Observe this invocation's bounded native history independently from another configuration.
    pub fn snapshot(&self) -> crate::PreparationSnapshot {
        let mut snapshot = self.native.lock().unwrap().snapshot();
        snapshot.provider = self.provider.clone();
        snapshot
    }
    /// Seal new effects, request supported normal exit, then force after the same bounded grace as Run.
    pub fn stop_with(&self, mode: plugin_protocol::process::ExitMode) {
        self.native.lock().unwrap().stop(mode);
        if self.native.lock().unwrap().drained() {
            self.alive.store(false, Ordering::Release);
            self.completion.retire();
        }
    }
    /// Immediate abandonment revokes all resources; explicit normal Stop uses stop_with instead.
    pub fn stop(&self) {
        self.alive.store(false, Ordering::Release);
        self.completion.retire();
    }
}
impl Drop for TargetRequest {
    /// Abandoned waits own cleanup; their external side effects are never claimed to roll back.
    fn drop(&mut self) {
        self.stop();
    }
}
impl Manager {
    /// Validate any embedded native document against the actual provider's negotiated public capabilities.
    /// This generic admission gate is reusable by host-owned surfaces and never grants additional access.
    pub fn validate_native_document(
        &self,
        provider: &str,
        document: &plugin_protocol::ui::Document,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.live.contains_key(provider),
            "Native UI provider is unavailable"
        );
        let entry = self
            .installed
            .get(provider)
            .ok_or_else(|| anyhow::anyhow!("Unknown native UI provider"))?;
        let negotiated = crate::capabilities::negotiate(&entry.manifest)?;
        document.validate().map_err(anyhow::Error::msg)?;
        for (name, version) in document.required_capabilities() {
            anyhow::ensure!(
                negotiated
                    .capabilities
                    .get(name)
                    .is_some_and(|actual| *actual >= version),
                "{name} was not negotiated for the native document"
            );
        }
        Ok(())
    }
    /// List compatible live contributors without selecting a default or starting project tools.
    pub fn target_providers(&self) -> Vec<String> {
        let contract = targets::declaration();
        let dependency = Dependency {
            version: ">=1.0,<2".parse().unwrap(),
            optional: false,
            methods: contract.methods,
        };
        self.live
            .keys()
            .filter(|id| {
                self.installed
                    .get(*id)
                    .and_then(|entry| {
                        entry
                            .manifest
                            .plugin_services
                            .provides
                            .get(targets::CONTRACT)
                    })
                    .is_some_and(|contract| dependency.matches(contract))
            })
            .cloned()
            .collect()
    }
    /// Begin only a known method on an explicit contributor. Schema, grants and trust precede effects.
    pub fn begin_target_call(
        &mut self,
        provider: &str,
        method: &str,
        arguments: Value,
    ) -> anyhow::Result<TargetRequest> {
        anyhow::ensure!(
            self.trusted && self.workspace_open,
            "Restricted/closed workspace cannot prepare targets"
        );
        anyhow::ensure!(
            matches!(method, "discover" | "prepare"),
            "Unknown target method"
        );
        self.begin_configuration_service(
            provider,
            targets::CONTRACT,
            targets::declaration(),
            method,
            arguments,
        )
    }

    /// All compatible live plugins can supply command templates, forms and validation.
    pub fn configuration_providers(&self) -> Vec<String> {
        let contract = plugin_protocol::configurations::declaration();
        let dependency = Dependency {
            version: ">=1.0,<2".parse().unwrap(),
            optional: false,
            methods: contract.methods,
        };
        self.live
            .keys()
            .filter(|id| {
                self.installed
                    .get(*id)
                    .and_then(|entry| {
                        entry
                            .manifest
                            .plugin_services
                            .provides
                            .get(plugin_protocol::configurations::CONTRACT)
                    })
                    .is_some_and(|contract| dependency.matches(contract))
            })
            .cloned()
            .collect()
    }

    /// Pin one configuration call to its original live instance and workspace with bounded resources.
    /// Permission or schema failures are returned before any guest callback or native process starts.
    pub fn begin_configuration_call(
        &mut self,
        provider: &str,
        method: &str,
        arguments: Value,
    ) -> anyhow::Result<TargetRequest> {
        self.begin_configuration_service(
            provider,
            plugin_protocol::configurations::CONTRACT,
            plugin_protocol::configurations::declaration(),
            method,
            arguments,
        )
    }

    /// Existing target and configuration consumers share instance pinning and native cleanup roots.
    fn begin_configuration_service(
        &mut self,
        provider: &str,
        name: &str,
        contract: plugin_protocol::service::Contract,
        method: &str,
        arguments: Value,
    ) -> anyhow::Result<TargetRequest> {
        anyhow::ensure!(
            self.trusted && self.workspace_open,
            "Restricted/closed workspace cannot configure targets"
        );
        anyhow::ensure!(
            contract.methods.contains_key(method),
            "Unknown configuration method"
        );
        let dependency = Dependency {
            version: ">=1.0,<2".parse().unwrap(),
            optional: false,
            methods: contract.methods,
        };
        let scope = self.host_scope();
        let caller = host_caller(&scope);
        self.refresh_services();
        let instance = self
            .live
            .get(provider)
            .and_then(|instance| instance.instance_id())
            .ok_or_else(|| anyhow::anyhow!("Target provider unavailable; reinstall or enable it"))?
            .to_owned();
        let reference = self
            .plugin_services
            .lock()
            .unwrap()
            .resolve_pinned(&caller, name, &dependency, &instance)
            .map_err(start_failure)?;
        let alive = Arc::new(AtomicBool::new(true));
        let native = Arc::new(std::sync::Mutex::new(
            crate::native_work::NativeWork::default(),
        ));
        self.host_resources.preparations.register(&alive, &native);
        let origin = Context {
            caller: caller.clone(),
            permissions: caller.permissions.clone(),
            ancestry: vec![],
            lifetimes: vec![self.host_alive.clone(), alive.clone()],
        };
        let mut completion = Completion::new(if method == "prepare" { 120_000 } else { 30_000 });
        completion.lifetimes = origin.lifetimes.clone();
        let owner = reference.provider.clone();
        let mut call = host_method_call(
            &caller,
            reference,
            method,
            arguments,
            &dependency,
            completion.clone(),
            self.host_alive.clone(),
        )
        .map_err(start_failure)?;
        call.context = origin
            .delegate(&owner, &call.signature)
            .map_err(start_failure)?;
        self.plugin_services
            .lock()
            .unwrap()
            .enqueue(call)
            .map_err(start_failure)?;
        Ok(TargetRequest {
            completion,
            alive,
            native,
            provider: provider.into(),
            instance,
            scope,
        })
    }
}
