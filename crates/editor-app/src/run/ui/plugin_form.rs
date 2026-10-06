//! Public plugin configuration integration: immutable request origins and one window draft.
//! Rendering and command orchestration are separate so native form values stay provider-owned.
use super::*;
use editor_core::{ConfigurationValidation, PluginConfiguration, RunConfig, RunConfigSet};
use plugin_runtime::plugin_protocol::{configurations as contract, ui as native};
use std::collections::{BTreeMap, BTreeSet};

mod actions;
mod projection;
mod render;
use projection::{projection, validated};
mod state;
pub(super) use state::*;
#[cfg(test)]
mod fault_tests;
#[cfg(test)]
mod native_profile;
#[cfg(test)]
mod rollout_tests;
mod tree;
mod tree_actions;
#[cfg(test)]
mod tree_tests;
pub(super) use render::render;

enum Purpose {
    Catalog(gpui_kit::EntityId),
    Form {
        window: gpui_kit::EntityId,
        id: String,
        snapshot: PluginConfiguration,
    },
    Commit {
        window: gpui_kit::EntityId,
        id: String,
        snapshot: PluginConfiguration,
    },
    Execute {
        id: String,
        snapshot: PluginConfiguration,
        action: Execution,
    },
}

/// Host-generated identities are never supplied by a provider; a close removes only that window's calls.
#[derive(Default)]
pub(crate) struct Bridge {
    pending: BTreeMap<u64, (String, Purpose)>,
    resume: Option<(String, u8)>,
    origins: BTreeMap<String, plugin_runtime::TargetOrigin>,
    live_origins: Vec<plugin_runtime::TargetOrigin>,
}
impl Bridge {
    fn reserve(&mut self, purpose: Purpose, workspace: String) -> u64 {
        // Application replacement must not reuse IDs while the worker still holds old publications.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.pending.insert(id, (workspace, purpose));
        id
    }
}

impl EditorApp {
    /// Carry validated provenance into the existing actor launch/preparation path.
    pub(in crate::run::ui) fn stage_validated_run(&self, id: &str, work: Work, cx: &App) -> bool {
        let origin = self.plugin_configuration_bridge.origins.get(id).cloned();
        self.extensions
            .read(cx)
            .stage_host_run(work.validated(origin))
    }
    /// Initialize the new native template surface in its actual owned HWND, preserving external selection.
    pub(super) fn initialize_plugin_form(
        &mut self,
        form: &Entity<RunConfigForm>,
        cx: &mut Context<Self>,
    ) {
        let set = self.run_controls.configuration_set();
        let selected = set.selected.clone();
        let tree = cx.new(|cx| gpui_base::TreeState::new(cx));
        let observed_window = form.entity_id();
        let observer = cx.observe(&tree, move |app, tree, cx| {
            let chosen = tree
                .read(cx)
                .selected_item()
                .map(|item| item.id.to_string());
            let current = app
                .run_form
                .as_ref()
                .filter(|form| form.entity_id() == observed_window)
                .and_then(|form| form.read(cx).plugin.as_ref())
                .and_then(|state| state.selected.clone());
            if chosen != current {
                if let Some(id) = chosen {
                    app.select_plugin_configuration(&id, cx);
                }
            }
        });
        form.update(cx, |form, cx| {
            let mut state = State::new(set, selected.clone());
            state.tree = Some(tree);
            state.tree_observer = Some(observer);
            form.plugin = Some(Box::new(state));
            cx.notify();
        });
        let request = self
            .plugin_configuration_bridge
            .reserve(Purpose::Catalog(form.entity_id()), self.workspace_key());
        self.extensions
            .read(cx)
            .stage_host_run(Work::ConfigurationCatalog {
                request,
                arguments: self.configuration_context(),
            });
        if let Some(id) = selected {
            self.request_plugin_form(form, &id, None, cx);
        }
    }

    fn configuration_context(&self) -> serde_json::Value {
        serde_json::json!({"workspace":self.workspace_key(),"locale":rust_i18n::locale().to_string(),"os":std::env::consts::OS})
    }

