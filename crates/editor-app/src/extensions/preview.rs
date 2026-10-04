//! File-scoped plugin surfaces share the editor body while the native editor keeps its state.

use super::*;

mod presentation;
mod toolbar;

impl ExtensionPanel {
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
    /// Select one visible, authorized preview deterministically when several plugins match a file.
    pub(crate) fn active_editor_preview(&self, cx: &App) -> Option<Entity<ExtensionPanel>> {
        let extension = self.active_path.as_ref()?.extension()?.to_str()?;
        let owner = self.extensions.read(cx);
        let mut matches = owner
            .entries
            .iter()
            .filter(|entry| {
                entry.enabled && entry.error.is_none() && entry.grants.contains("editor.read")
            })
            .flat_map(|entry| {
                entry.manifest.panels.iter().filter_map(|descriptor| {
                    (descriptor.position == "editor"
                        && descriptor
                            .file_extensions
                            .iter()
                            .any(|candidate| candidate.eq_ignore_ascii_case(extension)))
                    .then(|| format!("{}/{}", entry.manifest.id, descriptor.id))
                })
            })
            .filter_map(|key| {
                self.plugin_panels
                    .get(&key)
                    .filter(|panel| panel.read(cx).visible.get())
                    .map(|panel| (key, panel.clone()))
            })
            .collect::<Vec<_>>();
        matches.sort_by(|a, b| a.0.cmp(&b.0));
        matches.into_iter().next().map(|(_, panel)| panel)
    }

    /// Input changes invalidate the token even for clean disk reloads that keep the saved revision.
    pub(crate) fn invalidate_editor_previews(&self, cx: &mut Context<Self>) {
        for panel in self.plugin_panels.values() {
            panel.update(cx, |panel, cx| {
                if panel.editor_preview {
                    // A retained focus handle does not authorize a background result from the previous source.
                    panel.invalidate_code_highlighting(cx);
                    panel.preview_document = None;
                    panel.preview_version = None;
                    panel.preview_error = None;
                }
            });
        }
    }

    /// Publish unsaved text once per document revision, and clear a surface when its file changes.
    pub(crate) fn sync_editor_previews(&self, cx: &mut Context<Self>) {
        let selected = self.active_editor_preview(cx);
        let version = self
            .active_tab_index()
            .and_then(|index| self.plugin_document_version(index).ok());
        let context = self.active_tab_index().map(|index| {
            (
                self.tabs[index].session.path().to_path_buf(),
                self.tabs[index].session.revision(),
            )
        });
        for panel in self.plugin_panels.values() {
            if !panel.read(cx).editor_preview {
                continue;
            }
            let active = selected.as_ref().is_some_and(|selected| selected == panel);
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
                            panel.preview_error = Some("文档超过 1 MiB，无法预览".into());
                            panel.native_ui = None;
                            panel.native_toolbar = None;
                            panel.send(protocol::api::Notification::Preview {
                                document: None,
                                text: String::new(),
                            });
                            cx.notify();
                            return;
                        }
                        // Retain control focus only for this same source identity. The exact version
                        // gate still hides old trees until the new guest publication has arrived.
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

    /// Base owns pointer capture and minimum pane sizes; this layer supplies the shared appearance.
    pub(crate) fn render_editor_preview_split(
        &self,
        source: gpui_kit::AnyElement,
        preview: Entity<ExtensionPanel>,
        cx: &App,
    ) -> gpui_kit::AnyElement {
        gpui_base::h_resizable("editor-preview-split")
            .with_handle_appearance(Rc::new(|_, _, cx| {
                Some(
                    div()
                        .debug_selector(|| "editor-preview-divider".into())
                        .w(px(1.))
                        .h_full()
                        .bg(cx.theme().border)
                        .into_any_element(),
                )
            }))
            .child(
                gpui_base::resizable_panel()
                    .size_range(px(100.)..Pixels::MAX)
                    .child(source),
            )
            .child(
                gpui_base::resizable_panel()
                    .size_range(px(100.)..Pixels::MAX)
                    .child(
                        v_flex()
                            .debug_selector(|| "editor-preview-pane".into())
                            .size_full()
                            .min_h_0()
                            .overflow_hidden()
                            .bg(cx.theme().background)
                            .child(preview),
                    ),
            )
            .into_any_element()
    }
}

impl ExtensionPanel {
    /// A temporary publication gap retains focus metadata, never displays or routes its stale tree.
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
