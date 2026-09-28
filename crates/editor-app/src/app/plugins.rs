//! Tracks startup plugin loading and presents its progress beside the status bar.

use crate::*;
use gpui_kit::component::spinner::Spinner;

/// A plugin has exactly one visible lifecycle state during this launch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PluginLoadState {
    Enabled,
    Disabled,
    Loading,
    Error(String),
}

/// Keeps the plugin identity even when its grammar cannot be registered.
pub(crate) struct PluginLoadEntry {
    plugin: language_plugins::BundledPlugin,
    state: PluginLoadState,
    grammar_loaded: bool,
    server_loading: bool,
}

impl PluginLoadEntry {
    pub(crate) fn initial() -> Vec<Self> {
        language_plugins::BundledPlugin::ALL
            .into_iter()
            .filter(|plugin| extensions::contributions::plugin_root(plugin.manifest_id()).is_some())
            .map(|plugin| Self {
                plugin,
                grammar_loaded: false,
                server_loading: false,
                state: PluginLoadState::Loading,
            })
            .collect()
    }
}

/// The status bar separates in-progress work from actionable failures.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PluginPopupKind {
    Loading,
    Error,
}

impl EditorApp {
    /// A disabled language contribution must not start its external server.
    pub(crate) fn language_plugin_enabled(&self, language_id: &str) -> bool {
        self.plugin_loads
            .iter()
            .find(|entry| entry.plugin.language_id() == language_id)
            .is_some_and(|entry| entry.state != PluginLoadState::Disabled)
    }