    fn configuration_arguments(&self, data: &PluginConfiguration) -> serde_json::Value {
        let mut arguments = self.configuration_context();
        arguments["template"] = data.template.clone().into();
        arguments["values"] = data.values.clone().into();
        arguments
    }

    /// Form events are serialized per configuration, keeping rapid typing from losing earlier values.
    fn request_plugin_form(
        &mut self,
        form: &Entity<RunConfigForm>,
        id: &str,
        event: Option<contract::FormEvent>,
        cx: &mut Context<Self>,
    ) {
        let snapshot = form.update(cx, |form, cx| {
            let state = form.plugin.as_mut()?;
            if state.commit.is_some() {
                return None;
            }
            let data = state.draft.plugin_configurations.get_mut(id)?;
            if let Some(event) = event {
                let event = serde_json::to_string(&event).ok()?;
                if data.pending_events.len() >= 512
                    || data.pending_events.iter().map(String::len).sum::<usize>() + event.len()
                        > 64 * 1024
                {
                    state.edit_overflow.insert(id.into());
                    state.error = Some(t!("run.plugin_edit_quota").into());
                    cx.notify();
                    return None;
                }
                data.pending_events.push(event);
                data.validation = ConfigurationValidation::Unchecked;
                state.edit_failures.remove(id);
            }
            cx.notify();
            (!state.editing.contains_key(id)).then(|| data.clone())
        });
        let Some(snapshot) = snapshot else {
            return;
        };
        let origin = form
            .read(cx)
            .plugin
            .as_ref()
            .and_then(|state| state.form_origins.get(id))
            .cloned();
        let mut arguments = self.configuration_arguments(&snapshot);
        arguments["event"] = snapshot
            .pending_events
            .first()
            .cloned()
            .unwrap_or_default()
            .into();
        let request = self.plugin_configuration_bridge.reserve(
            Purpose::Form {
                window: form.entity_id(),
                id: id.into(),
                snapshot: snapshot.clone(),
            },
            self.workspace_key(),
        );
        form.update(cx, |form, cx| {
            form.plugin
                .as_mut()
                .unwrap()
                .editing
                .insert(id.into(), request);
            cx.notify();
        });
        self.extensions.read(cx).stage_host_run(
            Work::ConfigurationCall {
                request,
                provider: snapshot.provider,
                method: "form".into(),
                arguments,
            }
            .validated(origin),
        );
    }

