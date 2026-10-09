//! Coordinates document opening, tab activation, saving, and explorer selection.

use super::file_watch::{DiskContent, WatchedFile};
use crate::app::messages::MessageLevel;
use crate::*;

impl EditorApp {
    /// Layout changes retire source gestures and popovers before handing native focus to another surface.
    pub(crate) fn prepare_file_layout_change(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_text_drag(cx);
        self.dismiss_pointer_hover(cx);
        self.completion_popup.reset();
        window.blur(cx);
    }
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
        // Preserve click selection across refreshes even when it differs from the active tab.
        let selected_path = if !self.session_state.explorer_root_expanded {
            // Selecting a child would automatically expand a deliberately collapsed project root.
            Some(root.clone())
        } else {
            // An unselected tree stays unselected; the active tab may be in a collapsed subtree.
            self.tree_state
                .read(cx)
                .selected_item()
                .map(|item| PathBuf::from(item.id.as_str()))
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
                let Some(relative) = self.tabs[index].path().strip_prefix(old).ok() else {
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
                        .any(|(other, tab)| other != index && tab.path() == target)
                {
                    continue;
                }
                let previous = self.tabs[index].path().to_path_buf();
                self.close_language_document(&previous, cx);
                let file = &mut self.tabs[index];
                file.path = target.clone();
                file.file_revision = file.file_revision.saturating_add(1);
                if let Some(text) = &mut file.text {
                    text.session.rename(target.clone());
                    text.capability_revision = text.capability_revision.saturating_add(1);
                    renamed_editors.push((text.editor.clone(), target.clone()));
                }
                if self.active_path.as_ref() == Some(&previous) {
                    // The actor can finish before a shell repaint; renaming immediately revokes old consent paths.
                    self.withdraw_bundled_request(cx);
                    self.active_path = Some(target);
                }
                renamed = true;
            }
        }
        if renamed {
            for (editor, path) in renamed_editors {
                // A moved document's language providers must send the new file URI.
                detach_language_server(&editor, cx);
                if let Some(server) = self.language_servers.get(&language_for_path(&path)) {
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
                self.refresh_syntax_diagnostics(editor.entity_id(), cx);
            }
            self.sync_watched_documents();
            self.persist_session();
        }
        if let Some(snapshot) = update.snapshot {
            self.update_workspace_tree(snapshot, cx);
        }
        let mut preview_reloaded = false;
        for (path, disk_contents, read_at) in update.documents {
            if renamed_from.iter().any(|old| path.starts_with(old)) {
                continue;
            }
            let Some(index) = self.tabs.iter().position(|tab| tab.path() == path) else {
                continue;
            };
            let file = &mut self.tabs[index];
            if read_at < file.opened_at {
                continue;
            }
            if file.text.is_none() {
                let (digest, error) = match disk_contents {
                    Ok(DiskContent::FileDigest(digest)) => (Some(digest), None),
                    Err(error) => (None, Some(error.to_string())),
                    Ok(DiskContent::Text(_)) => continue,
                };
                if file.file_digest != digest || file.file_error != error {
                    file.file_digest = digest;
                    file.file_error = error;
                    file.file_revision = file.file_revision.saturating_add(1);
                    preview_reloaded |= self.active_path.as_ref() == Some(&path);
                    cx.notify();
                }
                continue;
            }
            let disk_contents = disk_contents.and_then(|contents| match contents {
                DiskContent::Text(text) => Ok(text),
                DiskContent::FileDigest(_) => Err(std::io::ErrorKind::InvalidData.into()),
            });
            let Some(tab) = file.text.as_mut() else {
                continue;
            };
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
                    // Silent disk reloads still invalidate plugin snapshots and notify subscriptions.
                    tab.capability_revision = tab.capability_revision.saturating_add(1);
                    tab.disk_digest = digest;
                    // set_value is silent and keeps the clean revision, so refresh its preview explicitly.
                    preview_reloaded |= self.active_path.as_ref() == Some(&path);
                    DiskState::Synced
                }
            };
            if new_state != tab.disk_state {
                tab.overwrite_confirmed = false;
                tab.disk_state = new_state;
                let message = match new_state {
                    DiskState::Synced => t!("status.disk_updated").to_string(),
                    DiskState::Conflict => t!("status.disk_conflict").to_string(),
                    DiskState::Deleted => t!("status.disk_deleted").to_string(),
                };
                if self.active_path.as_ref() == Some(&path) {
                    self.status = message.clone();
                }
                // Retain abnormal transitions once, including background tabs; recovery stays quiet.
                if new_state != DiskState::Synced {
                    self.record_host_message(
                        MessageLevel::Warning,
                        format!("{}: {message}", path.display()),
                        cx,
                    );
                }
                cx.notify();
            }
        }
        if preview_reloaded {
            self.invalidate_editor_previews(cx);
            self.sync_editor_previews(cx);
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
                .filter(|tab| tab.virtual_document.is_none())
                .map(|tab| WatchedFile {
                    path: tab.path().to_path_buf(),
                    text: tab.text.is_some(),
                })
                .collect(),
        );
    }

    pub(crate) fn persist_session(&mut self) {
        self.session_state.open_tabs = self
            .tabs
            .iter()
            .filter(|tab| tab.virtual_document.is_none())
            .map(|tab| tab.path().to_string_lossy().into_owned())
            .collect();
        self.session_state.active_file = self
            .active_path
            .as_ref()
            .filter(|path| {
                self.tabs
                    .iter()
                    .any(|tab| tab.path() == path.as_path() && tab.virtual_document.is_none())
            })
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
        // Explorer and other explicit file navigation retain their reveal behavior.
        self.open_file_with_navigation(path, true, true, window, cx);
    }

    /// Apply explicit reveal/focus policies to existing tabs and newly opened documents alike.
    fn open_file_with_navigation(
        &mut self,
        path: PathBuf,
        reveal: bool,
        focus_editor: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = path.canonicalize().unwrap_or(path);
        if let Some(index) = self.tabs.iter().position(|tab| tab.path() == path) {
            self.activate_tab_with_navigation(index, reveal, focus_editor, window, cx);
            return;
        }

        if self.file_requires_readonly_view(&path, cx) {
            self.tabs.push(OpenTab {
                virtual_document: None,
                path,
                file_id: NEXT_FILE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                file_revision: 0,
                opened_at: Instant::now(),
                file_digest: None,
                text: None,
                file_error: None,
            });
            self.sync_watched_documents();
            self.activate_tab_with_navigation(
                self.tabs.len() - 1,
                reveal,
                focus_editor,
                window,
                cx,
            );
            cx.notify();
            return;
        }

        match DocumentSession::open(&self.file_store, path.clone()) {
            Ok(opened) => {
                let document_path = opened.session.path().to_path_buf();
                let language = language_for_path(&document_path);
                let text = self.create_native_text_tab(opened, window, cx);
                self.tabs.push(OpenTab {
                    virtual_document: None,
                    path: document_path,
                    file_id: NEXT_FILE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                    file_revision: 0,
                    opened_at: Instant::now(),
                    file_digest: None,
                    file_error: None,
                    text: Some(text),
                });
                self.sync_watched_documents();
                self.activate_tab_with_navigation(
                    self.tabs.len() - 1,
                    reveal,
                    focus_editor,
                    window,
                    cx,
                );
                self.refresh_syntax_diagnostics(self.editor.entity_id(), cx);
                let editor = self.editor.downgrade();
                // Start highlighting only after the loaded text has painted once.
                window.on_next_frame(move |_, cx| {
                    let _ = editor.update(cx, |editor, cx| editor.set_highlighter(language, cx));
                });
            }
            Err(error) => {
                // Unknown binary encodings still have a file identity, even without an installed viewer.
                if matches!(&error, editor_core::DocumentError::Read { source, .. } if source.kind() == std::io::ErrorKind::InvalidData)
                {
                    self.tabs.push(OpenTab {
                        virtual_document: None,
                        path,
                        file_id: NEXT_FILE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                        file_revision: 0,
                        opened_at: Instant::now(),
                        file_digest: None,
                        text: None,
                        file_error: None,
                    });
                    self.sync_watched_documents();
                    self.activate_tab_with_navigation(
                        self.tabs.len() - 1,
                        reveal,
                        focus_editor,
                        window,
                        cx,
                    );
                    cx.notify();
                    return;
                }
                // Preserve the immediate failure while retaining it after the status changes.
                self.report_host_message(
                    MessageLevel::Error,
                    t!("status.open_failed", error = error.to_string()).to_string(),
                    cx,
                );
            }
        }
        cx.notify();
    }

    pub(crate) fn active_tab_index(&self) -> Option<usize> {
        let path = self.active_path.as_ref()?;
        self.tabs.iter().position(|tab| tab.path() == path)
    }

    /// Editing commands require the current file's own native session, never a retained background editor.
    pub(crate) fn active_text_tab_index(&self) -> Option<usize> {
        self.active_tab_index()
            .filter(|index| self.tabs[*index].text.is_some())
    }

    /// Access native text only after narrowing a file's capability; binary files return None.
    pub(crate) fn text_tab(&self, index: usize) -> Option<&TextTab> {
        self.tabs.get(index)?.text.as_ref()
    }
    /// Mutable access does not manufacture a session for a read-only file.
    pub(crate) fn text_tab_mut(&mut self, index: usize) -> Option<&mut TextTab> {
        self.tabs.get_mut(index)?.text.as_mut()
    }
    pub(crate) fn active_text_revision(&self) -> Option<u64> {
        self.text_tab(self.active_text_tab_index()?)
            .map(|text| text.session.revision())
    }

    /// Provider metadata chooses opaque files before decoding text. Generic image signature recognition
    /// also preserves a file tab when no viewer is installed; no extension or plugin ID is built in.
    fn file_requires_readonly_view(&self, path: &Path, cx: &App) -> bool {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if self.extensions.read(cx).entries.iter().any(|entry| {
            entry.manifest.panels.iter().any(|panel| {
                panel
                    .readonly_file_extensions
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(extension))
            })
        }) {
            return true;
        }
        use std::io::Read;
        let mut prefix = [0_u8; 32];
        std::fs::File::open(path)
            .and_then(|mut file| file.read(&mut prefix))
            .is_ok_and(|count| crate::ui::plugin::bitmap::recognizes_encoding(&prefix[..count]))
    }

    /// Opens a debugger's source location and places the caret on the line it stopped at.
    ///
    /// A debug provider reports a source and a one-based line, not an LSP position, so this follows
    /// the same reveal and centering path a definition jump uses without inventing a URI for it. A
    /// source that does not name an existing local file is reported as not located: the provider's
    /// word for a path is not evidence that the file is here, and guessing one would put the caret
    /// somewhere the target never stopped.
    pub(crate) fn open_debug_location(
        &mut self,
        source: &str,
        line: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let path = std::path::PathBuf::from(source);
        // A provider may report a path relative to the workspace it launched in.
        let path = if path.is_absolute() {
            path
        } else {
            self.workspace.root().join(path)
        };
        if !path.is_file() || line == 0 {
            return false;
        }
        let path = path.canonicalize().unwrap_or(path);
        // Source navigation updates the document without taking keyboard focus from the debug
        // panel. Consecutive frame keys and step shortcuts must stay in the user's active surface.
        let previous_focus = window.focused(cx);
        self.open_file_with_navigation(path.clone(), true, false, window, cx);
        if self.active_path.as_deref() != Some(path.as_path()) {
            return false;
        }
        // File-only viewers have no text session to lend to debugger source navigation.
        let Some(tab) = self
            .active_text_tab_index()
            .and_then(|index| self.text_tab(index))
        else {
            return false;
        };
        let editor = tab.editor.clone();
        let revision = tab.session.revision();
        let generation = tab.definition_highlight_generation;
        let position = editor.update(cx, |editor, cx| {
            let row = (line - 1) as usize;
            // A line beyond the file the provider named is refused rather than clamped: the target
            // did not stop there, and putting the caret at the end would claim it did.
            let offset = editor.text().line_start_offset(row);
            let position = editor.text().offset_to_position(offset);
            // A line inside a fold must be exposed before the caret can be shown at it.
            editor.unfold_at(position, cx);
            editor.set_cursor_position(position, window, cx);
            let _ = center_editor_cursor(editor, cx);
            position
        });
        // A newly opened tab has no layout yet, so its centering is revisited until it has painted.
        let app = cx.entity().downgrade();
        reveal_definition_after_layout(
            app, editor, position, revision, generation, false, 2, window,
        );
        // Base cursor positioning focuses the input immediately; restore the caller's focus once.
        // The navigation policy above schedules no later focus, so this cannot be overwritten.
        if let Some(focus) = previous_focus {
            focus.focus(window, cx);
        }
        true
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
        // Definition jumps follow the same explorer preference as tab switching.
        self.open_file_with_navigation(
            path.clone(),
            self.session_state.explorer_reveal_on_tab_switch,
            true,
            window,
            cx,
        );
        // Opening can fail; do not move the caret in the previously active file.
        if self.active_path.as_deref() != Some(path.as_path()) {
            return false;
        }
        let (Some(index), Some(selection)) = (self.active_text_tab_index(), selection) else {
            return true;
        };
        let Some(text) = self.tabs[index].text.as_ref() else {
            return false;
        };
        let editor = text.editor.clone();
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

        let Some(tab) = self.tabs[index].text.as_mut() else {
            return false;
        };
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
                        .filter_map(|file| file.text.as_ref())
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
        // Ordinary switches follow the saved preference; explicit navigation supplies its own policy.
        self.activate_tab_with_navigation(
            index,
            self.session_state.explorer_reveal_on_tab_switch,
            true,
            window,
            cx,
        );
    }

    /// Activate document content while keeping explorer reveal and keyboard focus caller choices.
    fn activate_tab_with_navigation(
        &mut self,
        index: usize,
        reveal: bool,
        focus_editor: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Platform input may still be awaiting semantic ranges; enter it into its original editor
        // before changing the active entity. Its Change subscription retains that document's revision.
        self.finish_linked_input(window, cx);
        if self
            .language_edits
            .bridge
            .as_ref()
            .is_some_and(|bridge| bridge.read(cx).has_pending())
        {
            let Some(path) = self.tabs.get(index).map(|tab| tab.path().to_path_buf()) else {
                return;
            };
            // Native history dispatch is deferred outside the input borrow. Keep its source mounted
            // until it drains, then find the destination again because tab indices may have changed.
            let owner = cx.entity().downgrade();
            window.defer(cx, move |window, cx| {
                let _ = owner.update(cx, |app, cx| {
                    if let Some(index) = app.tabs.iter().position(|tab| tab.path() == path) {
                        app.activate_tab_with_navigation(index, reveal, focus_editor, window, cx);
                    }
                });
            });
            return;
        }
        // A selection gesture belongs to one uninterrupted visit to its document.
        self.cancel_text_drag(cx);
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        let path = tab.path().to_path_buf();
        let language = tab
            .virtual_document
            .as_ref()
            .map(|virtual_tab| virtual_tab.language.clone())
            .unwrap_or_else(|| language_for_path(&path));
        let file_name = tab.title();
        let editor = tab.text.as_ref().map(|text| text.editor.clone());
        let file_id = tab.file_id;

        if editor.as_ref() != Some(&self.editor)
            || self.active_path.as_deref() != Some(path.as_path())
        {
            // Revoke queued first-use installation before a different native document becomes active.
            // Re-activating this same document preserves a permission dialog that is already on screen.
            self.withdraw_bundled_request(cx);
        }
        self.active_path = Some(path.clone());
        if let Some(editor) = editor {
            self.editor = editor;
        } else {
            // A binary file has no input target; retain background text state without lending it focus.
            window.blur(cx);
            self.dismiss_pointer_hover(cx);
        }
        // A completion index belongs to one document and must not cross tabs.
        self.completion_popup.reset();
        // Input interception follows the new editor entity; ranges from another tab never carry over.
        self.sync_linked_input(cx);
        // Session restoration preserves saved directory states instead of revealing each tab.
        if reveal && !self.restoring_documents && path.starts_with(self.workspace.root()) {
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
        // Debugger source updates preserve inspection focus; explicit navigation focuses editing.
        if focus_editor && self.active_text_tab_index().is_some() {
            let focus = self.editor.focus_handle(cx);
            let app = cx.entity().downgrade();
            window.defer(cx, move |window, cx| {
                let _ = app.update(cx, |app, cx| {
                    if app
                        .active_tab_index()
                        .is_some_and(|index| app.tabs[index].file_id == file_id)
                        && app.active_text_tab_index().is_some()
                        // Captured file/tool popups own keys until dismissal, even after a target switch.
                        && app.file_view_menu.is_none()
                        && app.tool_overflow.is_none()
                    {
                        focus.focus(window, cx);
                    }
                });
            });
        }
        self.status = t!("status.opened", file_name = file_name, language = language).to_string();
        self.persist_session();
        self.sync_plugin_documents(cx);
        cx.notify();
    }

    pub(crate) fn close_tab(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_path.as_ref() == Some(&path)
            && self
                .language_edits
                .bridge
                .as_ref()
                .is_some_and(|bridge| bridge.read(cx).has_pending())
        {
            // Base emits Change while replaying platform commands. Check dirty state only after
            // that event reaches DocumentSession, otherwise a newly typed clean tab could be lost.
            self.finish_linked_input(window, cx);
            let owner = cx.entity().downgrade();
            window.defer(cx, move |window, cx| {
                let _ = owner.update(cx, |app, cx| app.close_tab(path, window, cx));
            });
            return;
        }
        let Some(index) = self.tabs.iter().position(|tab| tab.path() == path) else {
            return;
        };
        if self.tabs[index].is_dirty() {
            // A refused user operation needs attention; normal tab activation does not enter history.
            self.report_host_message(
                MessageLevel::Warning,
                t!("status.save_before_close").to_string(),
                cx,
            );
            cx.notify();
            return;
        }

        self.remove_tab(index, path, window, cx);
    }

    /// Discard a replaced tab only after the caller validates explicit consent against its current revision.
    pub(crate) fn discard_tab(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.tabs.iter().position(|tab| tab.path() == path) {
            self.remove_tab(index, path, window, cx);
        }
    }

    /// Saved close and approved discard share document ownership, language and focus cleanup.
    pub(super) fn remove_tab(
        &mut self,
        index: usize,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let was_active = self.active_path.as_ref() == Some(&path);
        if was_active {
            // Retire the active tab's ownership before any background cutover can race its removal.
            self.withdraw_bundled_request(cx);
        }
        self.close_language_document(&path, cx);
        self.sync_plugin_documents(cx);
        if let Some(virtual_tab) = &self.tabs[index].virtual_document {
            virtual_tab.resource.revoke();
        }
        self.tabs.remove(index);
        self.sync_watched_documents();
        if !was_active {
            self.persist_session();
            self.sync_plugin_documents(cx);
            cx.notify();
            return;
        }

        self.active_path = None;
        if !self.tabs.is_empty() {
            self.activate_tab(index.min(self.tabs.len() - 1), window, cx);
        } else {
            // Retire document interactions before mounting the non-editable empty canvas.
            self.cancel_text_drag(cx);
            self.dismiss_pointer_hover(cx);
            self.completion_popup.reset();
            window.blur(cx);
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
            self.sync_plugin_documents(cx);
            // Dock panels cache their rendered content independently of the app shell.
            self.editor_panel.update(cx, |_, cx| cx.notify());
            cx.notify();
        }
    }

    pub(crate) fn move_tab_before(&mut self, source: &Path, target: &Path, cx: &mut Context<Self>) {
        let Some(source_index) = self.tabs.iter().position(|tab| tab.path() == source) else {
            return;
        };
        let Some(tab) = self.tabs.get(source_index) else {
            return;
        };
        if tab.path() == target {
            return;
        }

        let tab = self.tabs.remove(source_index);
        let target_index = self
            .tabs
            .iter()
            .position(|tab| tab.path() == target)
            .unwrap_or(self.tabs.len());
        self.tabs.insert(target_index, tab);
        self.persist_session();
        cx.notify();
    }

    pub(crate) fn save_current(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.active_text_tab_index() else {
            // Ordinary saves, including no-ops, remain internal state rather than notifications.
            self.status = t!("status.nothing_to_save").to_string();
            cx.notify();
            return;
        };
        if self.tabs[index].virtual_document.is_some() {
            self.report_host_message(
                MessageLevel::Warning,
                t!("status.readonly_document").to_string(),
                cx,
            );
            cx.notify();
            return;
        }
        let Some(tab) = self.tabs[index].text.as_mut() else {
            return;
        };
        if self.plugin_saves.contains(tab.path()) {
            // Serialize user and plugin saves so a delayed plugin snapshot cannot overwrite a newer save.
            self.status = "此文档正在保存，请稍候".into();
            cx.notify();
            return;
        }
        if !tab.session.is_dirty() && tab.disk_state != DiskState::Deleted {
            self.status = t!("status.no_changes_to_save").to_string();
            cx.notify();
            return;
        }

        if tab.disk_state == DiskState::Synced {
            // A save can precede its native event; compare the disk before overwriting it.
            let path = tab.path();
            match std::fs::read(path) {
                Ok(bytes) if Sha256::digest(&bytes).as_slice() != tab.disk_digest => {
                    tab.disk_state = DiskState::Conflict;
                    tab.overwrite_confirmed = true;
                    self.report_host_message(
                        MessageLevel::Warning,
                        t!("status.confirm_disk_overwrite").to_string(),
                        cx,
                    );
                    cx.notify();
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    tab.disk_state = DiskState::Deleted;
                    tab.overwrite_confirmed = true;
                    self.report_host_message(
                        MessageLevel::Warning,
                        t!("status.confirm_disk_restore").to_string(),
                        cx,
                    );
                    cx.notify();
                    return;
                }
                Err(error) => {
                    self.report_host_message(
                        MessageLevel::Error,
                        t!("status.save_failed", error = error.to_string()).to_string(),
                        cx,
                    );
                    cx.notify();
                    return;
                }
                Ok(_) => {}
            }
        }

        if tab.disk_state != DiskState::Synced && !tab.overwrite_confirmed {
            // Saving again is an explicit overwrite confirmation for a disk conflict or deletion.
            tab.overwrite_confirmed = true;
            let message = match tab.disk_state {
                DiskState::Conflict => t!("status.confirm_disk_overwrite").to_string(),
                DiskState::Deleted => t!("status.confirm_disk_restore").to_string(),
                DiskState::Synced => unreachable!(),
            };
            self.report_host_message(MessageLevel::Warning, message, cx);
            cx.notify();
            return;
        }

        let save_path = tab.path().to_path_buf();
        self.publish_document_save(index, None, cx);
        let tab = self.tabs[index]
            .text
            .as_mut()
            .expect("save target remains open");
        let value = self.editor.read(cx).value().to_string();
        if let Some(history) = &self.history {
            let _ = history.snapshot_file(&save_path);
        }
        let saved = tab.session.save(&self.file_store, &value);
        let event_result = saved.as_ref().map(|_| ()).map_err(|error| {
            plugin_runtime::plugin_protocol::api::Failure::new(
                plugin_runtime::plugin_protocol::api::ErrorCode::OperationFailed,
                error.to_string(),
            )
        });
        match saved {
            Ok(()) => {
                tab.disk_digest = Sha256::digest(value.as_bytes()).into();
                tab.last_saved_at = Instant::now();
                tab.disk_state = DiskState::Synced;
                tab.overwrite_confirmed = false;
                let path = tab.path().to_path_buf();
                // A successful routine save stays quiet; the document session still owns the write.
                self.status = t!("status.saved", path = path.display()).to_string();
                self.notify_language_document_saved(&path, value, cx);
            }
            Err(error) => {
                self.report_host_message(
                    MessageLevel::Error,
                    t!("status.save_failed", error = error.to_string()).to_string(),
                    cx,
                );
            }
        }
        self.publish_document_save(index, Some(event_result), cx);
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
                || app
                    .active_text_tab_index()
                    .and_then(|index| app.text_tab(index))
                    .is_none_or(|tab| {
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
    let completion_owner = app.clone();
    let editor_id = editor.entity_id();
    editor.update(cx, |editor, _| {
        let Some(provider) =
            language_navigation::LanguageDefinitionProvider::new(document_path, server.clone())
        else {
            return;
        };
        editor.lsp_mut().show_document = Some(Rc::new(
            move |params: &lsp_types::ShowDocumentParams, window: &mut Window, cx: &mut App| {
                // Only accept local files the host can open; other URIs retain native handling.
                let Ok(url) = url::Url::parse(params.uri.as_str()) else {
                    return false;
                };
                let Ok(path) = url.to_file_path() else {
                    return false;
                };
                if !path.is_file() || app.upgrade().is_none() {
                    return false;
                }
                let app = app.clone();
                let params = params.clone();
                // Ctrl-click and native definition actions call this while the source editor
                // is being updated. Release that entity before opening or moving its caret.
                window.defer(cx, move |window, cx| {
                    let _ = app.update(cx, |app, cx| {
                        app.open_definition_uri(&params.uri, params.selection, window, cx);
                    });
                });
                true
            },
        ));
        editor.lsp_mut().definition_provider = Some(Rc::new(provider));
        // Native hover and completion controls use the same server connection.
        editor.lsp_mut().hover_provider =
            crate::language::hover::LanguageHoverProvider::new(document_path, server.clone())
                .map(|provider| Rc::new(provider) as _);
        editor.lsp_mut().completion_provider =
            crate::language::completion::LanguageCompletionProvider::for_editor(
                document_path,
                server,
                completion_owner,
                editor_id,
            )
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

/// Resolve recognition consistently for opening, renaming and hot highlighter replacement.
pub(crate) fn language_for_path(path: &Path) -> String {
    if let Some(language) = crate::language::providers::language_for_path(path) {
        return language;
    }
    if crate::language::providers::handles_path(path) {
        return "text".into();
    }
    // Recognition is exclusively provided by installed declarations; upstream defaults stay inert.
    "text".to_owned()
}

/// Centers the painted caret using upstream viewport APIs; scrolling is clamped by the editor.
fn center_editor_cursor(editor: &mut EditorState, cx: &mut Context<EditorState>) -> Option<bool> {
    let (cursor, line_height) = editor.cursor_layout()?;
    let bounds = editor.input_bounds();
    let offset = editor.scroll_offset();
    // The caret layout omits vertical scrolling, while text_bounds moves with the
    // content. Compare the painted caret against the fixed viewport in screen coordinates.
    let delta = bounds.center().y - (cursor.top() + offset.y + line_height / 2.);
    if delta.abs() < px(1.) {
        return Some(true);
    }
    editor.set_scroll_offset(point(offset.x, (offset.y + delta).min(px(0.))), cx);
    Some(false)
}
