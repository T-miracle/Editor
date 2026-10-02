//! Hot LSP selection is independent of grammar loading and rebinds already-open document snapshots.
use crate::*;
impl EditorApp {
    pub(crate) fn sync_dynamic_language_servers(&mut self, cx: &mut Context<Self>) {
        let selected = crate::language::providers::language_servers();
        let available = self.extensions.read(cx).language_services();
        // Worker publications also carry ongoing transport failures, not only the initial handshake result.
        for server in self.language_servers.values() {
            if let Some(message) = server.recovery_status() {
                for (key, plan) in &available {
                    if let Ok(plan) = plan
                        && server.uses_service(plan)
                    {
                        self.extensions.update(cx, |panel, cx| {
                            panel.language_service_status(key, plan, message.clone(), cx)
                        });
                    }
                }
            }
        }
        let mut changed = false;
        self.language_servers.retain(|language, server| {
            let keep = if server.is_dynamic() {
                self.session_state.workspace_trusted
                    && selected
                        .get(language)
                        .and_then(|id| id.as_ref())
                        .and_then(|id| available.get(id))
                        .and_then(|service| service.as_ref().ok())
                        .is_some_and(|service| server.uses_service(service) && server.is_active())
            } else {
                !selected.contains_key(language)
            };
            if !keep {
                server.retire();
                changed = true;
            }
            keep
        });
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
                let service_key = id.expect("selected plan has an ID");
                self.extensions.update(cx, |extensions, cx| {
                    extensions.language_service_status(&service_key, &plan, "正在启动…".into(), cx)
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
                        {
                            let message = match result {
                                Ok(()) => "已就绪".to_owned(),
                                Err(error) => {
                                    app.status = format!("LSP {language}: {error:#}");
                                    format!("启动失败：{error:#}")
                                }
                            };
                            app.extensions.update(cx, |extensions, cx| {
                                extensions.language_service_status(&service_key, &plan, message, cx)
                            });
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
        }
        if changed {
            for tab in &self.tabs {
                editor::detach_language_server(&tab.editor, cx);
                let path = tab.session.path();
                if let Some(server) = self.language_servers.get(&editor::language_for_path(path)) {
                    editor::attach_language_server(
                        &tab.editor,
                        path,
                        server.clone(),
                        cx.entity().downgrade(),
                        cx,
                    );
                }
            }
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