    /// All results enter through the same production actor; stale origins cannot update a replacement window.
    pub(super) fn sync_plugin_configurations(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (catalogs, replies) = self.extensions.read(cx).take_configuration_replies();
        let origins = self.extensions.read(cx).configuration_origins();
        if self.plugin_configuration_bridge.live_origins != origins {
            self.plugin_configuration_bridge.live_origins = origins.clone();
            // Losing a provider changes visible validity without rewriting any user-owned values.
            let set = self.run_controls.configuration_set();
            for (id, mut data) in set.plugin_configurations {
                if matches!(data.validation, ConfigurationValidation::Valid)
                    && !origins
                        .iter()
                        .any(|origin| origin.provider() == data.provider)
                {
                    data.validation = ConfigurationValidation::Unavailable(
                        t!("run.plugin_provider_unavailable").into(),
                    );
                    if let Some(configuration) = self.run_controls.configuration(&id).cloned() {
                        let _ = self
                            .run_controls
                            .accept_configuration_projection(configuration, data);
                    }
                }
            }
            if let Some(form) = self.run_form.clone() {
                form.update(cx, |form, cx| {
                    if let Some(state) = form.plugin.as_mut() {
                        let retired = state
                            .form_origins
                            .iter()
                            .filter_map(|(id, origin)| {
                                (!origins.contains(origin)).then_some(id.clone())
                            })
                            .collect::<Vec<_>>();
                        for id in retired {
                            state.views.remove(&id);
                            state.documents.remove(&id);
                            state.form_origins.remove(&id);
                            if !state.editing.contains_key(&id) && state.commit.is_none() {
                                if let Some(data) = state.draft.plugin_configurations.get_mut(&id) {
                                    data.validation = ConfigurationValidation::Unavailable(
                                        t!("run.plugin_provider_unavailable").into(),
                                    );
                                }
                            }
                        }
                    }
                    cx.notify();
                });
            }
        }
        for (request, catalog) in catalogs {
            let Some((workspace, Purpose::Catalog(origin))) =
                self.plugin_configuration_bridge.pending.remove(&request)
            else {
                continue;
            };
            if workspace != self.workspace_key() {
                continue;
            }
            let Some(form) = self
                .run_form
                .clone()
                .filter(|form| form.entity_id() == origin)
            else {
                continue;
            };
            form.update(cx, |form, cx| {
                if let Some(state) = form.plugin.as_mut() {
                    state.catalog = catalog
                        .templates
                        .into_iter()
                        .filter(|(provider, _)| {
                            catalog.origins.iter().any(|origin| {
                                origin.provider() == provider && origins.contains(origin)
                            })
                        })
                        .collect();
                    state.error = (!catalog.failures.is_empty()).then(|| {
                        catalog
                            .failures
                            .into_iter()
                            .map(|(provider, error)| format!("{provider}: {error}"))
                            .collect::<Vec<_>>()
                            .join("\n")
                    });
                }
                cx.notify();
            });
        }
        for reply in replies {
            let Some((workspace, purpose)) = self
                .plugin_configuration_bridge
                .pending
                .remove(&reply.request)
            else {
                continue;
            };
            if workspace != self.workspace_key() {
                continue;
            }
            let result = if reply.result.is_ok()
                && reply
                    .origin
                    .as_ref()
                    .is_none_or(|origin| !origins.contains(origin))
            {
                Err(t!("run.plugin_stale").into())
            } else {
                reply.result
            };
            match purpose {
                Purpose::Form {
                    window: origin,
                    id,
                    snapshot,
                } => {
                    if result.is_ok() {
                        if let Some(receipt_origin) = reply.origin {
                            if let Some(form) = self
                                .run_form
                                .clone()
                                .filter(|form| form.entity_id() == origin)
                            {
                                form.update(cx, |form, _| {
                                    if let Some(state) = form.plugin.as_mut() {
                                        state.form_origins.insert(id.clone(), receipt_origin);
                                    }
                                });
                            }
                        }
                    }
                    self.accept_plugin_form(origin, &id, snapshot, result, cx)
                }
                Purpose::Commit {
                    window: origin,
                    id,
                    snapshot,
                } => self.accept_plugin_commit(origin, &id, snapshot, result, cx),
                Purpose::Execute {
                    id,
                    snapshot,
                    action,
                } => {
                    if let Some(origin) = reply.origin {
                        self.plugin_configuration_bridge
                            .origins
                            .insert(id.clone(), origin);
                    }
                    self.accept_plugin_execution(&id, snapshot, action, result, window, cx)
                }
                Purpose::Catalog(_) => {}
            }
        }
    }

