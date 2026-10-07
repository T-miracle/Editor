//! Draft-only structural operations and explicit window decisions share the same local baseline.
use super::*;
use gpui_base::input::{InputEvent, InputState};

impl EditorApp {
    /// Add a virtual folder at the ordinary insertion location and edit its name in the tree.
    pub(super) fn add_plugin_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.run_form.clone() else {
            return;
        };
        let workspace = self.workspace_key();
        let id = form.update(cx, |form, cx| {
            let state = form.plugin.as_mut()?;
            if state.commit.is_some() || state.draft.tree.folders.len() >= 128 {
                return None;
            }
            let parent = state.draft.insertion_parent(state.selected.as_deref());
            let id = state.draft.allocate_tree_id(&workspace).ok()?;
            state.draft.tree.folders.insert(
                id.clone(),
                editor_core::ConfigurationFolder {
                    name: t!("run.plugin_new_folder").into(),
                },
            );
            state.draft.place_tree_node(&id, parent, None).ok()?;
            state.selected = Some(id.clone());
            state.drawer = false;
            cx.notify();
            Some(id)
        });
        if let Some(id) = id {
            self.rename_plugin_folder(&id, window, cx);
        }
    }

    /// Folder labels use the existing native input engine, including selection and IME composition.
    pub(super) fn rename_plugin_folder(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = self.run_form.clone() else {
            return;
        };
        let Some(name) = form
            .read(cx)
            .plugin
            .as_ref()
            .and_then(|state| state.draft.tree.folders.get(id))
            .map(|folder| folder.name.clone())
        else {
            return;
        };
        let input = cx.new(|cx| InputState::new(window, cx).default_value(name));
        let renamed = id.to_owned();
        let origin = form.entity_id();
        let observer = cx.subscribe_in(&input, window, move |app, input, event, window, cx| {
            let Some(form) = app
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
                match event {
                    InputEvent::Change => {
                        if let Some(folder) = state.draft.tree.folders.get_mut(&renamed) {
                            folder.name = input.read(cx).value().to_string();
                        }
                    }
                    InputEvent::PressEnter { .. } => {
                        state.rename = None;
                    }
                    _ => {}
                }
                cx.notify();
            });
            if matches!(event, InputEvent::PressEnter { .. }) {
                let tree = form.read(cx).plugin.as_ref().unwrap().tree.clone().unwrap();
                tree.update(cx, |tree, cx| tree.focus(window, cx));
            }
        });
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        form.update(cx, |form, cx| {
            let state = form.plugin.as_mut().unwrap();
            state.rename = Some((id.into(), input));
            state.rename_observer = Some(observer);
            cx.notify();
        });
    }

    /// Empty tree space selects the virtual root without changing the external configuration.
    pub(super) fn clear_plugin_tree_selection(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(form) = self.run_form.clone() {
            let tree = form.update(cx, |form, cx| {
                let state = form.plugin.as_mut().unwrap();
                if state.commit.is_some() {
                    return None;
                }
                state.selected = None;
                state.rename = None;
                cx.notify();
                state.tree.clone()
            });
            if let Some(tree) = tree {
                tree.update(cx, |tree, cx| {
                    tree.set_selected_index(None, cx);
                    tree.focus(window, cx);
                });
            }
        }
    }

    /// Copies get independent identity and opaque values; only the provider can rename its own record.
    pub(super) fn copy_plugin_configuration(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.run_form.clone() else {
            return;
        };
        let workspace = self.workspace_key();
        let copied = form.update(cx, |form, cx| {
            let state = form.plugin.as_mut()?;
            if state.busy() || state.draft.configurations.len() >= editor_core::MAX_RUN_CONFIGS {
                return None;
            }
            let original = state.selected.clone()?;
            let mut data = state.draft.plugin_configurations.get(&original)?.clone();
            let id = state.draft.allocate_tree_id(&workspace).ok()?;
            data.name = t!("run.form_copy_name", name = data.name.clone()).into();
            data.validation = ConfigurationValidation::Unchecked;
            data.revision = 0;
            let mut configuration = state.draft.find(&original)?.clone();
            configuration.id = id.clone();
            configuration.name = data.name.clone();
            state.draft.upsert(configuration).ok()?;
            state
                .draft
                .plugin_configurations
                .insert(id.clone(), data.clone());
            let parent = state.draft.tree_parent(&original);
            state.draft.place_tree_node(&id, parent, None).ok()?;
            state.selected = Some(id.clone());
            cx.notify();
            Some((id, data.name))
        });
        if let Some((id, name)) = copied {
            self.request_plugin_form(
                &form,
                &id,
                Some(contract::FormEvent::Rename {
                    configuration_name: name,
                }),
                cx,
            );
        }
    }

    /// The same boundary serves pointer drops and keyboard reordering; failed moves leave a draft intact.
    pub(super) fn move_plugin_tree_node(
        &mut self,
        id: &str,
        parent: Option<String>,
        before: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        if let Some(form) = self.run_form.clone() {
            form.update(cx, |form, cx| {
                let state = form.plugin.as_mut().unwrap();
                if state.commit.is_some() {
                    return;
                }
                state.error = state
                    .draft
                    .place_tree_node(id, parent, before)
                    .err()
                    .map(|_| t!("run.plugin_tree_move_error").into());
                cx.notify();
            });
        }
    }

    /// Recursive deletion is counted and confirmed before it touches any window draft.
    pub(super) fn request_plugin_delete(&mut self, cx: &mut Context<Self>) {
        if let Some(form) = self.run_form.clone() {
            form.update(cx, |form, cx| {
                let state = form.plugin.as_mut().unwrap();
                if state.commit.is_none() {
                    state.decision = state.selected.clone().map(Decision::Delete);
                }
                cx.notify();
            });
        }
    }

    /// Confirmed removal cancels calls for only the affected identities, preserving existing sessions.
    pub(super) fn confirm_plugin_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.run_form.clone() else {
            return;
        };
        let removed = form.update(cx, |form, cx| {
            let state = form.plugin.as_mut().unwrap();
            let Some(Decision::Delete(id)) = state.decision.take() else {
                return vec![];
            };
            let removed = state.draft.tree_subtree(&id);
            state.draft.remove_tree_node(&id);
            if state
                .selected
                .as_ref()
                .is_some_and(|id| removed.contains(id))
            {
                state.selected = None;
            }
            for id in &removed {
                state.documents.remove(id);
                state.form_origins.remove(id);
                state.views.remove(id);
                state.edit_failures.remove(id);
                state.edit_overflow.remove(id);
                state.editing.remove(id);
            }
            state.rename = None;
            cx.notify();
            removed
        });
        let mut requests = vec![];
        self.plugin_configuration_bridge.pending.retain(|request, (_, purpose)| {
            let delete = matches!(purpose, Purpose::Form { window, id, .. } if *window == form.entity_id() && removed.contains(id));
            if delete { requests.push(*request); }
            !delete
        });
        self.extensions
            .read(cx)
            .stage_host_run(Work::CancelConfigurations { requests });
        self.focus_run_form_body(window, cx);
    }

    /// Continue or cancel the inner prompt, keeping drafts and restoring a live native focus owner.
    pub(super) fn dismiss_plugin_decision(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(form) = self.run_form.clone() {
            form.update(cx, |form, cx| {
                form.plugin.as_mut().unwrap().decision = None;
                cx.notify();
            });
        }
        self.focus_run_form_body(window, cx);
    }

    /// X and Escape request a three-way decision; Cancel explicitly discards only unapplied data.
    pub(in crate::run::ui) fn request_plugin_close(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(form) = self.run_form.clone() else {
            return true;
        };
        let Some(state) = form.read(cx).plugin.as_ref() else {
            return true;
        };
        if state.dirty() {
            form.update(cx, |form, cx| {
                form.plugin.as_mut().unwrap().decision = Some(Decision::Close);
                cx.notify();
            });
            false
        } else {
            self.close_run_form(cx);
            true
        }
    }
}
