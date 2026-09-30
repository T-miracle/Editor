//! Coordinates document opening, tab activation, saving, and explorer selection.

use crate::*;

impl EditorApp {
    pub(crate) fn refresh_files(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.workspace.snapshot();
        let count = snapshot.files.len();
        self.update_workspace_tree(snapshot, cx);
        self.status = t!("status.workspace_refreshed", count = count).to_string();
        cx.notify();
    }

    /// Rebuild only after the worker reports a changed file or directory set.
    fn update_workspace_tree(&mut self, snapshot: WorkspaceSnapshot, cx: &mut Context<Self>) {
        if self.workspace_snapshot.as_ref() == Some(&snapshot) {
            return;
        }
        let root = self.workspace.root().to_path_buf();
        let items = restore_expanded(
            tree_items(
                &root,
                snapshot
                    .files
                    .iter()
                    .map(|file| file.absolute_path.as_path()),
                snapshot.directories.iter().map(PathBuf::as_path),
            ),
            &self.session_state.expanded_directories,
        )
        .into_iter()
        // Restore the project root separately so an older session still opens it by default.
        .map(|item| item.expanded(self.session_state.explorer_root_expanded))
        .collect::<Vec<_>>();
        // External tabs keep the last project file highlighted across a tree refresh.
        let selected_path = match self.active_path.as_ref() {
            // Selecting a child would automatically expand a deliberately collapsed project root.
            _ if !self.session_state.explorer_root_expanded => Some(root.clone()),
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
        self.workspace_snapshot = Some(snapshot);
        cx.notify();
    }

    /// Apply a completed background scan without replacing unsaved editor text.
    pub(crate) fn apply_reconciliation(
        &mut self,
        update: Reconciliation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let renamed_from: Vec<_> = update.renames.iter().map(|(old, _)| old.clone()).collect();
        let mut renamed = false;
        let mut renamed_editors = Vec::new();
        for (old, new) in &update.renames {
            // A paired directory rename transfers descendants; ambiguous targets stay put.
            for index in 0..self.tabs.len() {
                let Some(relative) = self.tabs[index].session.path().strip_prefix(old).ok() else {
                    continue;
                };
                let target = if relative.as_os_str().is_empty() {
                    new.clone()
                } else {
                    new.join(relative)
                };
                if !target.is_file()
                    || self
                        .tabs
                        .iter()
                        .enumerate()
                        .any(|(other, tab)| other != index && tab.session.path() == target)
                {
                    continue;
                }
                let previous = self.tabs[index].session.path().to_path_buf();
                self.tabs[index].session.rename(target.clone());
                renamed_editors.push((self.tabs[index].editor.clone(), target.clone()));
                if self.active_path.as_ref() == Some(&previous) {
                    self.active_path = Some(target);
                }
                renamed = true;
            }
        }
        if renamed {
            for (editor, path) in renamed_editors {
                // A moved document's language providers must send the new file URI.
                detach_language_server(&editor, cx);
                if let Some(contribution) = language_plugins::language_for_path(&path)
                    .filter(|contribution| self.language_plugin_enabled(&contribution.id))
                    && let Some(server) = self.language_servers.get(&contribution.id)
                {
                    attach_language_server(
                        &editor,
                        &path,
                        server.clone(),
                        cx.entity().downgrade(),
                        cx,
                    );
                }
                editor.update(cx, |editor, cx| {
                    editor.set_highlighter(language_for_path(&path), cx)
                });
            }
            self.sync_watched_documents();
            self.persist_session();
        }
        if let Some(snapshot) = update.snapshot {
            self.update_workspace_tree(snapshot, cx);
        }
        for (path, disk_contents, read_at) in update.documents {
            if renamed_from.iter().any(|old| path.starts_with(old)) {
                continue;
            }
            let Some(index) = self.tabs.iter().position(|tab| tab.session.path() == path) else {
                continue;
            };
            let tab = &mut self.tabs[index];
            if read_at < tab.last_saved_at {
                continue;
            }
            let new_state = match disk_contents {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => DiskState::Deleted,
                Err(error) => {
                    tracing::warn!(%error, path = %path.display(), "open document could not be checked");
                    continue;
                }
                Ok(contents)
                    if Sha256::digest(contents.as_bytes()).as_slice() == tab.disk_digest =>
                {
                    DiskState::Synced
                }
                Ok(_contents) if tab.session.is_dirty() => DiskState::Conflict,
                Ok(contents) => {
                    // Programmatic reload must not mark the clean tab as a user edit.
                    let digest = Sha256::digest(contents.as_bytes()).into();
                    tab.suppress_change = true;
                    tab.editor
                        .update(cx, |editor, cx| editor.set_value(contents, window, cx));
                    tab.suppress_change = false;
                    tab.disk_digest = digest;
                    DiskState::Synced
                }
            };
            if new_state != tab.disk_state {
                tab.overwrite_confirmed = false;
                tab.disk_state = new_state;
                if self.active_path.as_ref() == Some(&path) {
                    self.status = match new_state {
                        DiskState::Synced => t!("status.disk_updated").to_string(),
                        DiskState::Conflict => t!("status.disk_conflict").to_string(),
                        DiskState::Deleted => t!("status.disk_deleted").to_string(),
                    };
                }
                cx.notify();
            }
        }
        if !update.native {
            tracing::warn!(
                "native file watcher unavailable; adaptive background scanning is active"
            );
        }
        if self.status == t!("status.refreshing_workspace").to_string() {
            let count = self
                .workspace_snapshot
                .as_ref()
                .map_or(0, |snapshot| snapshot.files.len());
            self.status = t!("status.workspace_refreshed", count = count).to_string();
            cx.notify();
        }
    }

    /// A parent directory is watched for each document outside the workspace.
    fn sync_watched_documents(&self) {
        self.file_watch.set_documents(
            self.tabs
                .iter()
                .map(|tab| tab.session.path().to_path_buf())
                .collect(),
        );
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

    pub(crate) fn select_file_in_tree(&mut self, path: &Path, cx: &mut Context<Self>) {
        // Revealing a project file expands its root; retain that state on the next disk refresh.
        if path.starts_with(self.workspace.root()) && !self.session_state.explorer_root_expanded {
            self.session_state.explorer_root_expanded = true;
            self.session_state.save();
        }
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
            // Keep the selected file centered when navigation reveals it in the explorer.
            if let Some(index) = state.selected_index() {
                state.scroll_to_item(index, ScrollStrategy::Center);
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
                let language = language_for_path(opened.session.path());
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
                let disk_digest = Sha256::digest(contents.as_bytes()).into();
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
                });
                if let Some(server) = server {
                    // All tabs for a plugin language share its workspace server.
                    attach_language_server(&editor, &document_path, server, app, cx);
                }
                // Definition markers use their own layer beside plugin syntax highlighting.
                let definition_highlight = editor.update(cx, |editor, cx| {
                    editor.create_decorations_collection(Vec::new(), cx)
                });
                if let Some(server) = newly_created_server {
                    // Keep the plugin loading through the handshake and first workspace analysis.
                    self.begin_server_loading(&language, cx);
                    let server_language = language.clone();
                    cx.spawn_in(window, async move |this, cx| {
                        let loading_server = server.clone();
                        let result = cx
                            .background_executor()
                            .scheduler_executor()
                            .spawn_dedicated(move |_| async move { server.prepare_until_ready() })
                            .await;
                        let _ = this.update_in(cx, |app, _, cx| {
                            app.finish_server_loading(
                                &server_language,
                                &loading_server,
                                result,
                                cx,
                            );
                        });
                    })
                    .detach();
                }
                let subscription =
                    cx.subscribe(&editor, |this, changed_editor, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change) {
                            // Each tab keeps its own revision, including background edits.
                            if let Some(index) = this.tabs.iter().position(|tab| {
                                tab.editor.entity_id() == changed_editor.entity_id()
                            }) {
                                let tab = &mut this.tabs[index];
                                if tab.suppress_change {
                                    return;
                                }
                                tab.session.note_edit();
                                if this.editor.entity_id() == changed_editor.entity_id() {
                                    this.status =
                                        t!("status.modified", revision = tab.session.revision())
                                            .to_string();
                                }
                            }
                            cx.notify();
                        }
                    });
                // Host-owned popovers follow upstream menu and hover notifications.
                let panel = self.editor_panel.downgrade();
                let observer = cx.observe(&editor, move |this, changed_editor, cx| {
                    if this.editor.entity_id() == changed_editor.entity_id() {
                        let _ = panel.update(cx, |_, cx| cx.notify());
                    }
                });
                self.tabs.push(OpenTab {
                    session: opened.session,
                    editor,
                    disk_digest,
                    last_saved_at: Instant::now(),
                    disk_state: DiskState::Synced,
                    suppress_change: false,
                    overwrite_confirmed: false,
                    definition_highlight,
                    definition_highlight_generation: 0,
                    _subscription: subscription,
                    _observer: observer,
                });
                self.sync_watched_documents();
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
        selection: Option<lsp_types::Range>,
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
        let path = path.canonicalize().unwrap_or(path);
        self.open_file(path.clone(), window, cx);
        // Opening can fail; do not move the caret in the previously active file.
        if self.active_path.as_deref() != Some(path.as_path()) {
            return false;
        }
        let (Some(index), Some(selection)) = (self.active_tab_index(), selection) else {
            return true;
        };
        let editor = self.tabs[index].editor.clone();
        let (cursor_position, highlight_range) = editor.update(cx, |editor, cx| {
            // LSP columns use UTF-16, while editor decorations use UTF-8 byte offsets.
            let start = lsp_position_to_offset(editor.text(), selection.start);
            let end = lsp_position_to_offset(editor.text(), selection.end);
            let cursor_position = editor.text().offset_to_position(start);
            let highlight_range = if end > start {
                start..end
            } else {
                editor
                    .text()
                    .word_range(start)
                    .or_else(|| {
                        start
                            .checked_sub(1)
                            .and_then(|at| editor.text().word_range(at))
                    })
                    .unwrap_or(start..start)
            };
            // A folded definition must be exposed before moving the caret to it.
            editor.unfold_at(cursor_position, cx);
            editor.set_cursor_position(cursor_position, window, cx);
            // Existing tabs can center immediately; new tabs wait for layout below.
            let _ = center_editor_cursor(editor, cx);
            (cursor_position, highlight_range)
        });

