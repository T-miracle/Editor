//! Native editor implementation of typed capability requests.
use super::*;
use plugin_runtime::{
    EditorRequest,
    plugin_protocol::api::{
        self, DocumentVersion, EditorOperation as Op, EditorValue as Value, ErrorCode, Failure,
    },
};

mod edits;
mod images;

impl EditorApp {
    /// Drain requests in effect order so each edit's native Change event advances its revision first.
    pub(crate) fn dispatch_editor_requests(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor_request_dispatch_scheduled || self.pending_editor_requests.is_empty() {
            return;
        }
        // A redraw may occur while effects are queued. Keep it from executing a second stale request.
        self.editor_request_dispatch_scheduled = true;
        let (plugin, request) = self.pending_editor_requests.remove(0);
        self.perform_editor_request(&plugin, request, window, cx);
        // Native EditorState remains the only text and undo owner. Its Change subscription and the
        // request completion were queued above; the next request observes their committed version.
        cx.defer_in(window, |this, window, cx| {
            this.editor_request_dispatch_scheduled = false;
            this.dispatch_editor_requests(window, cx);
        });
    }

    /// Publish snapshots of changed versions; intermediate typing events can safely coalesce.
    pub(crate) fn sync_plugin_documents(&mut self, cx: &mut Context<Self>) {
        if !self.session_state.workspace_trusted {
            self.plugin_documents.clear();
            return;
        }
        let current = (0..self.tabs.len())
            .filter_map(|index| self.plugin_document_version(index).ok())
            .map(|document| (document.id.clone(), document))
            .collect::<BTreeMap<_, _>>();
        let worker = self.extensions.read(cx).worker.clone();
        let mut state = worker.state.lock().unwrap();
        for (id, document) in &current {
            if self.plugin_documents.get(id) != Some(document) {
                state.document_events.push(api::DocumentChange {
                    document: document.clone(),
                    closed: false,
                });
            }
        }
        for (id, document) in &self.plugin_documents {
            if !current.contains_key(id) {
                let mut document = document.clone();
                document.revision = document.revision.saturating_add(1);
                state.document_events.push(api::DocumentChange {
                    document,
                    closed: true,
                });
            }
        }
        self.plugin_documents = current;
    }

    /// Tokens use the open entity identity, not merely a path that may later be reopened.
    pub(crate) fn plugin_document_version(&self, index: usize) -> Result<DocumentVersion, Failure> {
        let tab = &self.tabs[index];
        // OpenTab already stores its resolved path. Disk deletion does not end the editor entity's lifetime.
        let path = tab
            .session
            .path()
            .strip_prefix(self.workspace.root())
            .map_err(|_| {
                Failure::new(
                    ErrorCode::PermissionDenied,
                    "Document is outside the owning workspace",
                )
            })?;
        Ok(DocumentVersion {
            id: format!("{:?}", tab.editor.entity_id()),
            path: path.to_string_lossy().replace('\\', "/"),
            revision: tab.capability_revision,
        })
    }

