//! Coordinates document opening, tab activation, saving, and explorer selection.

use crate::*;

impl EditorApp {
    pub(crate) fn refresh_files(&mut self, cx: &mut Context<Self>) {
        let root = self.workspace.root().to_path_buf();
        let files = self.workspace.files();
        let items = restore_expanded(
            tree_items(&root, files.iter().map(|file| file.absolute_path.as_path())),
            &self.session_state.expanded_directories,
        );
        // External tabs keep the last project file highlighted across a tree refresh.
        let selected_path = match self.active_path.as_ref() {
            Some(path) if path.starts_with(self.workspace.root()) => Some(path.clone()),
            Some(_) => self
                .tree_state
                .read(cx)
                .selected_item()
                .map(|item| PathBuf::from(item.id.as_str())),
            None => None,
        };
        let selected_item = selected_path
            .as_ref()
            .and_then(|path| find_tree_item(&items, path))
            .cloned();
        self.tree_state.update(cx, |state, cx| {
            state.set_items(items, cx);
            state.set_selected_item(selected_item.as_ref(), cx);
        });
        self.status = t!("status.workspace_refreshed", count = files.len()).to_string();
        cx.notify();
    }

    pub(crate) fn persist_session(&mut self) {
        self.session_state.open_tabs = self
            .tabs
            .iter()
            .map(|tab| tab.session.path().to_string_lossy().into_owned())
            .collect();
        self.session_state.active_file = self
            .active_path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned());
        self.session_state.explorer_visible = self.explorer_visible;
        self.session_state.save();
    }

    pub(crate) fn select_file_in_tree(&self, path: &Path, cx: &mut Context<Self>) {
        let id = path.to_string_lossy().into_owned();
        // A tree click already selected its row; avoid another id lookup.
        let already_selected = self
            .tree_state
            .read(cx)
            .selected_item()
            .is_some_and(|item| item.id.as_str() == id);
        // TreeState resolves ids against its current items and expands ancestors as needed.
        // Rebuilding the workspace tree here blocks the editor on every file switch.
        let selected = TreeItem::new(id, "");
        self.tree_state.update(cx, |state, cx| {
            if !already_selected {
                state.set_selected_item(Some(&selected), cx);
            }
            // Nearest reveals tab and definition targets without moving a clicked visible row.
            if let Some(index) = state.selected_index() {
                state.scroll_to_item(index, ScrollStrategy::Nearest);
                cx.notify();
            }
        });
    }

    pub(crate) fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let path = path.canonicalize().unwrap_or(path);
        if let Some(index) = self.tabs.iter().position(|tab| tab.session.path() == path) {
            self.activate_tab(index, window, cx);
            return;
        }

        match DocumentSession::open(&self.file_store, path) {
            Ok(opened) => {
                let language = language_for_path(opened.session.path()).to_string();
                let mut newly_created_server = None;
                // Plugin manifests supply the language server for every matching source file.
                let server = language_plugins::language_for_path(opened.session.path())
                    .filter(|contribution| self.language_plugin_enabled(&contribution.id))
                    .filter(|contribution| contribution.lsp_command.is_some())
                    .and_then(|contribution| {
                        if let Some(server) = self.language_servers.get(&contribution.id) {
                            return Some(server.clone());
                        }
                        let server = Arc::new(language_navigation::LanguageServer::new(
                            self.workspace.root(),
                            contribution.clone(),
                        )?);
                        self.language_servers
                            .insert(contribution.id.clone(), server.clone());
                        newly_created_server = Some(server.clone());
                        Some(server)
                    });
                let document_path = opened.session.path().to_path_buf();
                let contents = opened.contents;
                let editor = cx.new(|cx| {
                    EditorState::new(window, cx)
                        // Keep the first render free of parser work while loading the file.
                        .language("text")
                        .line_number(true)
                        .indent_guides(true)
                        .folding(true)
                        .tab_size(TabSize {
                            tab_size: 4,
                            hard_tabs: false,
                        })
                });
                let app = cx.entity().downgrade();
                editor.update(cx, |editor, cx| {
                    editor.set_value(contents, window, cx);
                    // All tabs for a plugin language share its workspace server.
                    if let Some(server) = server
                        && let Some(provider) = language_navigation::LanguageDefinitionProvider::new(
                            &document_path,
                            server.clone(),
                        )
                    {
                        let app = app.clone();
                        editor.lsp_mut().show_document = Some(Rc::new(
                            move |params: &lsp_types::ShowDocumentParams,
                                  window: &mut Window,
                                  cx: &mut App| {
                                let target_position =
                                    params.selection.as_ref().map(|range| range.start);
                                app.update(cx, |app, cx| {
                                    app.open_definition_uri(
                                        &params.uri,
                                        target_position,
                                        window,
                                        cx,
                                    )
                                })
                                .unwrap_or(false)
                            },
                        ));
                        editor.lsp_mut().definition_provider = Some(Rc::new(provider));
                        // The plugin's hover response renders through GPUI Kit's popover.
                        if let Some(provider) = crate::language::hover::LanguageHoverProvider::new(
                            &document_path,
                            server.clone(),
                        ) {
                            editor.lsp_mut().hover_provider = Some(Rc::new(provider));
                        }
                        // The plugin's LSP also supplies the editor's native completion menu.
                        if let Some(provider) = crate::language::completion::LanguageCompletionProvider::new(
                            &document_path,
                            server,
                        ) {
                            editor.lsp_mut().completion_provider = Some(Rc::new(provider));
                        }
                    }
                });
                if let Some(server) = newly_created_server {
                    // Keep the plugin loading through the handshake and first workspace analysis.
                    self.begin_server_loading(&language, cx);
                    let server_language = language.clone();
                    cx.spawn_in(window, async move |this, cx| {
                        let result = cx
                            .background_executor()
                            .scheduler_executor()
                            .spawn_dedicated(move |_| async move { server.prepare_until_ready() })
                            .await;
                        let _ = this.update_in(cx, |app, _, cx| {
                            app.finish_server_loading(&server_language, result, cx);
                        });
                    })
                    .detach();
                }
                let subscription =
                    cx.subscribe(&editor, |this, changed_editor, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change)
                            && this.editor.entity_id() == changed_editor.entity_id()
                        {
                            if let Some(index) = this.active_tab_index() {
                                let tab = &mut this.tabs[index];
                                tab.session.note_edit();
                                this.status =
                                    t!("status.modified", revision = tab.session.revision())
                                        .to_string();
                            }
                            cx.notify();
                        }
                    });
                self.tabs.push(OpenTab {
                    session: opened.session,
                    editor,
                    _subscription: subscription,
                });
                self.activate_tab(self.tabs.len() - 1, window, cx);
                let editor = self.tabs.last().unwrap().editor.downgrade();
                // Start highlighting only after the loaded text has painted once.
                window.on_next_frame(move |_, cx| {
                    let _ = editor.update(cx, |editor, cx| editor.set_highlighter(language, cx));
                });
            }
            Err(error) => {
                self.status = t!("status.open_failed", error = error.to_string()).to_string()
            }
        }
        cx.notify();
    }

    pub(crate) fn active_tab_index(&self) -> Option<usize> {
        let path = self.active_path.as_ref()?;
        self.tabs.iter().position(|tab| tab.session.path() == path)
    }

    /// Opens local LSP targets, including sources in the Cargo registry and sysroot.
    pub(crate) fn open_definition_uri(
        &mut self,
        uri: &lsp_types::Uri,
        position: Option<lsp_types::Position>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Ok(url) = url::Url::parse(uri.as_str()) else {
            return false;
        };
        let Ok(path) = url.to_file_path() else {
            return false;
        };
        if !path.is_file() {
            return false;
        }
        self.open_file(path, window, cx);
        if let (Some(index), Some(position)) = (self.active_tab_index(), position) {
            let editor = self.tabs[index].editor.clone();
            editor.update(cx, |editor, cx| {
                editor.set_cursor_position(position, window, cx);
            });
        }
        true
    }

    pub(crate) fn activate_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        let path = tab.session.path().to_path_buf();
        let language = language_for_path(&path);
        let file_name = tab
            .session
            .file_name()
            .map(str::to_owned)
            .unwrap_or_else(|_| t!("editor.untitled").to_string());

        self.active_path = Some(path.clone());
        self.editor = tab.editor.clone();
        // Every activation path, including tabs and definition jumps, updates the explorer.
        if path.starts_with(self.workspace.root()) {
            self.select_file_in_tree(&path, cx);
        }
        // Keep the selected tab fully visible when opening or switching files.
        let measured_viewport = self.tabs_scroll.bounds().size.width;
        let viewport = if measured_viewport > px(0.) {
            measured_viewport
        } else {
            (window.bounds().size.width - px(EXPLORER_INITIAL_WIDTH) - px(24.)).max(px(0.))
        };
        let tab_left = px(190.) * index;
        let tab_right = tab_left + px(190.);
        let current = -self.tabs_scroll.offset().x;
        let target = if tab_left < current {
            tab_left
        } else if tab_right > current + viewport {
            tab_right - viewport
        } else {
            current
        };
        let max_scroll = (px(190.) * self.tabs.len() - viewport).max(px(0.));
        self.tabs_scroll
            .set_offset(point(-target.clamp(px(0.), max_scroll), px(0.)));
        let focus = self.editor.focus_handle(cx);
        window.defer(cx, move |window, cx| focus.focus(window, cx));
        self.status = t!("status.opened", file_name = file_name, language = language).to_string();
        self.persist_session();
        cx.notify();
    }

    pub(crate) fn close_tab(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.session.path() == path) else {
            return;
        };
        if self.tabs[index].session.is_dirty() {
            self.status = t!("status.save_before_close").to_string();
            cx.notify();
            return;
        }

        let was_active = self.active_path.as_ref() == Some(&path);
        self.tabs.remove(index);
        if !was_active {
            self.persist_session();
            cx.notify();
            return;
        }

        self.active_path = None;
        if !self.tabs.is_empty() {
            self.activate_tab(index.min(self.tabs.len() - 1), window, cx);
        } else {
            self.editor = cx.new(|cx| {
                EditorState::new(window, cx)
                    .language("text".to_string())
                    .line_number(true)
                    .indent_guides(true)
                    .folding(true)
                    .tab_size(TabSize {
                        tab_size: 4,
                        hard_tabs: false,
                    })
            });
            self.status = t!("status.no_open_files").to_string();
            self.persist_session();
            cx.notify();
        }
    }

    pub(crate) fn move_tab_before(&mut self, source: &Path, target: &Path, cx: &mut Context<Self>) {
        let Some(source_index) = self
            .tabs
            .iter()
            .position(|tab| tab.session.path() == source)
        else {
            return;
        };
        let Some(tab) = self.tabs.get(source_index) else {
            return;
        };
        if tab.session.path() == target {
            return;
        }

        let tab = self.tabs.remove(source_index);
        let target_index = self
            .tabs
            .iter()
            .position(|tab| tab.session.path() == target)
            .unwrap_or(self.tabs.len());
        self.tabs.insert(target_index, tab);
        self.persist_session();
        cx.notify();
    }

    pub(crate) fn save_current(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.active_tab_index() else {
            self.status = t!("status.nothing_to_save").to_string();
            cx.notify();
            return;
        };
        if !self.tabs[index].session.is_dirty() {
            self.status = t!("status.no_changes_to_save").to_string();
            cx.notify();
            return;
        }

        let value = self.editor.read(cx).value().to_string();
        if let Some(history) = &self.history {
            let _ = history.snapshot_file(self.tabs[index].session.path());
        }
        let tab = &mut self.tabs[index];
        match tab.session.save(&self.file_store, &value) {
            Ok(()) => {
                self.status = t!("status.saved", path = tab.session.path().display()).to_string();
            }
            Err(error) => {
                self.status = t!("status.save_failed", error = error.to_string()).to_string()
            }
        }
        cx.notify();
    }
}

fn language_for_path(path: &Path) -> &'static str {
    // Bundled plugin manifests own their extensions and language identities.
    if let Some(language) = language_plugins::language_for_path(path) {
        return &language.id;
    }
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
    {
        "js" | "jsx" => "javascript",
        "ts" | "tsx" => "typescript",
        "vue" => "vue",
        "html" | "htm" => "html",
        "css" => "css",
        "json" => "json",
        "md" | "markdown" => "markdown",
        "yaml" | "yml" => "yaml",
        _ => "text",
    }
}
