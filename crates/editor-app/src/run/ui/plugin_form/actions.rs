//! Window transactions and execution intents share validation, but never share commit authority.
use super::*;

impl EditorApp {
    /// Instantiation creates a fresh host identity and only stages the provider's defaults.
    pub(super) fn add_plugin_configuration(
        &mut self,
        provider: String,
        template: contract::Template,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = self.run_form.clone() else {
            return;
        };
        if template.unavailable.is_some() {
            return;
        }
        let workspace = self.workspace_key();
        let id = form.update(cx, |form, cx| {
            let state = form.plugin.as_mut()?;
            if state.commit.is_some()
                || state.draft.configurations.len() >= editor_core::MAX_RUN_CONFIGS
            {
                return None;
            }
            let id = state.draft.generate_id(&workspace);
            let data = PluginConfiguration {
                provider,
                template: template.id,
                values: template.defaults,
                name: template.label,
                program: "plugin-pending".into(),
                revision: 0,
                validation: ConfigurationValidation::Unchecked,
            };
            let configuration = projection(&data, &id, None).ok()?;
            state.draft.upsert(configuration).ok()?;
            state.draft.plugin_configurations.insert(id.clone(), data);
            state.selected = Some(id.clone());
            state.drawer = false;
            state.error = None;
            cx.notify();
            Some(id)
        });
        if let Some(id) = id {
            self.request_plugin_form(&form, &id, None, cx);
        }
    }

    /// Editing selection never changes the external run target and never discards another draft.
    pub(super) fn select_plugin_configuration(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(form) = self.run_form.clone() else {
            return;
        };
        let load = form.update(cx, |form, cx| {
            let Some(state) = form.plugin.as_mut() else {
                return false;
            };
            if !state.draft.plugin_configurations.contains_key(id) {
                return false;
            }
            state.selected = Some(id.into());
            cx.notify();
            !state.documents.contains_key(id) && !state.editing.contains_key(id)
        });
        if load {
            self.request_plugin_form(&form, id, None, cx);
        }
    }

    /// Each queued native action keeps its exact configuration identity; provider events remain ordered.
    pub(super) fn plugin_configuration_event(
        &mut self,
        id: &str,
        event: native::UiEvent,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = self.run_form.clone() else {
            return;
        };
        let send = form.update(cx, |form, cx| {
            let Some(state) = form.plugin.as_mut() else {
                return false;
            };
            if state.commit.is_some() {
                return false;
            }
            if state.editing.contains_key(id) {
                let queue = state.events.entry(id.into()).or_default();
                if queue.len() < 512 {
                    queue.push_back(event.clone());
                } else {
                    state.error = Some(t!("run.plugin_edit_quota").into());
                }
                cx.notify();
                false
            } else {
                true
            }
        });
        if send {
            self.request_plugin_form(&form, id, Some(event), cx);
        }
    }

    /// Save waits for serialized edits before validating a snapshot; Apply validates only the current row.
    pub(in crate::run::ui) fn begin_plugin_commit(
        &mut self,
        mode: CommitMode,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = self.run_form.clone() else {
            return;
        };
        let ids = form.update(cx, |form, cx| {
            let state = form.plugin.as_mut()?;
            if state.busy() {
                state.commit_requested = Some(mode);
                return None;
            }
            let ids = match mode {
                CommitMode::Apply => state.selected.clone().into_iter().collect::<Vec<_>>(),
                CommitMode::Save => state.draft.plugin_configurations.keys().cloned().collect(),
            };
            state.error = None;
            state.commit = Some(Commit {
                mode,
                waiting: ids.clone(),
            });
            cx.notify();
            Some(ids)
        });
        let Some(ids) = ids else {
            return;
        };
        for id in ids {
            let snapshot = form
                .read(cx)
                .plugin
                .as_ref()
                .unwrap()
                .draft
                .plugin_configurations[&id]
                .clone();
            let mut arguments = self.configuration_arguments(&snapshot);
            arguments["intent"] = "save".into();
            let request = self.plugin_configuration_bridge.reserve(Purpose::Commit {
                window: form.entity_id(),
                id,
                snapshot: snapshot.clone(),
            });
            self.extensions
                .read(cx)
                .stage_host_run(Work::ConfigurationCall {
                    request,
                    provider: snapshot.provider,
                    method: "validate".into(),
                    arguments,
                });
        }
        self.finish_plugin_commit(&form, cx);
    }

