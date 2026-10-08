//! Host templates share the normal draft/commit UI while executions own separate native processes.
use super::*;
use crate::plugin_development::{
    self,
    configuration::{self, Values},
};
use gpui_kit::{PathPromptOptions, PromptLevel};

impl EditorApp {
    /// Handle picker/reset actions locally; ordinary edits retain the normal serialized form path.
    pub(super) fn handle_host_configuration_event(
        &mut self,
        id: &str,
        event: &native::UiEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(form) = self.run_form.clone() else {
            return false;
        };
        let Some(data) = form
            .read(cx)
            .plugin
            .as_ref()
            .and_then(|state| state.draft.plugin_configurations.get(id))
            .cloned()
            .filter(|data| data.provider == configuration::PROVIDER)
        else {
            return false;
        };
        if !matches!(event.action, native::Action::Click) {
            return false;
        }
        if event.node == "reset-profile" {
            if self.plugin_configuration_bridge.jobs.active(id) {
                form.update(cx, |form, cx| {
                    form.plugin.as_mut().unwrap().error = Some(t!("plugin_dev.reset_busy").into());
                    cx.notify();
                });
            } else {
                let result = (|| -> anyhow::Result<()> {
                    let profile = plugin_development::profile(&self.workspace_key(), id)?;
                    if profile.exists() {
                        // Reset is reversible: retain the old isolated profile under a unique sibling.
                        let parent = profile
                            .parent()
                            .ok_or_else(|| anyhow::anyhow!("Invalid development profile"))?;
                        anyhow::ensure!(
                            profile.canonicalize()?.parent()
                                == Some(parent.canonicalize()?.as_path()),
                            "Profile escapes its private root"
                        );
                        // A CLI controller can own this configuration's profile without a GUI job.
                        // Keep the shared lease through the rename instead of relying on UI state.
                        let _lease = plugin_development::instance::profile_lease(&profile)?;
                        let backup = parent.join(format!(
                            "{}-reset-{}",
                            profile.file_name().unwrap().to_string_lossy(),
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)?
                                .as_nanos()
                        ));
                        std::fs::rename(profile, backup)?;
                    }
                    Ok(())
                })();
                form.update(cx, |form, cx| {
                    form.plugin.as_mut().unwrap().error =
                        result.err().map(|error| format!("{error:#}"));
                    cx.notify();
                });
            }
            return true;
        }
        let field = match event.node.as_str() {
            "output-browse" => "output",
            "projects-browse" => "projects",
            "workspace-browse" => "workspace",
            _ => return false,
        };
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: field == "projects",
            prompt: Some(
                match field {
                    "output" => t!("plugin_dev.choose_output"),
                    "workspace" => t!("plugin_dev.choose_workspace"),
                    _ => t!("plugin_dev.choose_projects"),
                }
                .into(),
            ),
        });
        let id = id.to_owned();
        let origin = form.entity_id();
        cx.spawn(async move |app, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await {
                let value = paths
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                let _ = app.update(cx, |app, cx| {
                    let valid = app
                        .run_form
                        .as_ref()
                        .filter(|form| form.entity_id() == origin)
                        .and_then(|form| form.read(cx).plugin.as_ref())
                        .and_then(|state| state.draft.plugin_configurations.get(&id))
                        .is_some_and(|current| current == &data);
                    if valid {
                        let event = native::UiEvent {
                            // A picker changes the native input intentionally; ordinary typing echoes
                            // must keep their value revision stable to preserve focus and IME state.
                            node: format!("picked-{field}"),
                            action: native::Action::Change(value),
                            revision: 0,
                        };
                        app.plugin_configuration_event(&id, event, cx);
                    }
                });
            }
        })
        .detach();
        true
    }

    /// Fresh host validation precedes every operation. Active work is located rather than duplicated.
    pub(in crate::run::ui) fn start_host_configuration(
        &mut self,
        id: &str,
        build_only: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(snapshot) = self
            .run_controls
            .configuration_set()
            .plugin_configurations
            .get(id)
            .cloned()
            .filter(|data| data.provider == configuration::PROVIDER)
        else {
            return false;
        };
        if !self.run_permitted(cx) {
            self.status = t!("run.restricted").into();
            cx.notify();
            return true;
        }
        if self.plugin_configuration_bridge.jobs.active(id) {
            cx.notify();
            return true;
        }
        let mut arguments = self.configuration_arguments(&snapshot);
        arguments["intent"] = if build_only { "build" } else { "run" }.into();
        let (data, projection) =
            validated(snapshot.clone(), id, configuration::validate(&arguments));
        let Some(projection) = projection else {
            self.status = actions::validation_reason(&data.validation);
            cx.notify();
            return true;
        };
        if let Err(error) = self
            .run_controls
            .accept_configuration_projection(projection, data)
        {
            self.status = error;
            cx.notify();
            return true;
        }
        if !build_only && !self.save_dirty_documents(cx) {
            return true;
        }
        if snapshot.template == "development" && !build_only {
            let result = (|| -> anyhow::Result<_> {
                let values: Values = serde_json::from_str(&snapshot.values)?;
                let project = plugin_runtime::development::Project::read(
                    &values.projects(&self.workspace_key())[0],
                )?;
                let manifest: plugin_runtime::plugin_protocol::Manifest = serde_json::from_slice(
                    &std::fs::read(project.root.join(&project.description.manifest))?,
                )?;
                let target = if values.workspace.is_empty() {
                    project.root.clone()
                } else {
                    let path = PathBuf::from(values.workspace);
                    if path.is_absolute() {
                        path
                    } else {
                        self.workspace.root().join(path)
                    }
                }
                .canonicalize()?;
                let root = plugin_development::profile(&self.workspace_key(), id)?;
                let consent = serde_json::json!({"project":project.root,"plugin":manifest.id,"workspace":target,"permissions":manifest.permissions});
                let current = std::fs::read(root.join("consent.json"))
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
                Ok((manifest.permissions, root, consent, current))
            })();
            let (permissions, root, consent, current) = match result {
                Ok(value) => value,
                Err(error) => {
                    self.status = format!("{error:#}");
                    cx.notify();
                    return true;
                }
            };
            if current.as_ref() != Some(&consent) {
                let allow = t!("plugin_dev.allow").to_string();
                let cancel = t!("plugin_dev.cancel").to_string();
                let answer = window.prompt(
                    PromptLevel::Warning,
                    &t!("plugin_dev.permission_title"),
                    Some(&t!(
                        "plugin_dev.permission_detail",
                        permissions = permissions.iter().cloned().collect::<Vec<_>>().join(", ")
                    )),
                    &[allow.as_str(), cancel.as_str()],
                    cx,
                );
                let id = id.to_owned();
                cx.spawn_in(window, async move |app, cx| {
                    if answer.await != Ok(0) {
                        return;
                    }
                    let _ = app.update_in(cx, |app, window, cx| {
                        if app
                            .run_controls
                            .configuration_set()
                            .plugin_configurations
                            .get(&id)
                            != Some(&snapshot)
                            || !app.run_permitted(cx)
                        {
                            return;
                        }
                        match plugin_development::instance::atomic_json(
                            &root.join("consent.json"),
                            &consent,
                        ) {
                            Ok(()) => {
                                app.start_host_configuration(&id, false, window, cx);
                            }
                            Err(error) => {
                                app.status = format!("{error:#}");
                                cx.notify();
                            }
                        }
                    });
                })
                .detach();
                return true;
            }
        }
        let result = (|| -> anyhow::Result<()> {
            let values: Values = serde_json::from_str(&snapshot.values)?;
            let mut args =
                values.args(&snapshot.template, &self.workspace_key(), build_only, id)?;
            if snapshot.template == "development" && !build_only {
                let project = plugin_runtime::development::Project::read(
                    &values.projects(&self.workspace_key())[0],
                )?;
                let manifest: plugin_runtime::plugin_protocol::Manifest = serde_json::from_slice(
                    &std::fs::read(project.root.join(&project.description.manifest))?,
                )?;
                for permission in manifest.permissions {
                    args.extend(["--grant".into(), permission]);
                }
            }
            let request = self.run_controls.begin(id);
            if let Err(error) = self.plugin_configuration_bridge.jobs.start(
                id,
                request,
                args,
                self.workspace.root(),
            ) {
                self.run_controls
                    .reject_start(id, request, &format!("{error:#}"));
                return Err(error);
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.status = format!("{error:#}");
        }
        cx.notify();
        true
    }
}
