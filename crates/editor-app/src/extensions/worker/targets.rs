//! The actor polls bounded target requests; Cargo/domain parsing stays inside providers.
use super::*;
use plugin_runtime::plugin_protocol::{api::RequestUpdate, targets::Candidate};
/// Successful sources and unavailable sources remain distinct in one bounded discovery receipt.
#[derive(Clone, Debug, Default)]
pub(crate) struct TargetCatalog {
    pub candidates: Vec<plugin_schema::DiscoveredTarget>,
    pub failed: BTreeMap<String, String>,
    pub declarative_error: Option<String>,
}
#[derive(Default)]
pub(in crate::extensions) struct TargetCalls {
    preparations: BTreeMap<u64, (String, usize, plugin_runtime::TargetRequest)>,
    discoveries: Vec<(
        String,
        u64,
        TargetCatalog,
        Vec<(String, plugin_runtime::TargetRequest)>,
    )>,
}
impl TargetCalls {
    /// The queue accepts immutable snapshots; explicit cancel revokes actual preparation roots.
    pub fn dispatch(
        &mut self,
        work: Work,
        manager: &mut Manager,
        output: &Arc<Mutex<Published>>,
    ) -> Option<Work> {
        match work {
            Work::PrepareTarget {
                config,
                index,
                request,
                provider,
                binding,
                env,
            } => {
                let result = if self.preparations.len() >= 128 {
                    Err(anyhow::anyhow!("Preparation queue is full"))
                } else {
                    manager.begin_target_call(&provider,"prepare",serde_json::json!({"workspace":manager.workspace(),"binding":binding,"env":env}))
                };
                match result {
                    Ok(pending) => {
                        self.preparations.insert(request, (config, index, pending));
                    }
                    Err(error) => output.lock().unwrap().target_preparations.push((
                        config,
                        index,
                        request,
                        Err(format!("{error:#}")),
                    )),
                };
                None
            }
            Work::CancelTarget { request, mode } => {
                if let Some((_, _, pending)) = self.preparations.get(&request) {
                    pending.stop_with(mode);
                    manager.poll();
                }
                None
            }
            Work::DiscoverTargets { workspace, request } => {
                // A newer explicit discovery replaces the wait, revoking all abandoned discovery tools.
                self.discoveries.clear();
                let declarative = crate::extensions::contributions::discover_run_targets(
                    std::path::Path::new(&workspace),
                );
                let mut pending = Vec::new();
                let mut catalog = TargetCatalog::default();
                for (index, provider) in manager.target_providers().into_iter().enumerate() {
                    if index >= 16 {
                        catalog
                            .failed
                            .insert(provider, "Target discovery provider quota exceeded".into());
                        continue;
                    }
                    match manager.begin_target_call(
                        &provider,
                        "discover",
                        serde_json::json!({"workspace":workspace}),
                    ) {
                        Ok(call) => pending.push((provider, call)),
                        Err(error) => {
                            catalog.failed.insert(provider, format!("{error:#}"));
                        }
                    }
                }
                match declarative {
                    Ok(targets) => catalog.candidates = targets,
                    Err(error) => catalog.declarative_error = Some(error),
                };
                self.discoveries
                    .push((workspace, request, catalog, pending));
                None
            }
            work => Some(work),
        }
    }
    /// Terminal receipts are consumed once. Revoked instances/workspaces never publish a usable artifact.
    pub fn poll(&mut self, manager: &Manager, output: &Arc<Mutex<Published>>) {
        self.preparations
            .retain(|request, (config, index, pending)| {
                let outcome = result(pending, manager);
                let mut snapshot = pending.snapshot();
                if let Some(result) = &outcome {
                    if snapshot.state != plugin_runtime::ExecutionState::Exited {
                        snapshot.state = if result.is_ok() {
                            plugin_runtime::ExecutionState::Exited
                        } else {
                            plugin_runtime::ExecutionState::Failed
                        };
                    }
                }
                output
                    .lock()
                    .unwrap()
                    .target_snapshots
                    .insert(*request, (config.clone(), *index, snapshot));
                let Some(result) = outcome else {
                    return true;
                };
                let result = result.and_then(|value| {
                    value["program"]
                        .as_str()
                        .filter(|program| !program.is_empty() && !program.contains('\0'))
                        .map(str::to_owned)
                        .ok_or_else(|| "Preparation returned no executable".into())
                });
                output.lock().unwrap().target_preparations.push((
                    config.clone(),
                    *index,
                    *request,
                    result,
                ));
                false
            });
        self.discoveries
            .retain(|(workspace, request, initial, pending)| {
                if pending.iter().any(|(_, call)| !call.status().is_terminal()) {
                    return true;
                }
                let mut catalog = initial.clone();
                for (provider, call) in pending {
                    match result(call, manager)
                        .unwrap()
                        .and_then(|value| decode(provider, value))
                    {
                        Ok(candidates) => catalog.candidates.extend(candidates),
                        Err(error) => {
                            catalog.failed.insert(provider.clone(), error);
                        }
                    }
                }
                let result = if catalog.candidates.len() <= 128 {
                    Ok(catalog)
                } else {
                    Err("Target catalog exceeds 128".into())
                };
                output.lock().unwrap().target_discoveries.push((
                    workspace.clone(),
                    *request,
                    result,
                ));
                false
            });
    }
}
/// A complete typed result is accepted only from the invocation's actual original live instance.
fn result(
    request: &plugin_runtime::TargetRequest,
    manager: &Manager,
) -> Option<Result<serde_json::Value, String>> {
    if !request.valid_for(manager) {
        request.stop();
    }
    match request.status() {
        RequestUpdate::Accepted | RequestUpdate::Progress { .. } => None,
        RequestUpdate::Completed { result } if request.valid_for(manager) => {
            Some(result.map_err(|error| error.message))
        }
        _ => Some(Err(
            "Target provider/workspace retired or preparation stopped".into(),
        )),
    }
}
/// Provider identity is authenticated by routing; raw candidate IDs cannot impersonate another plugin.
pub(super) fn decode(
    provider: &str,
    value: serde_json::Value,
) -> Result<Vec<plugin_schema::DiscoveredTarget>, String> {
    let candidates: Vec<Candidate> =
        serde_json::from_value(value["targets"].clone()).map_err(|error| error.to_string())?;
    Ok(candidates
        .into_iter()
        .map(|candidate| plugin_schema::DiscoveredTarget {
            id: format!("{}:{provider}:{}", provider.len(), candidate.identity),
            provider: provider.into(),
            target_type: format!("{}@{}", candidate.target_type, candidate.type_version),
            label: candidate.label.clone(),
            program: candidate.label,
            found_in: candidate.source,
            fields: [
                ("provider_binding".into(), candidate.binding),
                ("type_version".into(), candidate.type_version.to_string()),
            ]
            .into_iter()
            .collect(),
        })
        .collect())
}