    /// Runtime checks precede publication; the editor rechecks current trust and workspace at execution.
    pub(crate) fn perform_editor_request(
        &mut self,
        plugin: &str,
        request: EditorRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !request.begin() {
            return;
        }
        let same_workspace = Path::new(request.workspace())
            .canonicalize()
            .is_ok_and(|path| path == self.workspace.root());
        if !self.session_state.workspace_trusted || !same_workspace {
            request.finish(Err(Failure::new(
                ErrorCode::PermissionDenied,
                "Workspace authority has been revoked",
            )));
            return;
        }
        if let Op::SaveDocument { document } = request.operation() {
            self.save_plugin_document(document.clone(), request, cx);
            return;
        }
        if matches!(request.operation(), Op::ReplaceDocumentRange { .. }) {
            self.replace_plugin_document(request, window, cx);
            return;
        }
        if matches!(request.operation(), Op::SaveImageInput { .. }) {
            self.save_plugin_image(request, window, cx);
            return;
        }
        let result = (|| match request.operation() {
            Op::OpenDataFile { path } => {
                // The owning runtime supplies this root. Resolve aliases again immediately before opening.
                let root = request
                    .data_root()
                    .canonicalize()
                    .map_err(|error| Failure::new(ErrorCode::NotFound, error.to_string()))?;
                let file = root
                    .join(path)
                    .canonicalize()
                    .map_err(|error| Failure::new(ErrorCode::NotFound, error.to_string()))?;
                if !file.starts_with(&root) || !file.is_file() {
                    return Err(Failure::new(
                        ErrorCode::PermissionDenied,
                        "File escaped private data",
                    ));
                }
                if !request.enter_side_effect() {
                    return Err(Failure::new(
                        ErrorCode::Cancelled,
                        "Private file open cancelled",
                    ));
                }
                self.open_file(file.clone(), window, cx);
                if !self.tabs.iter().any(|tab| tab.session.path() == file) {
                    return Err(Failure::new(
                        ErrorCode::OperationFailed,
                        "Private file could not be opened",
                    ));
                }
                Ok(Value::Unit)
            }
            Op::ReadClipboard => {
                let text = cx
                    .read_from_clipboard()
                    .and_then(|item| item.text())
                    .unwrap_or_default();
                if text.len() > 1024 * 1024 {
                    return Err(Failure::new(
                        ErrorCode::LimitExceeded,
                        "Clipboard exceeds 1 MiB",
                    ));
                }
                Ok(Value::Clipboard { text })
            }
            Op::WriteClipboard { text } => {
                // Retired or cancelled requests must not overwrite a newer user's clipboard.
                if !request.enter_side_effect() {
                    return Err(Failure::new(
                        ErrorCode::Cancelled,
                        "Clipboard write cancelled",
                    ));
                }
                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                Ok(Value::Unit)
            }
            Op::ReadSelection => {
                let index = self
                    .active_tab_index()
                    .ok_or_else(|| Failure::new(ErrorCode::NotFound, "No active document"))?;
                let document = self.plugin_document_version(index)?;
                let text = self.tabs[index].editor.read(cx).selected_text().to_string();
                if text.len() > 1024 * 1024 {
                    return Err(Failure::new(
                        ErrorCode::LimitExceeded,
                        "Selection exceeds 1 MiB",
                    ));
                }
                Ok(Value::Selection { document, text })
            }
            Op::ReadDocumentSelection { document } => {
                self.read_plugin_document_selection(document, window, cx)
            }
            Op::ActiveDirectory => {
                let path = if let Some(index) = self.active_tab_index() {
                    let document = self.plugin_document_version(index)?;
                    Path::new(&document.path)
                        .parent()
                        .unwrap_or(Path::new(""))
                        .to_string_lossy()
                        .into_owned()
                } else {
                    String::new()
                };
                Ok(Value::Directory { path })
            }
            Op::SetPanelVisibility { panel, visible } => {
                let key = format!("{plugin}/{panel}");
                let owner = self.plugin_panels.get(&key).cloned().ok_or_else(|| {
                    Failure::new(ErrorCode::NotFound, "Owned panel is unavailable")
                })?;
                if !request.enter_side_effect() {
                    return Err(Failure::new(
                        ErrorCode::Cancelled,
                        "Panel request cancelled",
                    ));
                }
                if *visible {
                    owner.update(cx, |owner, cx| owner.show(window, cx));
                    self.session_state.plugin_panel_visibility.insert(key, true);
                    self.persist_session();
                } else {
                    self.hide_plugin_panel(plugin, panel, cx);
                }
                self.dock_area.update(cx, |_, cx| cx.notify());
                cx.notify();
                Ok(Value::PanelVisibility {
                    panel: panel.clone(),
                    visible: *visible,
                })
            }
            Op::SaveDocument { .. }
            | Op::ReplaceDocumentRange { .. }
            | Op::SaveImageInput { .. } => unreachable!(),
        })();
        request.finish(result);
    }

