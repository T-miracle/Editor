//! A bounded broker routes owned values; it never holds a WASM store or calls a guest under its lock.
use crate::request_state::Completion;
use plugin_protocol::{
    api::{ErrorCode, Failure, ResourceHandle},
    service::{Caller, Choice, Contract, Dependency, Method},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

pub(crate) type Shared = Arc<Mutex<Broker>>;

#[derive(Clone)]
pub(crate) struct Provider {
    pub alive: Arc<AtomicBool>,
    pub caller: Caller,
    pub contracts: BTreeMap<String, Contract>,
}
#[derive(Clone)]
pub(crate) struct Reference {
    pub provider: Provider,
    pub contract: String,
    pub dependency: Dependency,
    revision: u64,
    /// A live session keeps its provider after the user's choice changes for future launches.
    pinned: bool,
}
/// A selection revision never rolls back when a provider is removed and later selected again.
#[derive(Default)]
struct Selection {
    providers: Vec<String>,
    preference: Option<String>,
    revision: u64,
}
/// Only the Manager's typed command route creates a host origin; guest caller strings are descriptive.
#[derive(Clone)]
pub(crate) enum InvocationOrigin {
    Delegated,
    HostCommand { target: String },
}

/// The original source and shrinking authority survive nested calls; ancestry bounds asynchronous cycles.
#[derive(Clone)]
pub(crate) struct Context {
    pub lifetimes: Vec<Arc<AtomicBool>>,
    /// Pending native interactions observe source waits without revoking already owned programs.
    pub native_waits: Vec<Completion<Value>>,
    /// A native menu target is descriptive context, separate from schema-checked arguments.
    pub menu: Option<plugin_protocol::commands::Context>,
    /// An internal admission record is never serialized to, or accepted from, a guest.
    pub origin: InvocationOrigin,
    pub caller: Caller,
    pub ancestry: Vec<String>,
    pub permissions: BTreeSet<String>,
}
impl Context {
    /// A directly invoked provider may use its own picker permission, never through another hop.
    pub(crate) fn direct_host_selection(&self, instance: &str) -> bool {
        matches!(&self.origin, InvocationOrigin::HostCommand { target } if target == instance)
            && self.ancestry.len() == 1
            && self.ancestry[0] == instance
            && self.permissions.contains("files.select")
            && self
                .lifetimes
                .iter()
                .all(|alive| alive.load(Ordering::Acquire))
            && self.native_waits.iter().all(|wait| {
                !matches!(
                    wait.status(),
                    plugin_protocol::api::RequestUpdate::Cancelled { .. }
                )
            })
    }

    /// Each hop keeps the original owner, shrinks authority and adds the target's revocation boundary.
    pub(crate) fn delegate(
        &self,
        provider: &Provider,
        signature: &Method,
    ) -> Result<Self, Failure> {
        if self.ancestry.len() >= 8 || self.ancestry.contains(&provider.caller.instance) {
            return Err(Failure::new(
                ErrorCode::Conflict,
                "Service call cycle or depth limit detected",
            ));
        }
        if !signature.permissions.is_subset(&self.permissions)
            || !signature
                .permissions
                .is_subset(&provider.caller.permissions)
        {
            return Err(Failure::new(
                ErrorCode::PermissionDenied,
                "Caller and provider must both authorize the service method",
            ));
        }
        let mut next = self.clone();
        next.permissions = signature.permissions.clone();
        next.ancestry.push(provider.caller.instance.clone());
        next.lifetimes.push(provider.alive.clone());
        Ok(next)
    }
}
#[derive(Clone)]
pub(crate) struct Call {
    pub handle: ResourceHandle,
    pub reference: Reference,
    pub method: String,
    pub signature: Method,
    pub arguments: Value,
    pub context: Context,
    pub completion: Completion<Value>,
}
pub(crate) struct Pending {
    pub return_context: Option<Context>,
    pub call: Call,
    pub reported: u64,
}

/// Only host UI writes choices; project files and plugin requests have no setter.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Preferences {
    pub user: BTreeMap<String, String>,
    pub projects: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Default)]
