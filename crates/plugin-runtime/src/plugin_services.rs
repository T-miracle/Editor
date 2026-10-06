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
}
/// A selection revision never rolls back when a provider is removed and later selected again.
#[derive(Default)]
struct Selection {
    providers: Vec<String>,
    preference: Option<String>,
    revision: u64,
}
/// The original source and shrinking authority survive nested calls; ancestry bounds asynchronous cycles.
#[derive(Clone)]
pub(crate) struct Context {
    pub lifetimes: Vec<Arc<AtomicBool>>,
    pub caller: Caller,
    pub ancestry: Vec<String>,
    pub permissions: BTreeSet<String>,
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
    pub preferences: Preferences,
    selections: BTreeMap<(String, String), Selection>,
}
impl Broker {
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
    /// Reconciliation invalidates queued work when either exact instance incarnation is gone.
    pub fn reconcile(&mut self, providers: Vec<Provider>) {
        self.providers = providers
            .into_iter()
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
        })
    }
    /// Open references retain their provider incarnation; changed user choices cannot retarget them.
    pub fn validate_reference(
        &self,
        caller: &Caller,
        reference: &Reference,
    ) -> Result<(), Failure> {
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
