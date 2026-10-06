//! Public plugin configuration integration: immutable request origins and one window draft.
//! Rendering and command orchestration are separate so native form values stay provider-owned.
use super::*;
use editor_core::{ConfigurationValidation, PluginConfiguration, RunConfig, RunConfigSet};
use plugin_runtime::plugin_protocol::{configurations as contract, ui as native};
use std::collections::{BTreeMap, VecDeque};

mod actions;
mod render;
mod tree;
mod tree_actions;
#[cfg(test)]
mod tree_tests;
pub(super) use render::render;

/// Window-local canonical values and keyed native views; the persisted baseline remains in RunControls.
pub(super) struct State {
    pub baseline: RunConfigSet,
    pub draft: RunConfigSet,
    pub selected: Option<String>,
    pub catalog: Vec<(String, contract::Template)>,
    pub drawer: bool,
    pub documents: BTreeMap<String, native::Document>,
    pub views: BTreeMap<String, Entity<crate::ui::plugin::PluginView>>,
    pub events: BTreeMap<String, VecDeque<contract::FormEvent>>,
    pub editing: BTreeMap<String, u64>,
    pub commit_requested: Option<CommitMode>,
    pub commit: Option<Commit>,
    pub error: Option<String>,
    pub scroll: gpui_kit::ScrollHandle,
    pub tree: Option<Entity<gpui_base::TreeState>>,
    pub tree_signature: String,
    pub tree_observer: Option<Subscription>,
    pub rename: Option<(String, Entity<gpui_base::input::InputState>)>,
    pub rename_observer: Option<Subscription>,
    pub decision: Option<Decision>,
}

