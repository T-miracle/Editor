//! Source-local UI projects the same approved plugin tree and disappears with its owning preview.
use super::*;
use gpui_kit::{AnyElement, Styled};

impl EditorApp {
    /// The toolbar lives inside the source pane so every preview mode uses the same visibility boundary.
    pub(crate) fn render_editor_source_toolbar(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let panel = self.active_editor_preview(cx)?;
        if panel
            .read(cx)
            .current_document()
            .is_none_or(|scene| scene.active_native_editor().is_none())
        {
            panel.update(cx, |panel, _| panel.native_toolbar = None);
            return None;
        }
        let toolbar = panel.update(cx, |panel, cx| panel.source_toolbar(window, cx))?;
        Some(
            div()
                .debug_selector(|| "editor-source-toolbar".into())
                .w_full()
                .flex_shrink_0()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(toolbar)
                .into_any_element(),
        )
    }
}

impl ExtensionPanel {
    /// Keep keyed controls across publication updates, but never revive a missing or stale source tree.
    fn source_toolbar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<crate::ui::plugin::PluginView>> {
        let Some(mut document) = self.current_document().map(|document| (*document).clone()) else {
            self.native_toolbar = None;
            return None;
        };
        let Some(mut toolbar) = document.editor_toolbar.take() else {
            self.native_toolbar = None;
            return None;
        };
        // A modal in the complete publication also owns input over its source-local projection.
        toolbar.disabled |= document.dialog.is_some() || document.menu.is_some();
        document.root = toolbar;
        // This projection has no preview scroll and cannot claim the original scene's viewport stream.
        document.editor_viewport = None;
        document.dialog = None;
        document.menu = None;
        let environment = self.native_environment(cx);
        if let Some(view) = &self.native_toolbar {
            view.update(cx, |view, cx| {
                view.update_document(document, environment, window, cx)
            });
        } else {
            let tx = self.worker.tx.clone();
            let plugin = self.active.clone()?;
            let panel = self.surface_id.clone()?;
            let visible = self.visible.clone();
            let epoch = self.instance_epoch;
            let parent = self.parent.clone();
            let owner = cx.entity().downgrade();
            self.native_toolbar = Some(cx.new(|cx| {
                crate::ui::plugin::PluginView::new(
                    plugin.clone(),
                    document,
                    environment,
                    move |event, cx| {
                        if visible.get() {
                            // The callback runs inside PluginView's update. A cadence flush also updates
                            // that view's code jobs, so defer until its native input borrow is released.
                            let parent = parent.clone();
                            let owner = owner.clone();
                            let tx = tx.clone();
                            let plugin = plugin.clone();
                            let panel = panel.clone();
                            cx.defer(move |cx| {
                                if matches!(event.action, protocol::ui::Action::Click) {
                                    let handled = parent
                                        .update(cx, |app, cx| {
                                            let Some(active) =
                                                app.active_editor_preview(cx).filter(|active| {
                                                    Some(active) == owner.upgrade().as_ref()
                                                })
                                            else {
                                                return true;
                                            };
                                            let Some(version) =
                                                app.active_tab_index().and_then(|index| {
                                                    app.plugin_document_version(index).ok()
                                                })
                                            else {
                                                return true;
                                            };
                                            let queued = active.update(cx, |panel, _| {
                                                let Some(document) = panel.current_document()
                                                else {
                                                    return true;
                                                };
                                                if document.source.as_ref() == Some(&version) {
                                                    return false;
                                                }
                                                if epoch != panel.instance_epoch
                                                    || event.revision != document.revision
                                                {
                                                    return true;
                                                }
                                                let mut node = None;
                                                if let Some(toolbar) = &document.editor_toolbar {
                                                    toolbar.visit(&mut |candidate| {
                                                        if candidate.id == event.node {
                                                            node = Some(candidate.clone());
                                                        }
                                                    });
                                                }
                                                if let Some(node) = node {
                                                    panel.pending_toolbar =
                                                        Some((epoch, version, node, event.clone()));
                                                    panel.preview_ready = true;
                                                }
                                                true
                                            });
                                            if queued {
                                                app.sync_editor_previews(cx);
                                            }
                                            queued
                                        })
                                        .unwrap_or(true);
                                    if handled {
                                        return;
                                    }
                                }
                                let _ = tx.send(Work::Event(
                                    plugin.clone(),
                                    epoch,
                                    Some(panel.clone()),
                                    PluginEvent::Ui(event),
                                ));
                            });
                        }
                    },
                    window,
                    cx,
                )
                .content_sized()
            }));
        }
        let key = format!(
            "{}/{}",
            self.active.as_deref().unwrap_or_default(),
            self.surface_id.as_deref().unwrap_or_default()
        );
        // Source canvases consume the same worker-owned raster slots as every other native node.
        self.native_toolbar
            .as_ref()
            .unwrap()
            .update(cx, |view, cx| {
                view.update_images(&key, &self.images, window, cx)
            });
        self.native_toolbar.clone()
    }
}