    pub(super) fn accept_plugin_commit(
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
            if state.draft.plugin_configurations.get(id) != Some(&snapshot) {
                state.commit = None;
                state.error = Some(t!("run.plugin_stale").into());
                cx.notify();
                return;
            }
            let (data, configuration) = validated(snapshot, id, reply);
            if let Some(mut configuration) = configuration {
                if let Some(previous) = state.draft.find(id) {
                    configuration.breakpoints = previous.breakpoints.clone();
                }
                let _ = state.draft.upsert(configuration);
            }
            state.draft.plugin_configurations.insert(id.into(), data);
            if let Some(commit) = &mut state.commit {
                commit.waiting.retain(|pending| pending != id);
            }
            cx.notify();
        });
        self.finish_plugin_commit(&form, cx);
    }

    /// Durable success alone advances the baseline; business errors are saved, I/O errors keep the window.
    fn finish_plugin_commit(&mut self, form: &Entity<RunConfigForm>, cx: &mut Context<Self>) {
        let snapshot = form.read(cx).plugin.as_ref().and_then(|state| {
            let commit = state.commit.as_ref()?;
            if !commit.waiting.is_empty() {
                return None;
            }
            let mut set = match commit.mode {
                CommitMode::Save => state.draft.clone(),
                CommitMode::Apply => {
                    let mut baseline = state.baseline.clone();
                    if let Some(id) = &state.selected {
                        baseline.upsert(state.draft.find(id)?.clone()).ok()?;
                        baseline
                            .plugin_configurations
                            .insert(id.clone(), state.draft.plugin_configurations[id].clone());
                    }
                    baseline
                }
            };
            if matches!(commit.mode, CommitMode::Save) {
                set.selected = state.selected.clone();
            }
            Some((commit.mode, set))
        });
        let Some((mode, set)) = snapshot else {
            return;
        };
        let workspace = self.workspace_key();
        match self
            .run_controls
            .commit_configuration_set(set.clone(), &workspace)
        {
            Ok(()) => {
                form.update(cx, |form, cx| {
                    let state = form.plugin.as_mut().unwrap();
                    state.baseline = set;
                    state.commit = None;
                    cx.notify();
                });
                if matches!(mode, CommitMode::Save) {
                    self.close_run_form(cx);
                }
            }
            Err(error) => form.update(cx, |form, cx| {
                let state = form.plugin.as_mut().unwrap();
                state.commit = None;
                state.error = Some(error);
                cx.notify();
            }),
        }
        cx.notify();
    }

    /// Returning true means execution is pending or rejected; only a matching one-shot receipt resumes it.
    pub(in crate::run::ui) fn guard_plugin_execution(
        &mut self,
        id: &str,
        action: Execution,
        cx: &mut Context<Self>,
    ) -> bool {
        if self
            .plugin_configuration_bridge
            .resume
            .as_ref()
            .is_some_and(|(owner, kind)| owner == id && *kind == action.kind())
        {
            self.plugin_configuration_bridge.resume = None;
            return false;
        }
        let Some(snapshot) = self
            .run_controls
            .configuration_set()
            .plugin_configurations
            .get(id)
            .cloned()
        else {
            return false;
        };
        if !self.run_permitted(cx) {
            self.status = t!("run.restricted").into();
            cx.notify();
            return true;
        }
        let mut arguments = self.configuration_arguments(&snapshot);
        arguments["intent"] = match &action {
            Execution::Run(_) => "run",
            Execution::Build => "build",
            Execution::Debug => "debug",
        }
        .into();
        let request = self.plugin_configuration_bridge.reserve(Purpose::Execute {
            id: id.into(),
            snapshot: snapshot.clone(),
            action,
        });
        self.extensions
            .read(cx)
            .stage_host_run(Work::ConfigurationCall {
                request,
                provider: snapshot.provider,
                method: "validate".into(),
                arguments,
            });
        self.status = t!("run.plugin_validating").into();
        cx.notify();
        true
    }

    pub(super) fn accept_plugin_execution(
        &mut self,
        id: &str,
        snapshot: PluginConfiguration,
        action: Execution,
        reply: Result<serde_json::Value, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .run_controls
            .configuration_set()
            .plugin_configurations
            .get(id)
            != Some(&snapshot)
        {
            self.status = t!("run.plugin_stale").into();
            cx.notify();
            return;
        }
        let (data, configuration) = validated(snapshot, id, reply);
        let Some(mut configuration) = configuration else {
            if let Some(configuration) = self.run_controls.configuration(id).cloned() {
                let _ = self
                    .run_controls
                    .accept_configuration_projection(configuration, data.clone());
            }
            self.status = validation_reason(&data.validation);
            cx.notify();
            return;
        };
        // Breakpoints belong to the editor's document integration, independently from form preparation.
        if let Some(previous) = self.run_controls.configuration(id) {
            configuration.breakpoints = previous.breakpoints.clone();
        }
        if let Err(error) = self
            .run_controls
            .accept_configuration_projection(configuration, data)
        {
            self.status = error;
            cx.notify();
            return;
        }
        self.plugin_configuration_bridge.resume = Some((id.into(), action.kind()));
        match action {
            Execution::Run(env) => self.start_configuration(id, env, window, cx),
            Execution::Debug => self.debug_configuration(id, cx),
            Execution::Build => {
                if self
                    .run_controls
                    .selected()
                    .is_some_and(|configuration| configuration.id == id)
                {
                    self.build_selected(window, cx);
                } else {
                    self.plugin_configuration_bridge.resume = None;
                    self.status = t!("run.plugin_stale").into();
                }
            }
        }
    }
}

/// Visible storage state is not a substitute for fresh execution validation.
pub(super) fn validation_reason(status: &ConfigurationValidation) -> String {
    match status {
        ConfigurationValidation::Valid => String::new(),
        ConfigurationValidation::Unchecked => t!("run.plugin_unchecked").into(),
        ConfigurationValidation::Invalid(reason) | ConfigurationValidation::Unavailable(reason) => {
            reason.clone()
        }
    }
}