        let tab = &mut self.tabs[index];
        tab.definition_highlight_generation = tab.definition_highlight_generation.wrapping_add(1);
        let generation = tab.definition_highlight_generation;
        let marker = tab.definition_highlight.clone();
        let editor_id = editor.entity_id();
        let has_highlight = !highlight_range.is_empty();
        let decorations = has_highlight
            .then(|| {
                TextDecoration::new(
                    highlight_range,
                    HighlightStyle {
                        background_color: Some(cx.theme().selection),
                        ..Default::default()
                    },
                )
            })
            .into_iter()
            .collect();
        marker.set(decorations, cx);

        // A newly opened editor may need more than one frame to acquire its layout.
        let app = cx.entity().downgrade();
        let target_revision = tab.session.revision();
        reveal_definition_after_layout(
            app,
            editor,
            cursor_position,
            target_revision,
            generation,
            false,
            12,
            window,
        );

        if has_highlight {
            // Earlier timers cannot clear a newer jump in the same document.
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                let _ = this.update_in(cx, |app, _, cx| {
                    if let Some(tab) = app
                        .tabs
                        .iter()
                        .find(|tab| tab.editor.entity_id() == editor_id)
                        && tab.definition_highlight_generation == generation
                    {
                        marker.clear(cx);
                    }
                });
            })
            .detach();
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
        // A completion index belongs to one document and must not cross tabs.
        self.completion_selection.set((0, 0));
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
        self.sync_watched_documents();
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
        if !self.tabs[index].session.is_dirty() && self.tabs[index].disk_state != DiskState::Deleted
        {
            self.status = t!("status.no_changes_to_save").to_string();
            cx.notify();
            return;
        }

        if self.tabs[index].disk_state == DiskState::Synced {
            // A save can precede its native event; compare the disk before overwriting it.
            let path = self.tabs[index].session.path();
            match std::fs::read(path) {
                Ok(bytes) if Sha256::digest(&bytes).as_slice() != self.tabs[index].disk_digest => {
                    self.tabs[index].disk_state = DiskState::Conflict;
                    self.tabs[index].overwrite_confirmed = true;
                    self.status = t!("status.confirm_disk_overwrite").to_string();
                    cx.notify();
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    self.tabs[index].disk_state = DiskState::Deleted;
                    self.tabs[index].overwrite_confirmed = true;
                    self.status = t!("status.confirm_disk_restore").to_string();
                    cx.notify();
                    return;
                }
                Err(error) => {
                    self.status = t!("status.save_failed", error = error.to_string()).to_string();
                    cx.notify();
                    return;
                }
                Ok(_) => {}
            }
        }

        if self.tabs[index].disk_state != DiskState::Synced && !self.tabs[index].overwrite_confirmed
        {
            // Saving again is an explicit overwrite confirmation for a disk conflict or deletion.
            self.tabs[index].overwrite_confirmed = true;
            self.status = match self.tabs[index].disk_state {
                DiskState::Conflict => t!("status.confirm_disk_overwrite").to_string(),
                DiskState::Deleted => t!("status.confirm_disk_restore").to_string(),
                DiskState::Synced => unreachable!(),
            };
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
                tab.disk_digest = Sha256::digest(value.as_bytes()).into();
                tab.last_saved_at = Instant::now();
                tab.disk_state = DiskState::Synced;
                tab.overwrite_confirmed = false;
                self.status = t!("status.saved", path = tab.session.path().display()).to_string();
            }
            Err(error) => {
                self.status = t!("status.save_failed", error = error.to_string()).to_string()
            }
        }
        cx.notify();
    }
}

