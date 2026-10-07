//! Session history routes disk recovery through the same background ownership as file transfers.
use super::*;
use crate::editor::file_watch::DiskContent;

#[derive(Clone, Copy)]
pub(super) enum Direction {
    Undo,
    Redo,
}

impl EditorApp {
    pub(crate) fn undo_file_transfer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.start_file_recovery(Direction::Undo, window, cx);
    }
    pub(crate) fn redo_file_transfer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.start_file_recovery(Direction::Redo, window, cx);
    }

    /// Menu text names the actual recorded operation instead of the currently selected tree row.
    pub(crate) fn file_history_label(&self, redo: bool) -> String {
        let receipt = if redo {
            self.file_transfers.redo.last()
        } else {
            self.file_transfers.undo.last()
        };
        let operation = receipt.map_or_else(
            || t!("transfer.no_history").to_string(),
            |receipt| receipt.label.clone(),
        );
        if redo {
            t!("transfer.redo", operation = operation)
        } else {
            t!("transfer.undo", operation = operation)
        }
        .to_string()
    }
    pub(crate) fn file_history_available(&self, redo: bool) -> bool {
        !self.file_transfers.is_running()
            && if redo {
                !self.file_transfers.redo.is_empty()
            } else {
                !self.file_transfers.undo.is_empty()
            }
    }

    fn start_file_recovery(
        &mut self,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.file_transfers.is_running() || self.file_transfers.closing {
            self.file_transfer_error(t!("transfer.busy").to_string(), cx);
            return;
        }
        let receipt = match direction {
            Direction::Undo => self.file_transfers.undo.pop(),
            Direction::Redo => self.file_transfers.redo.pop(),
        };
        let Some(receipt) = receipt else {
            return;
        };
        let (events, updates) = mpsc::unbounded();
        let cancel = Arc::new(AtomicBool::new(false));
        let watched = self
            .tabs
            .iter()
            .map(|tab| crate::editor::file_watch::WatchedFile {
                path: tab.path().to_path_buf(),
                text: tab.text.is_some(),
            })
            .collect();
        let worker = match Worker::new(
            self.workspace.clone(),
            watched,
            Kind::Copy,
            events,
            cancel.clone(),
            receipt.label.clone(),
            self.file_transfers.resources.clone(),
        ) {
            Ok(worker) => worker,
            Err(error) => {
                self.record_file_outcome(
                    Some(direction),
                    Receipt {
                        backup: receipt.backup.clone(),
                        changes: Vec::new(),
                        moves: Vec::new(),
                        label: receipt.label.clone(),
                    },
                    Some(receipt),
                );
                self.file_transfer_error(error, cx);
                return;
            }
        };
        self.file_transfers.active = Some(Active {
            cancel,
            visible: false,
            completed: 0,
            bytes: 0,
            path: self.workspace.root().to_path_buf(),
            cut_offer: None,
            history: Some(direction),
        });
        self.show_file_progress_later(window, cx);
        let task = cx.background_executor().spawn(worker.recover(receipt));
        self.listen_file_worker(task, updates, window, cx);
        cx.notify();
    }

    /// Partial undo/redo keeps unapplied records on its originating stack and records only actual inverses.
    pub(super) fn record_file_outcome(
        &mut self,
        direction: Option<Direction>,
        receipt: Receipt,
        pending: Option<Receipt>,
    ) {
        match direction {
            None => {
                if !receipt.changes.is_empty() {
                    self.file_transfers.redo.clear();
                    self.file_transfers.undo.push(receipt);
                }
            }
            Some(Direction::Undo) => {
                if let Some(pending) = pending {
                    self.file_transfers.undo.push(pending);
                }
                if !receipt.changes.is_empty() {
                    self.file_transfers.redo.push(receipt);
                }
            }
            Some(Direction::Redo) => {
                if let Some(pending) = pending {
                    self.file_transfers.redo.push(pending);
                }
                if !receipt.changes.is_empty() {
                    self.file_transfers.undo.push(receipt);
                }
            }
        }
    }

    /// A clean move may preserve a dirty source buffer; force or destructive changes require review.
    pub(super) fn review_file_recovery(
        &mut self,
        paths: Vec<PathBuf>,
        preserve: Vec<PathBuf>,
        changed: bool,
        reply: oneshot::Sender<Decision>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dirty = paths
            .iter()
            .any(|path| !preserve.contains(path) && self.dirty_transfer_target(path));
        if !changed && !dirty {
            let _ = reply.send(Decision {
                choice: Choice::Force,
                subsequent: false,
                discard: false,
                approvals: self.file_recovery_approvals(&paths),
            });
            return;
        }
        let previous_focus = window.focused(cx);
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        self.file_transfers.prompt = Some(Prompt {
            source: paths.first().cloned().unwrap_or_default(),
            target: paths.last().cloned().unwrap_or_default(),
            merge: false,
            recovery: true,
            discard: false,
            subsequent: false,
            reply,
            focus,
            previous_focus,
            affected: paths,
        });
        cx.notify();
    }

    /// Only successful, explicitly confirmed recovery discards live edits; disk reading stays in the worker.
    pub(super) fn discard_recovered_documents(
        &mut self,
        update: Reconciliation,
        approvals: Vec<Approval>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut preserved = false;
        for (path, contents, read_at) in &update.documents {
            if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.path() == path) {
                let Some(text) = tab.text.as_mut() else {
                    continue;
                };
                if !approvals.iter().any(|approval| {
                    approval.path == *path
                        && approval.file_id == tab.file_id
                        && approval.revision == text.session.revision()
                        && approval.capability_revision == text.capability_revision
                }) || *read_at < tab.opened_at
                    || *read_at < text.last_saved_at
                {
                    // The disk operation may have completed, but later input never inherits older discard consent.
                    preserved = true;
                    continue;
                }
                let value = match contents {
                    Ok(DiskContent::Text(value)) => value.clone(),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
                    _ => continue,
                };
                text.suppress_change = true;
                text.editor
                    .update(cx, |editor, cx| editor.set_value(value, window, cx));
                text.suppress_change = false;
                text.session.accept_disk_reload();
                text.disk_digest = [0; 32];
                text.capability_revision = text.capability_revision.saturating_add(1);
            }
        }
        self.apply_reconciliation(update, window, cx);
        self.invalidate_editor_previews(cx);
        self.sync_editor_previews(cx);
        if preserved {
            self.file_transfer_error(t!("transfer.new_edits_preserved").to_string(), cx);
        }
    }

    /// Capture only identities and revisions; editable text and its undo stack stay on the UI thread.
    pub(super) fn file_recovery_approvals(&self, paths: &[PathBuf]) -> Vec<Approval> {
        self.tabs
            .iter()
            .filter_map(|tab| {
                if !paths.iter().any(|path| tab.path().starts_with(path)) {
                    return None;
                }
                let text = tab.text.as_ref()?;
                Some(Approval {
                    path: tab.path().to_path_buf(),
                    file_id: tab.file_id,
                    revision: text.session.revision(),
                    capability_revision: text.capability_revision,
                })
            })
            .collect()
    }
}
