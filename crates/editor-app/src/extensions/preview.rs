//! File-scoped plugin surfaces share the editor body while the native editor keeps its state.

use super::*;

mod preference_import;
mod presentation;
mod providers;
mod toolbar;
pub(crate) mod viewport;

impl ExtensionPanel {
    /// A failed file decode belongs to this exact current file and can expose the host retry action.
    pub(crate) fn file_preview_failed(&self) -> bool {
        let Some(version) = &self.preview_file else {
            return false;
        };
        let prefix = format!(
            "{}/{}/image/",
            self.active.as_deref().unwrap_or(""),
            self.surface_id.as_deref().unwrap_or("")
        );
        self.images.photos.iter().any(|(key, photo)| {
            key.starts_with(&prefix)
                && photo.resource.source == protocol::api::ContentVersion::File(version.clone())
                && photo.decoded.is_err()
        })
    }

    /// Withdraw derived code jobs in every native projection owned by this surface.
    pub(crate) fn invalidate_code_highlighting(&mut self, cx: &mut Context<Self>) {
        for view in self.native_ui.iter().chain(self.native_toolbar.iter()) {
            view.update(cx, |view, cx| view.invalidate_code_highlighting(cx));
        }
        cx.notify();
    }

    /// Layout restoration must exclude panels whose lifecycle follows the current document.
    pub(crate) fn is_editor_preview(&self) -> bool {
        self.editor_preview
    }
}