    /// Save an immutable snapshot through DocumentSession; do not copy or mutate a second editor model.
    fn save_plugin_document(
        &mut self,
        document: DocumentVersion,
        request: EditorRequest,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = (0..self.tabs.len()).find(|index| {
            self.plugin_document_version(*index)
                .is_ok_and(|version| version == document)
        }) else {
            request.finish(Err(Failure::new(
                ErrorCode::StaleRevision,
                "Document was edited, renamed or closed",
            )));
            return;
        };
        let tab = &self.tabs[index];
        let path = tab.session.path().to_path_buf();
        if let Err(error) = self.check_plugin_save_path(&path) {
            request.finish(Err(error));
            return;
        }
        if tab.disk_state != DiskState::Synced || !self.plugin_saves.insert(path.clone()) {
            request.finish(Err(Failure::new(
                ErrorCode::Conflict,
                "Document changed on disk or another save is active",
            )));
            return;
        }
        let value = tab.editor.read(cx).value().to_string();
        let digest = tab.disk_digest;
        let history = self.history.clone();
        let saved_path = path.clone();
        cx.spawn(async move |app, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let disk = std::fs::read(&saved_path)
                        .map_err(|e| Failure::new(ErrorCode::Conflict, e.to_string()))?;
                    if Sha256::digest(&disk).as_slice() != digest {
                        return Err(Failure::new(
                            ErrorCode::Conflict,
                            "File changed on disk; explicit user resolution required",
                        ));
                    }
                    if let Some(history) = history {
                        let _ = history.snapshot_file(&saved_path);
                    }
                    platform_windows::PreparedFileStore::prepare(saved_path, value)
                        .map_err(|e| Failure::new(ErrorCode::OperationFailed, e.to_string()))
                })
                .await;
            let _ = app.update(cx, |app, cx| {
                app.plugin_saves.remove(&path);
                let result = result
                    .and_then(|prepared| app.commit_plugin_save(&document, &request, prepared, cx));
                request.finish(result);
                cx.notify();
            });
        })
        .detach();
    }

    /// Recheck authority, identity, revision and disk immediately before the atomic commit on the UI thread.
    fn commit_plugin_save(
        &mut self,
        document: &DocumentVersion,
        request: &EditorRequest,
        prepared: platform_windows::PreparedFileStore,
        cx: &mut Context<Self>,
    ) -> Result<Value, Failure> {
        if !self.session_state.workspace_trusted {
            return Err(Failure::new(
                ErrorCode::PermissionDenied,
                "Workspace trust revoked",
            ));
        }
        let index = (0..self.tabs.len())
            .find(|index| {
                self.plugin_document_version(*index)
                    .is_ok_and(|version| version == *document)
            })
            .ok_or_else(|| {
                Failure::new(
                    ErrorCode::StaleRevision,
                    "Document changed during save preparation",
                )
            })?;
        let path = self.tabs[index].session.path().to_path_buf();
        self.check_plugin_save_path(&path)?;
        let tab = &mut self.tabs[index];
        let disk =
            std::fs::read(&path).map_err(|e| Failure::new(ErrorCode::Conflict, e.to_string()))?;
        if tab.disk_state != DiskState::Synced
            || Sha256::digest(&disk).as_slice() != tab.disk_digest
        {
            return Err(Failure::new(
                ErrorCode::Conflict,
                "File changed during save preparation",
            ));
        }
        if !request.enter_side_effect() {
            return Err(Failure::new(ErrorCode::Cancelled, "Save did not execute"));
        }
        tab.session
            .save(&prepared, prepared.contents())
            .map_err(|e| Failure::new(ErrorCode::OperationFailed, e.to_string()))?;
        tab.disk_digest = Sha256::digest(prepared.contents().as_bytes()).into();
        tab.disk_state = DiskState::Synced;
        tab.last_saved_at = Instant::now();
        tab.overwrite_confirmed = false;
        self.notify_language_document_saved(&path, prepared.contents().to_owned(), cx);
        Ok(Value::Saved {
            document: document.clone(),
        })
    }

    /// A stored document identity survives disk deletion, but writes require a live, unredirected workspace path.
    fn check_plugin_save_path(&self, path: &Path) -> Result<(), Failure> {
        let resolved = path
            .canonicalize()
            .map_err(|e| Failure::new(ErrorCode::Conflict, e.to_string()))?;
        if resolved != path || !resolved.starts_with(self.workspace.root()) {
            return Err(Failure::new(
                ErrorCode::PermissionDenied,
                "Document path was redirected",
            ));
        }
        Ok(())
    }
}