/// Revisit a definition jump until its displayed row reaches the viewport center.
fn reveal_definition_after_layout(
    app: WeakEntity<EditorApp>,
    target_editor: Entity<EditorState>,
    position: lsp_types::Position,
    revision: u64,
    generation: u64,
    has_centered_once: bool,
    remaining_frames: u8,
    window: &mut Window,
) {
    window.on_next_frame(move |window, cx| {
        let mut needs_another_frame = false;
        let mut centered_this_frame = false;
        let _ = app.update(cx, |app, cx| {
            // A later jump, edit, or tab switch must cancel this pending reveal.
            if app.editor.entity_id() != target_editor.entity_id()
                || app.active_tab_index().is_none_or(|index| {
                    let tab = &app.tabs[index];
                    tab.session.revision() != revision
                        || tab.definition_highlight_generation != generation
                })
            {
                return;
            }

            // Leave a user's later caret move in place instead of restoring the old jump.
            if target_editor.read(cx).cursor_position() != position {
                return;
            }
            let centered = target_editor.update(cx, |editor, cx| center_editor_cursor(editor, cx));
            match centered {
                Some(already_centered) => {
                    // Verify on the following painted frame, including after unfolding.
                    centered_this_frame = true;
                    needs_another_frame = !already_centered || !has_centered_once;
                }
                None => {
                    // A new tab cannot center the cursor until it has painted once.
                    needs_another_frame = true;
                    window.refresh();
                }
            }
        });
        if needs_another_frame && remaining_frames > 0 {
            reveal_definition_after_layout(
                app,
                target_editor,
                position,
                revision,
                generation,
                has_centered_once || centered_this_frame,
                remaining_frames - 1,
                window,
            );
        }
    });
}