impl State {
    pub fn new(set: RunConfigSet, selected: Option<String>) -> Self {
        let selected = selected.filter(|id| set.plugin_configurations.contains_key(id));
        Self {
            baseline: set.clone(),
            draft: set,
            selected,
            catalog: vec![],
            drawer: false,
            documents: BTreeMap::new(),
            views: BTreeMap::new(),
            events: BTreeMap::new(),
            editing: BTreeMap::new(),
            commit_requested: None,
            commit: None,
            error: None,
            scroll: gpui_kit::ScrollHandle::new(),
            tree: None,
            tree_signature: String::new(),
            tree_observer: None,
            rename: None,
            rename_observer: None,
            decision: None,
        }
    }
    /// Compare actual user data; rendered native controls and transient request state are not drafts.
    pub fn dirty(&self) -> bool {
        // Selection, validation receipts and executable caches are not unsaved user edits.
        fn user_data(set: &RunConfigSet) -> serde_json::Value {
            let values = set
                .plugin_configurations
                .iter()
                .map(|(id, data)| {
                    (
                        id.clone(),
                        serde_json::json!([data.provider, data.template, data.values, data.name]),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            serde_json::json!([set.tree.folders, set.tree.placements, values])
        }
        user_data(&self.draft) != user_data(&self.baseline) || self.busy()
    }
    pub fn busy(&self) -> bool {
        self.commit.is_some()
            || !self.editing.is_empty()
            || self.events.values().any(|queue| !queue.is_empty())
    }
}

/// Decisions stay inside the owned native window and never implicitly commit a destructive edit.
pub(super) enum Decision {
    Close,
    Delete(String),
}

#[derive(Clone, Copy)]
pub(super) enum CommitMode {
    Apply,
    Save,
}
pub(super) struct Commit {
    pub mode: CommitMode,
    pub waiting: Vec<String>,
    pub selected: Option<String>,
}

/// Execution intent captures its original arguments; changing external selection cannot retarget a Run.
pub(super) enum Execution {
    Run(Vec<plugin_runtime::RunEnvEntry>),
    Build,
    Debug,
}
impl Execution {
    fn kind(&self) -> u8 {
        match self {
            Self::Run(_) => 0,
            Self::Build => 1,
            Self::Debug => 2,
        }
    }
}

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
    next: u64,
    pending: BTreeMap<u64, Purpose>,
    resume: Option<(String, u8)>,
}
impl Bridge {
    fn reserve(&mut self, purpose: Purpose) -> u64 {
        self.next = self.next.wrapping_add(1).max(1);
        self.pending.insert(self.next, purpose);
        self.next
    }
}

impl EditorApp {
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
            .reserve(Purpose::Catalog(form.entity_id()));
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
        let Some(snapshot) = form
            .read(cx)
            .plugin
            .as_ref()
            .and_then(|state| state.draft.plugin_configurations.get(id))
            .cloned()
        else {
            return;
        };
        let mut arguments = self.configuration_arguments(&snapshot);
        arguments["event"] = event
            .map(|event| serde_json::to_string(&event).unwrap())
            .unwrap_or_default()
            .into();
        let request = self.plugin_configuration_bridge.reserve(Purpose::Form {
            window: form.entity_id(),
            id: id.into(),
            snapshot: snapshot.clone(),
        });
        form.update(cx, |form, cx| {
            form.plugin
                .as_mut()
                .unwrap()
                .editing
                .insert(id.into(), request);
            cx.notify();
        });
        self.extensions
            .read(cx)
            .stage_host_run(Work::ConfigurationCall {
                request,
                provider: snapshot.provider,
                method: "form".into(),
                arguments,
            });
    }

    /// All results enter through the same production actor; stale origins cannot update a replacement window.
    pub(super) fn sync_plugin_configurations(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (catalogs, replies) = self.extensions.read(cx).take_configuration_replies();
        for (request, catalog) in catalogs {
            let Some(Purpose::Catalog(origin)) =
                self.plugin_configuration_bridge.pending.remove(&request)
            else {
                continue;
            };
            let Some(form) = self
                .run_form
                .clone()
                .filter(|form| form.entity_id() == origin)
            else {
                continue;
            };
            form.update(cx, |form, cx| {
                if let Some(state) = form.plugin.as_mut() {
                    state.catalog = catalog.templates;
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
            let Some(purpose) = self
                .plugin_configuration_bridge
                .pending
                .remove(&reply.request)
            else {
                continue;
            };
            match purpose {
                Purpose::Form {
                    window: origin,
                    id,
                    snapshot,
                } => self.accept_plugin_form(origin, &id, snapshot, reply.result, cx),
                Purpose::Commit {
                    window: origin,
                    id,
                    snapshot,
                } => self.accept_plugin_commit(origin, &id, snapshot, reply.result, cx),
                Purpose::Execute {
                    id,
                    snapshot,
                    action,
                } => self.accept_plugin_execution(&id, snapshot, action, reply.result, window, cx),
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
            if state.draft.plugin_configurations.get(id) != Some(&snapshot) {
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
                    let mut data = snapshot.clone();
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
                    state.draft.plugin_configurations.insert(id.into(), data);
                    state.documents.insert(id.into(), reply.document);
                    Ok(())
                });
            state.error = outcome.err();
            if let Some(error) = &state.error {
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
            form.plugin
                .as_mut()
                .and_then(|state| state.events.get_mut(id))
                .and_then(VecDeque::pop_front)
        });
        if let Some(event) = next {
            self.request_plugin_form(form, id, Some(event), cx);
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
            .filter_map(|(id, purpose)| {
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

/// Provider launch data is converted through the existing generic target grammar, never Shell parsing.
fn projection(
    data: &PluginConfiguration,
    id: &str,
    launch: Option<contract::Launch>,
) -> Result<RunConfig, String> {
    let launch = launch.unwrap_or(contract::Launch {
        target: serde_json::json!({"mode":"program","program":data.program,"args":[]}),
        directory: None,
        env: Default::default(),
        tool_paths: vec![],
        build: vec![],
        prelaunch: vec![],
        provider: None,
    });
    let config: RunConfig = serde_json::from_value(serde_json::json!({
        "id":id,"name":if data.name.trim().is_empty(){id}else{&data.name},"target":launch.target,
        "directory":launch.directory,"env":launch.env,"tool_paths":launch.tool_paths,
        "build":launch.build,"prelaunch":launch.prelaunch,"provider":launch.provider,"local":true
    }))
    .map_err(|error| error.to_string())?;
    config.validate().map_err(|error| error.to_string())?;
    Ok(config)
}

/// A malformed success is unavailable, not valid; old prepared data remains non-authoritative.
fn validated(
    snapshot: PluginConfiguration,
    id: &str,
    reply: Result<serde_json::Value, String>,
) -> (PluginConfiguration, Option<RunConfig>) {
    let mut data = snapshot;
    match reply.and_then(contract::decode::<contract::Validation>) {
        Ok(validation) if validation.valid => match validation
            .launch
            .ok_or("Missing launch data".to_owned())
            .and_then(|launch| projection(&data, id, Some(launch)))
        {
            Ok(configuration) => {
                data.validation = ConfigurationValidation::Valid;
                return (data, Some(configuration));
            }
            Err(error) => data.validation = ConfigurationValidation::Unavailable(error),
        },
        Ok(validation) => data.validation = ConfigurationValidation::Invalid(validation.message),
        Err(error) => data.validation = ConfigurationValidation::Unavailable(error),
    }
    (data, None)
}