impl EditorApp {
    /// Failures explain the currently matching contribution; the file identity and retry target remain live.
    pub(crate) fn file_view_unavailable_reason(&self, cx: &App) -> String {
        if let Some(error) = self
            .active_tab_index()
            .and_then(|index| self.tabs[index].file_error.as_ref())
        {
            return error.clone();
        }
        if !self.session_state.workspace_trusted {
            return t!("file_view.restricted").to_string();
        }
        if self.file_provider_selection_needed(cx) {
            return t!("file_view.choose_provider").to_string();
        }
        let extension = self
            .active_path
            .as_ref()
            .and_then(|path| path.extension())
            .and_then(|value| value.to_str())
            .unwrap_or("");
        let owner = self.extensions.read(cx);
        let chosen = self.remembered_file_provider_key();
        let candidates = owner
            .entries
            .iter()
            .filter(|entry| {
                entry.manifest.panels.iter().any(|panel| {
                    panel.position == "editor"
                        && !panel.auxiliary
                        && chosen
                            .as_ref()
                            .is_none_or(|key| *key == format!("{}/{}", entry.manifest.id, panel.id))
                        && panel
                            .readonly_file_extensions
                            .iter()
                            .any(|value| value.eq_ignore_ascii_case(extension))
                })
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return if chosen.is_some() {
                t!("file_view.provider_unavailable")
            } else {
                t!("file_view.no_provider")
            }
            .to_string();
        }
        if let Some(error) = candidates.iter().find_map(|entry| entry.error.as_ref()) {
            return error.clone();
        }
        if candidates.iter().all(|entry| !entry.enabled) {
            return t!("file_view.disabled").to_string();
        }
        t!("file_view.denied").to_string()
    }

    /// Advancing the file epoch withdraws pending resource consumers and permits a fresh bounded decode.
    pub(crate) fn retry_file_view(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.active_tab_index() else {
            return;
        };
        let extension = self.tabs[index]
            .path()
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        // Only enabled faulted matches are restarted; retry never grants permission or enables a plugin.
        let chosen = self.remembered_file_provider_key();
        let failed = self
            .extensions
            .read(cx)
            .entries
            .iter()
            .filter(|entry| {
                entry.enabled
                    && entry.error.is_some()
                    && entry.manifest.panels.iter().any(|panel| {
                        panel.position == "editor"
                            && !panel.auxiliary
                            && chosen.as_ref().is_none_or(|key| {
                                *key == format!("{}/{}", entry.manifest.id, panel.id)
                            })
                            && panel
                                .readonly_file_extensions
                                .iter()
                                .any(|value| value.eq_ignore_ascii_case(extension))
                    })
            })
            .map(|entry| entry.manifest.id.clone())
            .collect::<Vec<_>>();
        for id in failed {
            self.extensions
                .read(cx)
                .worker
                .queue_lifecycle(Work::Restart(id));
        }
        self.tabs[index].file_revision = self.tabs[index].file_revision.saturating_add(1);
        self.tabs[index].file_error = None;
        self.invalidate_editor_previews(cx);
        self.sync_editor_previews(cx);
        self.editor_panel.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    /// The user's workspace/file-type selection is the sole authority for a complete center layout.
    pub(crate) fn active_editor_preview(&self, cx: &App) -> Option<Entity<ExtensionPanel>> {
        self.selected_file_provider(cx)
    }

    /// Input changes invalidate the token even for clean disk reloads that keep the saved revision.
    pub(crate) fn invalidate_editor_previews(&self, cx: &mut Context<Self>) {
        for panel in self.plugin_panels.values() {
            panel.update(cx, |panel, cx| {
                if panel.editor_preview {
                    panel.source_viewport.reset();
                    // A retained focus handle does not authorize a background result from the previous source.
                    panel.invalidate_code_highlighting(cx);
                    // Keep the last published input records until synchronization can revoke
                    // an unselected provider. Clearing them here loses FilePreview(None).
                    // Text acceptance is invalidated independently by its exact source token.
                    panel.preview_version = None;
                    panel.preview_error = None;
                }
            });
        }
    }

    /// Publish unsaved text once per document revision, and clear a surface when its file changes.
    pub(crate) fn sync_editor_previews(&mut self, cx: &mut Context<Self>) {
        self.remember_single_file_provider(cx);
        self.sync_file_previews(cx);
        let selected = self.active_editor_preview(cx);
        let version = self
            .active_tab_index()
            .and_then(|index| self.plugin_document_version(index).ok());
        let context = self
            .active_text_tab_index()
            .and_then(|index| self.text_tab(index))
            .map(|text| (text.path().to_path_buf(), text.session.revision()));
        for panel in self.plugin_panels.values() {
            if !panel.read(cx).editor_preview {
                continue;
            }
            let active = self.file_panel_is_active(panel, &selected, cx);
            panel.update(cx, |panel, cx| {
                if active && (panel.preview_document != context || panel.preview_version != version)
                {
                    if context.is_some() {
                        panel.preview_error = None;
                        let text = self.editor.read(cx).text().to_string();
                        // No source token means the document is outside this workspace's authority.
                        let Some(version) = &version else {
                            panel.preview_version = None;
                            panel.preview_document = context.clone();
                            panel.native_ui = None;
                            panel.native_toolbar = None;
                            panel.send(protocol::api::Notification::Preview {
                                document: None,
                                text: String::new(),
                            });
                            return;
                        };
                        if text.len() > 1024 * 1024 {
                            // The source token remains current while the oversized text never crosses WASM.
                            panel.preview_version = Some(version.clone());
                            panel.preview_document = context.clone();
                            panel.preview_error = Some(t!("preview.document_limit").to_string());
                            panel.native_ui = None;
                            panel.native_toolbar = None;
                            panel.send(protocol::api::Notification::Preview {
                                document: None,
                                text: String::new(),
                            });
                            cx.notify();
                            return;
                        }
                        // Retain focus and readonly paint only for this same source identity.
                        // Guest events still require the exact version until fresh publication arrives.
                        panel.invalidate_code_highlighting(cx);
                        if panel
                            .native_ui
                            .as_ref()
                            .is_some_and(|view| !view.read(cx).same_source_document(version))
                        {
                            panel.native_ui = None;
                        }
                        if panel
                            .native_toolbar
                            .as_ref()
                            .is_some_and(|view| !view.read(cx).same_source_document(version))
                        {
                            panel.native_toolbar = None;
                        }
                        panel.send(protocol::api::Notification::Preview {
                            document: Some(version.clone()),
                            text,
                        });
                        panel.preview_version = Some(version.clone());
                        panel.preview_document = context.clone();
                    }
                } else if !active && panel.preview_document.take().is_some() {
                    // The removed source observer will not receive a pointer release in the next document.
                    panel.source_viewport.withdraw();
                    panel.send(protocol::api::Notification::Preview {
                        document: None,
                        text: String::new(),
                    });
                    panel.preview_version = None;
                    panel.preview_error = None;
                    panel.native_ui = None;
                    panel.native_toolbar = None;
                }
            });
        }
    }

    /// Binary files publish resource identity alone. Withdrawing it revokes bytes before a new tree arrives.
    fn sync_file_previews(&self, cx: &mut Context<Self>) {
        let selected = self.active_editor_preview(cx);
        let context = self
            .active_tab_index()
            .map(|index| self.plugin_file_context(index));
        for panel in self.plugin_panels.values() {
            if !panel.read(cx).editor_preview {
                continue;
            }
            let participates = self.file_panel_is_active(panel, &selected, cx);
            let owner = self.editor_preview_owner_key(panel, cx);
            let legacy = owner
                .as_deref()
                .and_then(|owner| self.session_state.legacy_display_payload(owner));
            let panel_state = panel.read(cx);
            let file_contract = self.active_text_tab_index().is_none()
                || panel_state.entries.iter().any(|entry| {
                    Some(&entry.manifest.id) == panel_state.active.as_ref()
                        && entry.supports_capability("ui.tools")
                        && entry.supports_capability("editor.files")
                });
            let active = context.is_some() && participates && file_contract;
            panel.update(cx, |panel, cx| {
                if active {
                    let file = context
                        .as_ref()
                        .and_then(|context| context.as_ref().ok())
                        .cloned();
                    let version = file.as_ref().map(|file| file.version.clone());
                    let source = file.as_ref().and_then(|file| file.text.clone());
                    // Queue import before FilePreview in this same actor channel so guest binding
                    // sees the stored legacy bundle; acknowledgement is required before session cleanup.
                    if let (Some(owner), Some(data), Some(file)) = (&owner, &legacy, &file)
                        && file.text.is_some()
                        && panel.legacy_import.as_ref()
                            != Some(&(panel.instance_epoch, file.file_type.clone()))
                        && panel.entries.iter().any(|entry| {
                            Some(&entry.manifest.id) == panel.active.as_ref()
                                && entry.grants.contains("storage")
                                && entry.supports_capability("storage.private")
                        })
                    {
                        panel.legacy_import = Some((panel.instance_epoch, file.file_type.clone()));
                        let _ = panel.worker.tx.send(Work::ImportPreference {
                            plugin: panel.active.clone().expect("owned preview"),
                            epoch: panel.instance_epoch,
                            owner: owner.clone(),
                            workspace: self.session_state.workspace.clone(),
                            key: protocol::api::PreferenceKey {
                                file_type: file.file_type.clone(),
                                name: "imported-presentation".into(),
                            },
                            data: data.clone(),
                        });
                    }
                    if panel.preview_file == version
                        && panel.preview_version == source
                        && panel.preview_document.is_some()
                    {
                        return;
                    }
                    let same_file = panel
                        .preview_file
                        .as_ref()
                        .zip(version.as_ref())
                        .is_some_and(|(old, new)| old.id == new.id && old.path == new.path);
                    panel.preview_error = context
                        .as_ref()
                        .and_then(|result| result.as_ref().err())
                        .map(ToString::to_string);
                    panel.preview_version = source;
                    panel.preview_file = version;
                    // Text receives its new snapshot below even when the disk-file epoch is unchanged.
                    panel.preview_document = self
                        .active_path
                        .clone()
                        .filter(|_| self.active_text_tab_index().is_none())
                        .map(|path| (path, file.as_ref().map_or(0, |file| file.version.revision)));
                    if !same_file {
                        panel.native_ui = None;
                        panel.native_toolbar = None;
                        panel.source_viewport.withdraw();
                    }
                    panel.send(protocol::api::Notification::FilePreview { file });
                    cx.notify();
                } else if panel.preview_file.take().is_some() {
                    panel.send(protocol::api::Notification::FilePreview { file: None });
                    panel.preview_version = None;
                    panel.preview_document = None;
                    panel.preview_error = None;
                    panel.native_ui = None;
                    panel.native_toolbar = None;
                    panel.source_viewport.withdraw();
                    cx.notify();
                }
            });
        }
    }

    /// File-resource tokens do not depend on text entities; closed/reopened paths cannot reuse them.
    pub(crate) fn plugin_file_context(
        &self,
        index: usize,
    ) -> Result<protocol::api::FileContext, protocol::api::Failure> {
        let file = self.tabs.get(index).ok_or_else(|| {
            protocol::api::Failure::new(protocol::api::ErrorCode::NotFound, "File is closed")
        })?;
        let relative = file
            .path()
            .strip_prefix(self.workspace.root())
            .map_err(|_| {
                protocol::api::Failure::new(
                    protocol::api::ErrorCode::PermissionDenied,
                    "File is outside the owning workspace",
                )
            })?;
        let context = protocol::api::FileContext {
            version: protocol::api::FileVersion {
                id: format!("file-{}", file.file_id),
                path: relative.to_string_lossy().replace('\\', "/"),
                revision: file
                    .text
                    .as_ref()
                    .map_or(file.file_revision, |text| text.capability_revision),
            },
            file_type: file
                .path()
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("")
                .to_ascii_lowercase(),
            text: self.plugin_document_version(index).ok(),
        };
        context.version.validate()?;
        Ok(context)
    }
}

impl ExtensionPanel {
    /// A publication gap retains same-source readonly paint; obsolete event authority is withdrawn.
    pub(super) fn retire_unowned_native_view(&mut self, cx: &App) {
        let retained = self.editor_preview
            && self.preview_version.as_ref().is_some_and(|version| {
                self.native_ui
                    .as_ref()
                    .is_some_and(|view| view.read(cx).same_source_document(version))
            });
        if !retained {
            self.native_ui = None;
        }
    }
}