pub(crate) struct Broker {
    providers: BTreeMap<String, Provider>,
    queue: VecDeque<Call>,
    /// Deferred callbacks are observed by the manager on its next tick, outside the guest's lock.
    completed: VecDeque<(Call, Result<Value, Failure>)>,
    pub preferences: Preferences,
    selections: BTreeMap<(String, String), Selection>,
}
impl Broker {
    /// Discovery exposes only live typed command metadata in the caller's current scope.
    pub(crate) fn commands(&self, caller: &Caller) -> Vec<plugin_protocol::commands::Descriptor> {
        self.providers
            .values()
            .filter(|provider| {
                provider.alive.load(Ordering::Acquire) && provider.caller.scope == caller.scope
            })
            .flat_map(|provider| {
                provider.contracts.iter().filter_map(|(id, contract)| {
                    let command = id
                        .strip_prefix(plugin_protocol::commands::CONTRACT_PREFIX)?
                        .split_once('/')?
                        .1;
                    Some(plugin_protocol::commands::Descriptor {
                        plugin: provider.caller.plugin.clone(),
                        command: command.into(),
                        signature: contract.methods.get("invoke")?.clone(),
                    })
                })
            })
            .collect()
    }

    /// An explicitly named command pins the current provider incarnation, without service preferences.
    pub(crate) fn command_reference(
        &self,
        caller: &Caller,
        plugin: &str,
        command: &str,
    ) -> Result<Reference, Failure> {
        let id = plugin_protocol::commands::contract(plugin, command);
        let provider = self
            .providers
            .values()
            .find(|provider| {
                provider.caller.plugin == plugin
                    && provider.caller.scope == caller.scope
                    && provider.alive.load(Ordering::Acquire)
                    && provider.contracts.contains_key(&id)
            })
            .ok_or_else(|| {
                Failure::new(
                    ErrorCode::CapabilityUnavailable,
                    "Typed command is unavailable in this caller scope",
                )
            })?;
        let dependency = Dependency {
            version: semver::VersionReq::parse("^1").unwrap(),
            optional: false,
            methods: provider.contracts[&id].methods.clone(),
        };
        self.resolve_pinned(caller, &id, &dependency, &provider.caller.instance)
    }
    /// Reply admission validates unchanged source authority, original shape and the 64 KiB bound.
    pub(crate) fn complete_deferred(
        &mut self,
        call: Call,
        result: Result<Value, Failure>,
    ) -> Result<(), Failure> {
        if !self.context_alive(&call.context) || call.completion.status().is_terminal() {
            return Err(Failure::new(
                ErrorCode::InvalidHandle,
                "Invocation source or deadline ended",
            ));
        }
        if self.completed.len() >= 128
            || serde_json::to_vec(&result).map_or(true, |bytes| bytes.len() > 65536)
        {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Deferred reply quota exceeded",
            ));
        }
        if let Ok(value) = &result {
            call.signature.result.accepts(value)?;
        }
        call.completion.finish(result.clone());
        if !matches!(
            call.completion.status(),
            plugin_protocol::api::RequestUpdate::Completed { .. }
        ) {
            return Err(Failure::new(
                ErrorCode::InvalidHandle,
                "Invocation was cancelled before completion",
            ));
        }
        self.completed.push_back((call, result));
        Ok(())
    }
    /// Manager observations are delivered once, without holding a guest store under the broker lock.
    pub(crate) fn take_completed(&mut self) -> Vec<(Call, Result<Value, Failure>)> {
        self.completed.drain(..).collect()
    }
    /// Every delegation hop must still be the exact active incarnation that authorized the work.
    pub fn context_alive(&self, context: &Context) -> bool {
        context
            .lifetimes
            .iter()
            .all(|alive| alive.load(Ordering::Acquire))
    }
    pub fn new(preferences: Preferences) -> Self {
        Self {
            preferences,
            ..Default::default()
        }
    }
    /// Every participant that offers contracts, whether a runtime instance or the host itself.
    ///
    /// A guest is discoverable because its instance is alive and active; the host's own session
    /// services are offered for as long as the runtime they belong to is alive. Both are published
    /// through this one list, so a consumer's selection and this registry cannot disagree about who
    /// offers a contract.
    pub fn reconcile(&mut self, guests: Vec<Provider>, host: Vec<Provider>) {
        self.providers = guests
            .into_iter()
            .chain(host)
            .map(|p| (p.caller.instance.clone(), p))
            .collect();
        self.refresh_selections();
        self.queue.retain(|call| {
            let alive = self.providers.contains_key(&call.handle.instance)
                && self
                    .providers
                    .contains_key(&call.reference.provider.caller.instance);
            if !alive {
                call.completion.finish(Err(Failure::new(
                    ErrorCode::InvalidHandle,
                    "Service participant exited or was replaced",
                )));
            }
            alive
        });
    }
    /// Track changes per contract and logical scope, without invalidating unrelated service handles.
    pub fn refresh_selections(&mut self) {
        let mut available: BTreeMap<(String, String), Vec<String>> = self
            .selections
            .keys()
            .cloned()
            .map(|key| (key, Vec::new()))
            .collect();
        for provider in self.providers.values() {
            for contract in provider.contracts.keys() {
                // Command ownership is explicit; these entries are not selectable service providers.
                if contract.starts_with(plugin_protocol::commands::CONTRACT_PREFIX) {
                    continue;
                }
                available
                    .entry((provider.caller.scope.clone(), contract.clone()))
                    .or_default()
                    .push(provider.caller.instance.clone());
            }
        }
        for ((scope, contract), providers) in available {
            let preference = self.preference(&scope, &contract).cloned();
            let selection = self.selections.entry((scope, contract)).or_default();
            if selection.providers != providers || selection.preference != preference {
                selection.revision = selection
                    .revision
                    .checked_add(1)
                    .expect("Service selection revision exhausted");
                selection.providers = providers;
                selection.preference = preference;
            }
        }
    }
    fn preference(&self, scope: &str, contract: &str) -> Option<&String> {
        self.preferences
            .projects
            .get(scope)
            .and_then(|map| map.get(contract))
            .or_else(|| self.preferences.user.get(&choice_key(scope, contract)))
    }
    /// Package identity of the provider a start in this scope would use; descriptive, never routing.
    pub fn selected_provider(&self, scope: &str, contract: &str) -> Option<String> {
        if let Some(id) = self.preference(scope, contract)
            && self
                .providers
                .values()
                .any(|provider| &provider.caller.plugin == id)
        {
            return Some(id.clone());
        }
        let mut candidates = self
            .providers
            .values()
            .filter(|provider| {
                provider.caller.scope == scope && provider.contracts.contains_key(contract)
            })
            .map(|provider| provider.caller.plugin.clone());
        let first = candidates.next()?;
        // An ambiguous contract has no single owner; report nothing rather than guessing one.
        candidates.next().is_none().then_some(first)
    }
    pub fn resolve(
        &self,
        caller: &Caller,
        contract: &str,
        dependency: &Dependency,
    ) -> Result<Reference, Failure> {
        let candidates: Vec<_> = self
            .providers
            .values()
            .filter(|p| {
                p.alive.load(Ordering::Acquire)
                    && p.caller.scope == caller.scope
                    && p.contracts
                        .get(contract)
                        .is_some_and(|c| dependency.matches(c))
            })
            .collect();
        let selected = if let Some(id) = self.preference(&caller.scope, contract) {
            candidates
                .iter()
                .find(|p| &p.caller.plugin == id)
                .copied()
                .ok_or_else(|| {
                    Failure::new(
                        ErrorCode::CapabilityUnavailable,
                        "Selected service provider is unavailable or incompatible",
                    )
                })?
        } else if candidates.len() == 1 {
            candidates[0]
        } else {
            return Err(Failure::new(
                if candidates.is_empty() {
                    ErrorCode::CapabilityUnavailable
                } else {
                    ErrorCode::Conflict
                },
                format!(
                    "Service {contract} has {} compatible providers; choose one in plugin settings",
                    candidates.len()
                ),
            ));
        };
        Ok(Reference {
            provider: selected.clone(),
            contract: contract.into(),
            dependency: dependency.clone(),
            revision: self
                .selections
                .get(&(caller.scope.clone(), contract.into()))
                .map_or(0, |selection| selection.revision),
            pinned: false,
        })
    }
    /// Resolve an existing session's exact incarnation without consulting future-launch preferences.
    pub fn resolve_pinned(
        &self,
        caller: &Caller,
        contract: &str,
        dependency: &Dependency,
        instance: &str,
    ) -> Result<Reference, Failure> {
        let provider = self
            .providers
            .get(instance)
            .filter(|provider| {
                provider.caller.scope == caller.scope
                    && provider.alive.load(Ordering::Acquire)
                    && provider
                        .contracts
                        .get(contract)
                        .is_some_and(|shape| dependency.matches(shape))
            })
            .ok_or_else(|| {
                Failure::new(
                    ErrorCode::InvalidHandle,
                    "Session provider exited or changed its contract",
                )
            })?;
        Ok(Reference {
            provider: provider.clone(),
            contract: contract.into(),
            dependency: dependency.clone(),
            revision: 0,
            pinned: true,
        })
    }
    /// Open references retain their provider incarnation; changed user choices cannot retarget them.
    pub fn validate_reference(
        &self,
        caller: &Caller,
        reference: &Reference,
    ) -> Result<(), Failure> {
        if reference.pinned {
            self.resolve_pinned(
                caller,
                &reference.contract,
                &reference.dependency,
                &reference.provider.caller.instance,
            )?;
            return Ok(());
        }
        let current = self
            .resolve(caller, &reference.contract, &reference.dependency)
            .map_err(|_| {
                Failure::new(
                    ErrorCode::InvalidHandle,
                    "Service reference is no longer available",
                )
            })?;
        if current.revision != reference.revision
            || current.provider.caller.instance != reference.provider.caller.instance
        {
            return Err(Failure::new(
                ErrorCode::InvalidHandle,
                "Service provider changed; open a new reference",
            ));
        }
        Ok(())
    }
    pub fn enqueue(&mut self, call: Call) -> Result<(), Failure> {
        self.queue
            .retain(|call| !call.completion.update().1.is_terminal());
        if self.queue.len() >= 256 {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Service queue is full",
            ));
        }
        self.queue.push_back(call);
        Ok(())
    }
    /// A tick dispatches at most 32 callbacks; nested calls wait for a later tick instead of reentering stores.
    pub fn take_batch(&mut self) -> Vec<Call> {
        let count = self.queue.len().min(32);
        self.queue.drain(..count).collect()
    }
    pub fn choices(&self, scope: &str) -> Vec<Choice> {
        let mut contracts: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for provider in self.providers.values().filter(|p| p.caller.scope == scope) {
            for contract in provider.contracts.keys() {
                // Explicit command ownership is never a user-selectable service preference.
                if contract.starts_with(plugin_protocol::commands::CONTRACT_PREFIX) {
                    continue;
                }
                contracts
                    .entry(contract.clone())
                    .or_default()
                    .insert(provider.caller.plugin.clone());
            }
        }
        contracts
            .into_iter()
            .map(|(contract, candidates)| {
                let selected = self.preference(scope, &contract).cloned().or_else(|| {
                    (candidates.len() == 1).then(|| candidates.iter().next().unwrap().clone())
                });
                Choice {
                    user: self
                        .preferences
                        .user
                        .get(&choice_key(scope, &contract))
                        .cloned(),
                    project: self
                        .preferences
                        .projects
                        .get(scope)
                        .and_then(|values| values.get(&contract))
                        .cloned(),
                    scope: if scope == "application" {
                        plugin_protocol::api::InstanceScope::Application
                    } else {
                        plugin_protocol::api::InstanceScope::Workspace
                    },
                    contract,
                    candidates: candidates.into_iter().collect(),
                    selected,
                }
            })
            .collect()
    }
}

/// Application and workspace contracts may share a name without sharing provider preferences.
pub(crate) fn choice_key(scope: &str, contract: &str) -> String {
    format!(
        "{}/{contract}",
        if scope == "application" {
            "application"
        } else {
            "workspace"
        }
    )
}