/// Convert an LSP UTF-16 line/column to the editor's UTF-8 byte offset.
fn lsp_position_to_offset(text: &gpui_base::input::Rope, position: lsp_types::Position) -> usize {
    let line = text.slice_line(position.line as usize);
    let column = (position.character as usize).min(line.len_utf16());
    text.line_start_offset(position.line as usize) + line.utf16_to_byte_idx(column)
}

/// Attach a shared workspace server to a newly opened or newly supported document.
pub(crate) fn attach_language_server(
    editor: &Entity<EditorState>,
    document_path: &Path,
    server: Arc<language_navigation::LanguageServer>,
    app: WeakEntity<EditorApp>,
    cx: &mut Context<EditorApp>,
) {
    editor.update(cx, |editor, _| {
        let Some(provider) =
            language_navigation::LanguageDefinitionProvider::new(document_path, server.clone())
        else {
            return;
        };
        editor.lsp_mut().show_document = Some(Rc::new(
            move |params: &lsp_types::ShowDocumentParams, window: &mut Window, cx: &mut App| {
                app.update(cx, |app, cx| {
                    app.open_definition_uri(&params.uri, params.selection, window, cx)
                })
                .unwrap_or(false)
            },
        ));
        editor.lsp_mut().definition_provider = Some(Rc::new(provider));
        // Native hover and completion controls use the same server connection.
        editor.lsp_mut().hover_provider =
            crate::language::hover::LanguageHoverProvider::new(document_path, server.clone())
                .map(|provider| Rc::new(provider) as _);
        editor.lsp_mut().completion_provider =
            crate::language::completion::LanguageCompletionProvider::new(document_path, server)
                .map(|provider| Rc::new(provider) as _);
    });
}

/// Remove providers when their package is disabled so stale servers cannot answer requests.
pub(crate) fn detach_language_server(editor: &Entity<EditorState>, cx: &mut Context<EditorApp>) {
    editor.update(cx, |editor, _| {
        editor.lsp_mut().show_document = None;
        editor.lsp_mut().definition_provider = None;
        editor.lsp_mut().hover_provider = None;
        editor.lsp_mut().completion_provider = None;
    });
}

fn language_for_path(path: &Path) -> String {
    // Installed plugin manifests own their extensions and language identities.
    if let Some(language) = language_plugins::language_for_path(path) {
        return language.id;
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
    .to_owned()
}

/// Centers the painted caret using upstream viewport APIs; scrolling is clamped by the editor.
fn center_editor_cursor(editor: &mut EditorState, cx: &mut Context<EditorState>) -> Option<bool> {
    let (cursor, line_height) = editor.cursor_layout()?;
    let bounds = editor.text_bounds()?;
    let offset = editor.scroll_offset();
    let delta = bounds.center().y - (cursor.top() + line_height / 2.);
    if delta.abs() < px(1.) {
        return Some(true);
    }
    editor.set_scroll_offset(point(offset.x, (offset.y + delta).min(px(0.))), cx);
    Some(false)
}