    /// Run independent grammar validation off the UI thread and publish each result.
    pub(crate) fn start_plugin_loading(&mut self, cx: &mut Context<Self>) {
        let generation = self.plugin_loading_generation;
        for plugin in self
            .plugin_loads
            .iter()
            .map(|entry| entry.plugin)
            .collect::<Vec<_>>()
        {
            if self
                .plugin_loads
                .iter()
                .any(|entry| entry.plugin == plugin && entry.state == PluginLoadState::Disabled)
            {
                continue;
            }
            cx.spawn(async move |this, cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move { language_plugins::load_bundled_plugin(plugin) })
                    .await;
                let _ = this.update_in(cx, |app, _, cx| {
                    if app.plugin_loading_generation != generation {
                        return;
                    }
                    let state = match result {
                        Ok(()) => PluginLoadState::Enabled,
                        Err(error) => {
                            tracing::warn!(plugin = plugin.name(), %error, "plugin loading failed");
                            PluginLoadState::Error(format!("{error:#}"))
                        }
                    };
                    app.finish_grammar_loading(plugin, state, cx);
                });
            })
            .detach();
        }
    }

    /// Reconcile installed package changes with open editors and active theme resources.
    pub(crate) fn sync_runtime_contributions(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.plugin_loading_generation = self.plugin_loading_generation.wrapping_add(1);
        let previous = self
            .plugin_loads
            .iter()
            .map(|entry| entry.plugin)
            .collect::<Vec<_>>();
        self.plugin_loads = PluginLoadEntry::initial();
        for plugin in previous {
            if !self.plugin_loads.iter().any(|entry| entry.plugin == plugin) {
                language_plugins::mask_language(plugin.language_id());
                self.language_servers.remove(plugin.language_id());
                for tab in &self.tabs {
                    if tab
                        .session
                        .path()
                        .extension()
                        .and_then(|part| part.to_str())
                        == Some(if plugin == language_plugins::BundledPlugin::Rust {
                            "rs"
                        } else {
                            "toml"
                        })
                    {
                        tab.editor.update(cx, |editor, cx| {
                            editor.set_highlighter("text".to_owned(), cx)
                        });
                    }
                }
            }
        }
        for entry in &self.plugin_loads {
            // A hot update masks the previous version until the new grammar passes validation.
            language_plugins::mask_language(entry.plugin.language_id());
            self.language_servers.remove(entry.plugin.language_id());
        }
        let mut newly_created_servers = Vec::new();
        for tab in &self.tabs {
            let path = tab.session.path();
            let Some(contribution) = language_plugins::language_for_path(path) else {
                // A removed plugin must release its native editor LSP providers.
                if matches!(
                    path.extension().and_then(|part| part.to_str()),
                    Some("rs" | "toml")
                ) {
                    editor::detach_language_server(&tab.editor, cx);
                }
                continue;
            };
            editor::detach_language_server(&tab.editor, cx);
            if contribution.lsp_command.is_none() {
                continue;
            }
            let server = if let Some(server) = self.language_servers.get(&contribution.id) {
                server.clone()
            } else if let Some(server) = language_navigation::LanguageServer::new(
                self.workspace.root(),
                contribution.clone(),
            ) {
                let server = Arc::new(server);
                self.language_servers
                    .insert(contribution.id.clone(), server.clone());
                newly_created_servers.push((contribution.id.clone(), server.clone()));
                server
            } else {
                continue;
            };
            editor::attach_language_server(&tab.editor, path, server, cx.entity().downgrade(), cx);
        }
        for (language, server) in newly_created_servers {
            self.begin_server_loading(&language, cx);
            cx.spawn_in(window, async move |this, cx| {
                let result = cx
                    .background_executor()
                    .scheduler_executor()
                    .spawn_dedicated(move |_| async move { server.prepare_until_ready() })
                    .await;
                let _ = this.update_in(cx, |app, _, cx| {
                    app.finish_server_loading(&language, result, cx)
                });
            })
            .detach();
        }
        // A hot package update may replace a grammar or theme without changing its ID.
        apply_theme(&theme::active_theme(self.dark_theme), cx);
        self.start_plugin_loading(cx);
        cx.notify();
    }

    /// Grammar availability activates highlighting even when server startup is pending.
    fn finish_grammar_loading(
        &mut self,
        plugin: language_plugins::BundledPlugin,
        state: PluginLoadState,
        cx: &mut Context<Self>,
    ) {
        if let Some(entry) = self
            .plugin_loads
            .iter_mut()
            .find(|entry| entry.plugin == plugin)
        {
            entry.grammar_loaded = state == PluginLoadState::Enabled;
            if entry.grammar_loaded && matches!(entry.state, PluginLoadState::Error(_)) {
                // Server failures remain visible while the grammar becomes usable.
                self.activate_plugin_highlighting(plugin, cx);
                return;
            }
            if entry.grammar_loaded && entry.server_loading {
                self.set_plugin_state(plugin, PluginLoadState::Loading, cx);
                // Highlighting depends on the grammar, not the server readiness state.
                self.activate_plugin_highlighting(plugin, cx);
                return;
            }
        }
        self.set_plugin_state(plugin, state, cx);
    }

    /// Keep a language plugin loading while its first workspace server starts.
    pub(crate) fn begin_server_loading(&mut self, language_id: &str, cx: &mut Context<Self>) {
        if let Some(entry) = self
            .plugin_loads
            .iter_mut()
            .find(|entry| entry.plugin.language_id() == language_id)
        {
            entry.server_loading = true;
            if !matches!(
                entry.state,
                PluginLoadState::Error(_) | PluginLoadState::Disabled
            ) {
                entry.state = PluginLoadState::Loading;
            }
            cx.notify();
        }
    }

    /// Publish server readiness only after grammar validation also finishes.
    pub(crate) fn finish_server_loading(
        &mut self,
        language_id: &str,
        result: anyhow::Result<()>,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self
            .plugin_loads
            .iter_mut()
            .find(|entry| entry.plugin.language_id() == language_id)
        else {
            return;
        };
        entry.server_loading = false;
        let plugin = entry.plugin;
        let grammar_loaded = entry.grammar_loaded;
        let grammar_failed = matches!(entry.state, PluginLoadState::Error(_));
        match result {
            Ok(()) if grammar_loaded && !grammar_failed => {
                self.set_plugin_state(plugin, PluginLoadState::Enabled, cx)
            }
            Ok(()) => cx.notify(),
            Err(error) if !grammar_failed => {
                tracing::warn!(%error, "language server preparation failed");
                self.set_plugin_state(plugin, PluginLoadState::Error(format!("{error:#}")), cx);
            }
            Err(error) => tracing::warn!(%error, "language server preparation failed"),
        }
    }

    /// Refresh open buffers after a grammar becomes available to them.
    fn set_plugin_state(
        &mut self,
        plugin: language_plugins::BundledPlugin,
        state: PluginLoadState,
        cx: &mut Context<Self>,
    ) {
        let enabled = state == PluginLoadState::Enabled;
        if let Some(entry) = self
            .plugin_loads
            .iter_mut()
            .find(|entry| entry.plugin == plugin)
        {
            entry.state = state;
        }
        if enabled {
            self.activate_plugin_highlighting(plugin, cx);
        }
        if self.plugin_popup.is_some_and(|(kind, _)| {
            kind == PluginPopupKind::Loading && self.plugin_count(kind, cx) == 0
        }) {
            self.plugin_popup = None;
        }
        cx.notify();
    }

    /// Refresh existing editors once a plugin grammar is usable.
    fn activate_plugin_highlighting(
        &mut self,
        plugin: language_plugins::BundledPlugin,
        cx: &mut Context<Self>,
    ) {
        for tab in &self.tabs {
            if language_plugins::language_for_path(tab.session.path())
                .is_some_and(|language| language.id == plugin.language_id())
            {
                let language = plugin.language_id().to_owned();
                tab.editor
                    .update(cx, |editor, cx| editor.set_highlighter(language, cx));
            }
        }
    }

    pub(crate) fn plugin_count(&self, kind: PluginPopupKind, cx: &App) -> usize {
        let bundled = self
            .plugin_loads
            .iter()
            .filter(|entry| match kind {
                PluginPopupKind::Loading => entry.state == PluginLoadState::Loading,
                PluginPopupKind::Error => matches!(entry.state, PluginLoadState::Error(_)),
            })
            .count();
        let runtime = self.extensions.read(cx);
        bundled
            + match kind {
                PluginPopupKind::Loading => runtime.startup.len(),
                PluginPopupKind::Error => runtime
                    .entries
                    .iter()
                    .filter(|entry| entry.error.is_some())
                    .count(),
            }
    }

    /// Place the detail card above the pointer that activated its indicator.
    fn toggle_plugin_popup(
        &mut self,
        kind: PluginPopupKind,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.plugin_popup = match self.plugin_popup {
            Some((current, _)) if current == kind => None,
            _ => Some((kind, event.position)),
        };
        cx.notify();
    }

    /// Render the supplied loading and error artwork at the far right of the bar.
    pub(crate) fn render_plugin_indicator(
        &self,
        kind: PluginPopupKind,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let count = self.plugin_count(kind, cx);
        let (id, label) = match kind {
            PluginPopupKind::Loading => ("plugin-loading-indicator", t!("plugins.loading")),
            PluginPopupKind::Error => ("plugin-error-indicator", t!("plugins.error")),
        };
        h_flex()
            .id(id)
            .debug_selector(move || id.into())
            .items_center()
            .gap_1()
            .px_2()
            .rounded_sm()
            .hover(|style| style.bg(cx.theme().secondary_hover))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event, _, cx| this.toggle_plugin_popup(kind, event, cx)),
            )
            .child(match kind {
                PluginPopupKind::Loading => Spinner::new()
                    .icon(
                        Icon::default()
                            .data(include_bytes!("../../assets/plugin-status/loading.svg")),
                    )
                    .small()
                    .into_any_element(),
                PluginPopupKind::Error => Icon::default()
                    .data(include_bytes!("../../assets/plugin-status/error.svg"))
                    .small()
                    .into_any_element(),
            })
            .child(format!("{label} {count}"))
    }

    /// Show the selected plugin list at the click location, with failure reasons.
    pub(crate) fn render_plugin_popup(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let Some((kind, position)) = self.plugin_popup else {
            return div().into_any_element();
        };
        let width = px(340.).min(window.viewport_size().width - px(16.));
        let title = match kind {
            PluginPopupKind::Loading => t!("plugins.loading_list"),
            PluginPopupKind::Error => t!("plugins.error_list"),
        };
        // Runtime plugin names and startup failures share the existing status popup.
        let runtime = self.extensions.read(cx);
        let runtime_details: Vec<_> = match kind {
            PluginPopupKind::Loading => runtime
                .startup
                .values()
                .map(|name| (name.clone(), t!("plugins.loading").to_string()))
                .collect(),
            PluginPopupKind::Error => runtime
                .entries
                .iter()
                .filter_map(|entry| {
                    entry
                        .error
                        .as_ref()
                        .map(|error| (entry.manifest.name.clone(), error.clone()))
                })
                .collect(),
        };
        // GPUI measures the card before placing it above the clicked window point.
        gpui_base::Positioner::side(Bounds::new(position, size(px(1.), px(1.))))
            .placement(gpui_base::Placement::Top)
            .align(gpui_base::Align::End)
            .offset(px(8.))
            .margin(px(8.))
            .occlude()
            .child(
                v_flex()
                    .id("plugin-status-popup")
                    .debug_selector(|| "plugin-status-popup".into())
                    .w(width)
                    .max_h(px(210.))
                    .overflow_y_scroll()
                    .p_3()
                    .gap_2()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().popover)
                    .text_color(cx.theme().foreground)
                    .shadow_md()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(div().font_semibold().child(title.to_string()))
                    .children(self.plugin_loads.iter().filter_map(|entry| {
                        let detail = match (&entry.state, kind) {
                            (PluginLoadState::Loading, PluginPopupKind::Loading) => {
                                Some(t!("plugins.loading").to_string())
                            }
                            (PluginLoadState::Error(error), PluginPopupKind::Error) => {
                                Some(error.clone())
                            }
                            _ => None,
                        }?;
                        Some(
                            v_flex()
                                .gap_1()
                                .child(div().font_semibold().child(entry.plugin.name()))
                                .child(div().text_xs().child(detail)),
                        )
                    }))
                    .children(runtime_details.into_iter().map(|(name, detail)| {
                        v_flex()
                            .gap_1()
                            .child(div().font_semibold().child(name))
                            .child(div().text_xs().child(detail))
                    })),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{TestAppContext, component::Root, gpui, px, size};
    use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

    /// A previously installed runtime plugin must appear in the startup indicator.
    #[gpui::test]
    fn installed_runtime_plugin_is_visible_while_starting(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            typography::init(cx);
            apply_theme(builtin_theme(false), cx);
        });
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join(".runtime-plugin-test");
        std::fs::create_dir_all(&root).unwrap();
        let manifest = serde_json::from_str::<plugin_runtime::plugin_protocol::Manifest>(
            include_str!("../../../../plugins/terminal/manifest.json"),
        )
        .unwrap();
        let installed = plugin_runtime::Installed {
            manifest: manifest.clone(),
            digest: "fixture".into(),
            grants: manifest.permissions.clone(),
            enabled: true,
            project_enabled: Default::default(),
            global_enabled: None,
            error: None,
        };
        std::fs::write(
            root.join("registry.json"),
            serde_json::to_vec(&BTreeMap::from([(manifest.id, installed)])).unwrap(),
        )
        .unwrap();
        let workspace = Workspace::open(directory.path()).unwrap();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            Root::new(
                cx.new(|cx| EditorApp::new(workspace, None, window, cx)),
                window,
                cx,
            )
        });
        cx.simulate_resize(size(px(1000.), px(800.)));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            cx.debug_bounds("plugin-loading-indicator").is_some(),
            "an enabled runtime plugin must show startup loading in the status bar"
        );
    }

    /// No language reports loading until an installed package declares it.
    #[test]
    fn initial_state_requires_installed_plugins() {
        let directory = tempfile::tempdir().unwrap();
        extensions::contributions::refresh(directory.path()).unwrap();
        assert!(PluginLoadEntry::initial().is_empty());
    }

    /// Installed package enablement is the source of truth for language availability.
    #[test]
    fn missing_plugin_does_not_start_its_language_server() {
        let directory = tempfile::tempdir().unwrap();
        extensions::contributions::refresh(directory.path()).unwrap();
        assert!(PluginLoadEntry::initial().is_empty());
    }

    /// Both transient states expose clickable icons and anchored plugin lists.
    #[gpui::test]
    fn status_indicators_open_plugin_lists(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            typography::init(cx);
            apply_theme(builtin_theme(false), cx);
            cx.set_reduce_motion(true);
        });
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::open(directory.path()).unwrap();
        let view_slot = Rc::new(RefCell::new(None));
        let capture = view_slot.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let view = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
            *capture.borrow_mut() = Some(view.clone());
            Root::new(view, window, cx)
        });
        let view = view_slot.borrow_mut().take().unwrap();
        cx.run_until_parked();
        cx.simulate_resize(size(px(1000.), px(800.)));
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                // The status UI also renders failures from packages loaded after startup.
                app.plugin_loads.push(PluginLoadEntry {
                    plugin: language_plugins::BundledPlugin::Rust,
                    state: PluginLoadState::Loading,
                    grammar_loaded: false,
                    server_loading: false,
                });
                app.set_plugin_state(
                    language_plugins::BundledPlugin::Rust,
                    PluginLoadState::Error("missing grammar".to_owned()),
                    cx,
                );
            });
            window.draw(cx).clear(cx);
        });
        let indicator = cx
            .debug_bounds("plugin-error-indicator")
            .expect("plugin failure should have a status indicator");
        cx.simulate_click(indicator.center(), Default::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let error_popup = cx
            .debug_bounds("plugin-status-popup")
            .expect("clicking the error indicator should paint a popup");
        assert!(error_popup.origin.y + error_popup.size.height < indicator.center().y);

        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.set_plugin_state(
                    language_plugins::BundledPlugin::Rust,
                    PluginLoadState::Loading,
                    cx,
                );
            });
            window.draw(cx).clear(cx);
        });
        let loading = cx
            .debug_bounds("plugin-loading-indicator")
            .expect("loading plugin should have a spinning status indicator");
        cx.simulate_click(loading.center(), Default::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let popup = cx
            .debug_bounds("plugin-status-popup")
            .expect("clicking the loading indicator should paint a popup");
        // A painted popup must have content and sit just above the clicked point.
        assert!(
            popup.size.height > px(20.),
            "popup has no visible content: {popup:?}"
        );
        let gap = loading.center().y - (popup.origin.y + popup.size.height);
        assert!(
            (px(4.)..=px(16.)).contains(&gap),
            "popup is not just above the loading click: popup={popup:?}, indicator={loading:?}"
        );
        assert!(popup.origin.x <= loading.center().x);
        assert!(loading.center().x <= popup.origin.x + popup.size.width);
    }
}