    fn accept_plugin_form(
        &mut self,
        origin: gpui_kit::EntityId,
        id: &str,
        snapshot: PluginConfiguration,
        reply: Result<serde_json::Value, String>,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = self
            .run_form
            .clone()
            .filter(|form| form.entity_id() == origin)
        else {
            return;
        };
        form.update(cx, |form, cx| {
            let Some(state) = form.plugin.as_mut() else {
                return;
            };
            state.editing.remove(id);
            let Some(current) = state.draft.plugin_configurations.get(id).cloned() else {
                return;
            };
            // More input can be queued while this request runs. Only acknowledged canonical values
            // must still match; the newer queue suffix belongs to the same draft and stays intact.
            if !same_form_values(&current, &snapshot) {
                return;
            }
            let outcome = reply
                .and_then(contract::decode::<contract::Form>)
                .and_then(|reply| {
                    reply.document.validate()?;
                    if reply.document.source.is_some()
                        || reply.document.editor_toolbar.is_some()
                        || reply.document.editor_image_input
                        || reply.document.link_events
                        || reply.document.code_highlighting
                        || reply.document.editor_viewport.is_some()
                    {
                        return Err(
                            "Configuration form cannot claim editor-source capabilities".into()
                        );
                    }
                    let mut data = current.clone();
                    if let Some(event) = snapshot.pending_events.first() {
                        if data.pending_events.first() != Some(event) {
                            return Err("Stale form acknowledgement".into());
                        }
                        data.pending_events.remove(0);
                    }
                    data.values = reply.values;
                    data.name = reply.name;
                    data.program = reply.program;
                    if !data.storage_valid() {
                        return Err("Invalid configuration form envelope".into());
                    }
                    if data.values != snapshot.values
                        || data.name != snapshot.name
                        || data.program != snapshot.program
                    {
                        data.revision = data
                            .revision
                            .checked_add(1)
                            .ok_or("Configuration revision exhausted")?;
                        data.validation = ConfigurationValidation::Unchecked;
                    }
                    // Opening an unchanged form must preserve its prepared launch and editor breakpoints.
                    // Changed values invalidate the receipt; preparation is replaced only after validation.
                    if data != snapshot || state.draft.find(id).is_none() {
                        let mut configuration = projection(&data, id, None)?;
                        if let Some(previous) = state.draft.find(id) {
                            configuration.breakpoints = previous.breakpoints.clone();
                        }
                        state
                            .draft
                            .upsert(configuration)
                            .map_err(|error| error.to_string())?;
                    }
                    state.edit_failures.remove(id);
                    state.draft.plugin_configurations.insert(id.into(), data);
                    state.documents.insert(id.into(), reply.document);
                    Ok(())
                });
            state.error = outcome.err();
            if let Some(error) = &state.error {
                state.edit_failures.insert(id.into());
                if let Some(data) = state.draft.plugin_configurations.get_mut(id) {
                    data.validation = ConfigurationValidation::Unavailable(error.clone());
                }
            }
            cx.notify();
        });
        self.advance_plugin_edits(&form, id, cx);
    }

    fn advance_plugin_edits(
        &mut self,
        form: &Entity<RunConfigForm>,
        id: &str,
        cx: &mut Context<Self>,
    ) {
        let next = form.update(cx, |form, _| {
            form.plugin.as_ref().is_some_and(|state| {
                !state.edit_failures.contains(id)
                    && state
                        .draft
                        .plugin_configurations
                        .get(id)
                        .is_some_and(|data| !data.pending_events.is_empty())
            })
        });
        if next {
            self.request_plugin_form(form, id, None, cx);
        } else {
            let mode = form.update(cx, |form, _| {
                let state = form.plugin.as_mut()?;
                (!state.busy())
                    .then(|| state.commit_requested.take())
                    .flatten()
            });
            if let Some(mode) = mode {
                self.begin_plugin_commit(mode, cx);
            }
        }
    }

    /// Closing a native window retires only its panel and validation requests, not active run sessions.
    pub(super) fn cancel_plugin_window_calls(&mut self, cx: &mut Context<Self>) {
        let Some(form) = &self.run_form else {
            return;
        };
        let origin = form.entity_id();
        let requests: Vec<_> = self
            .plugin_configuration_bridge
            .pending
            .iter()
            .filter_map(|(id, (_, purpose))| {
                let belongs = match purpose {
                    Purpose::Catalog(window)
                    | Purpose::Form { window, .. }
                    | Purpose::Commit { window, .. } => *window == origin,
                    _ => false,
                };
                belongs.then_some(*id)
            })
            .collect();
        for request in &requests {
            self.plugin_configuration_bridge.pending.remove(request);
        }
        self.extensions
            .read(cx)
            .stage_host_run(Work::CancelConfigurations { requests });
    }
}

/// A delayed acknowledgement may consume only the canonical revision that produced it.
fn same_form_values(left: &PluginConfiguration, right: &PluginConfiguration) -> bool {
    left.provider == right.provider
        && left.template == right.template
        && left.values == right.values
        && left.name == right.name
        && left.program == right.program
        && left.revision == right.revision
}
