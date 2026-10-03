//! Hot LSP selection is independent of grammar loading and rebinds already-open document snapshots.
use crate::*;

/// Readiness belongs to the selected service instance, independently from its grammar provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ServiceLoadState {
    Loading,
    Ready,
    Failed(String),
}

impl EditorApp {
    /// Only the active server may complete its startup; retired results cannot overwrite replacements.
    pub(crate) fn finish_server_loading(
        &mut self,
        language: &str,
        server: &Arc<language_navigation::LanguageServer>,
        result: anyhow::Result<()>,
        cx: &mut Context<Self>,
    ) {
        // A cancelled provider may still be present in the last UI map until worker publication arrives.
        if !server.is_active()
            || !self
                .language_servers
                .get(language)
                .is_some_and(|active| Arc::ptr_eq(active, server))
        {
            return;
        }
        self.language_service_states.insert(
            language.to_owned(),
            match result {
                Ok(()) => ServiceLoadState::Ready,
                Err(error) => ServiceLoadState::Failed(format!("{error:#}")),
            },
        );
        if self.plugin_popup.is_some_and(|(kind, _)| {
            kind == PluginPopupKind::Loading && self.plugin_count(kind, cx) == 0
        }) {
            self.plugin_popup = None;
        }
        cx.notify();
    }

    pub(crate) fn sync_dynamic_language_servers(&mut self, cx: &mut Context<Self>) {
        let selected = crate::language::providers::language_servers();
        let available = self.extensions.read(cx).language_services();
        // Worker publications also carry ongoing transport failures, not only the initial handshake result.
        for server in self.language_servers.values() {
            if let Some(recovery) = server.recovery_state()
                && let Some(message) = server.recovery_status()
            {
                let level = match recovery {
                    language_navigation::RecoveryState::WaitingRetry => {
                        plugin_runtime::LogLevel::Warning
                    }
                    language_navigation::RecoveryState::Paused => plugin_runtime::LogLevel::Error,
                    language_navigation::RecoveryState::Recovered => plugin_runtime::LogLevel::Info,
                };
                for (key, plan) in &available {
                    if let Ok(plan) = plan
                        && server.uses_service(plan)
                    {
                        self.extensions.update(cx, |panel, cx| {
                            panel.language_service_status(key, plan, level, message.clone(), cx)
                        });
                    }
                }
            }
        }
        let mut changed = false;
        self.language_servers.retain(|language, server| {
            let keep = self.session_state.workspace_trusted
                && selected
                    .get(language)
                    .and_then(|id| id.as_ref())
                    .and_then(|id| available.get(id))
                    .and_then(|service| service.as_ref().ok())
                    .is_some_and(|service| server.uses_service(service) && server.is_active());
            if !keep {
                server.retire();
                changed = true;
            }
            keep
        });
        self.language_service_states
            .retain(|language, _| self.language_servers.contains_key(language));
        if self.session_state.workspace_trusted {
            for (language, id) in selected {
                if self.language_servers.contains_key(&language) {
                    continue;
                }
                let Some(plan) = id.as_ref().and_then(|id| available.get(id)) else {
                    continue;
                };
                let plan = match plan {
                    Ok(plan) => plan.clone(),
                    Err(error) => {
                        self.status = format!("LSP {language}: {error}");
                        self.language_service_states
                            .insert(language.clone(), ServiceLoadState::Failed(error.clone()));
                        continue;
                    }
                };
                let Some(server) =
                    language_navigation::LanguageServer::from_service(plan.clone()).map(Arc::new)
                else {
                    continue;
                };
                self.language_servers
                    .insert(language.clone(), server.clone());
                changed = true;
                self.language_service_states
                    .insert(language.clone(), ServiceLoadState::Loading);
                let service_key = id.expect("selected plan has an ID");
                self.extensions.update(cx, |extensions, cx| {
                    extensions.language_service_status(
                        &service_key,
                        &plan,
                        plugin_runtime::LogLevel::Info,
                        t!("plugins.logs.lsp_starting").to_string(),
                        cx,
                    )
                });
                cx.spawn(async move |this, cx| {
                    let current = server.clone();
                    let result = cx
                        .background_executor()
                        .scheduler_executor()
                        .spawn_dedicated(move |_| async move { server.prepare_until_ready() })
                        .await;
                    let _ = this.update(cx, |app, cx| {
                        if app
                            .language_servers
                            .get(&language)
                            .is_some_and(|server| Arc::ptr_eq(server, &current))
                            // Cancellation can finish before the UI receives the retired provider snapshot.
                            && current.is_active()
                        {
                            let (level, message) = match &result {
                                Ok(()) => (
                                    plugin_runtime::LogLevel::Info,
                                    t!("plugins.logs.lsp_ready").to_string(),
                                ),
                                Err(error) => {
                                    app.status = format!("LSP {language}: {error:#}");
                                    (plugin_runtime::LogLevel::Error, format!("{error:#}"))
                                }
                            };
                            app.finish_server_loading(&language, &current, result, cx);
                            app.extensions.update(cx, |extensions, cx| {
                                extensions.language_service_status(
                                    &service_key,
                                    &plan,
                                    level,
                                    message,
                                    cx,
                                )
                            });
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
        }
        let mut documents_changed = false;
        for tab in &self.tabs {
            let path = tab.session.path();
            let language = editor::language_for_path(path);
            let previous = tab.editor.read(cx).language_name().to_string();
            if !changed && previous == language {
                continue;
            }
            // Recognition can change without replacing any service; revoke old per-document authority first.
            if previous != language {
                if let Some(server) = self.language_servers.get(&previous) {
                    Self::close_server_document(server.clone(), path, cx);
                }
            }
            documents_changed = true;
            editor::detach_language_server(&tab.editor, cx);
            if let Some(server) = self.language_servers.get(&language) {
                editor::attach_language_server(
                    &tab.editor,
                    path,
                    server.clone(),
                    cx.entity().downgrade(),
                    cx,
                );
            }
        }
        if changed || documents_changed {
            self.reset_syntax_diagnostics(cx);
            cx.notify();
        }
    }
    /// didClose is delivered on the transport executor, without blocking tab removal on the UI thread.
    pub(crate) fn close_language_document(&self, path: &Path, cx: &mut Context<Self>) {
        let Some(server) = self
            .language_servers
            .get(&editor::language_for_path(path))
            .cloned()
        else {
            return;
        };
        Self::close_server_document(server, path, cx);
    }

    /// Closing a tab and changing its recognizer share synchronous lease revocation and asynchronous wire cleanup.
    fn close_server_document(
        server: Arc<language_navigation::LanguageServer>,
        path: &Path,
        cx: &mut Context<Self>,
    ) {
        let Some(uri) = language_navigation::file_uri(path) else {
            return;
        };
        let Some(document) = server.retire_document(&uri) else {
            return;
        };
        cx.spawn(async move |_, cx| {
            let _ = cx
                .background_executor()
                .scheduler_executor()
                .spawn_dedicated(move |_| async move { server.close_document(document) })
                .await;
        })
        .detach();
    }
}
