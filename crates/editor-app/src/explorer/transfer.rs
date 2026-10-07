//! Application-facing transfer state; one worker serves paste, native drop and tree drag.

pub(crate) mod clipboard;
mod operation;
pub(crate) mod shortcuts;
mod snapshot;

use crate::*;
use futures::channel::{mpsc, oneshot};
use gpui_kit::AnyElement;
pub(crate) use operation::Kind;
use operation::{Choice, Decision, Event, Outcome, Receipt, Worker};
use std::sync::atomic::{AtomicBool, Ordering};

/// Session disk history is separate from each document's text undo stack.
#[derive(Default)]
pub(crate) struct TransferState {
    active: Option<Active>,
    prompt: Option<Prompt>,
    undo: Vec<Receipt>,
    redo: Vec<Receipt>,
    deferred_renames: Vec<(PathBuf, PathBuf)>,
}

struct Active {
    cancel: Arc<AtomicBool>,
    visible: bool,
    completed: usize,
    bytes: u64,
    path: PathBuf,
    cut_offer: Option<(gpui_kit::ClipboardItem, Option<u32>)>,
}

struct Prompt {
    source: PathBuf,
    target: PathBuf,
    merge: bool,
    recovery: bool,
    discard: bool,
    subsequent: bool,
    reply: oneshot::Sender<Decision>,
    focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
}

impl Drop for TransferState {
    fn drop(&mut self) {
        // A closing window cancels its worker; dropped prompt senders also release awaited decisions.
        if let Some(active) = &self.active {
            active.cancel.store(true, Ordering::Relaxed);
        }
    }
}

impl TransferState {
    pub(crate) fn is_running(&self) -> bool {
        self.active.is_some()
    }
    pub(crate) fn defer_reconciliation(&mut self, update: &Reconciliation) -> bool {
        if self.is_running() {
            self.deferred_renames.extend(update.renames.clone());
            true
        } else {
            false
        }
    }
}

impl EditorApp {
    /// Clipboard paste enters the shared policy and retains the exact cut offer for later cleanup.
    pub(crate) fn paste_file_offer(
        &mut self,
        row: Option<&Path>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.file_transfers.is_running() {
            self.file_transfer_error(t!("transfer.busy").to_string(), cx);
            return;
        }
        let item = cx.read_from_clipboard();
        let (sources, kind) = match clipboard::read(item.as_ref()) {
            Ok(value) => value,
            Err(error) => {
                self.file_transfer_error(error, cx);
                return;
            }
        };
        let sequence = clipboard::sequence();
        self.start_file_transfer(sources, self.explorer_destination(row), kind, window, cx);
        if kind == Kind::Move
            && let Some(active) = &mut self.file_transfers.active
        {
            active.cut_offer = item.map(|item| (item, sequence));
        }
    }

    /// Folders target themselves, files their parent, and unselected commands target the workspace root.
    pub(crate) fn explorer_destination(&self, row: Option<&Path>) -> PathBuf {
        match row {
            Some(path) if path.is_dir() => path.to_path_buf(),
            Some(path) => path.parent().unwrap_or(self.workspace.root()).to_path_buf(),
            None => self.workspace.root().to_path_buf(),
        }
    }

    /// Ordinary writes must never replace a target represented by a dirty document.
    fn dirty_transfer_target(&self, path: &Path) -> bool {
        self.tabs.iter().any(|tab| {
            tab.path().starts_with(path)
                && tab
                    .text
                    .as_ref()
                    .is_some_and(|text| text.session.is_dirty())
        })
    }

    /// Begin a serial batch on GPUI's background executor, with UI decisions sent over value channels.
    pub(crate) fn start_file_transfer(
        &mut self,
        sources: Vec<PathBuf>,
        destination: PathBuf,
        kind: Kind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.file_transfers.is_running() {
            self.file_transfer_error(t!("transfer.busy").to_string(), cx);
            return;
        }
        if sources.is_empty() {
            self.file_transfer_error(t!("explorer.no_clipboard_files").to_string(), cx);
            return;
        }
        let label = if kind == Kind::Move {
            t!("transfer.move")
        } else {
            t!("transfer.copy")
        }
        .to_string();
        let (events, mut updates) = mpsc::unbounded();
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
            kind,
            events,
            cancel.clone(),
            label,
        ) {
            Ok(worker) => worker,
            Err(error) => {
                self.file_transfer_error(error, cx);
                return;
            }
        };
        let timer_cancel = cancel.clone();
        self.file_transfers.active = Some(Active {
            cancel,
            visible: false,
            completed: 0,
            bytes: 0,
            path: destination.clone(),
            cut_offer: None,
        });
        // A lightweight timer reveals progress only when the batch outlasts the normal quick interaction.
        cx.spawn_in(window, async move |app, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(350))
                .await;
            let _ = app.update_in(cx, |app, _, cx| {
                if let Some(active) = &mut app.file_transfers.active
                    && Arc::ptr_eq(&active.cancel, &timer_cancel)
                {
                    active.visible = true;
                    cx.notify();
                }
            });
        })
        .detach();
        let task = cx
            .background_executor()
            .spawn(worker.run(sources, destination));
        cx.spawn_in(window, async move |app, cx| {
            while let Some(event) = updates.next().await {
                if app
                    .update_in(cx, |app, window, cx| {
                        app.file_transfer_event(event, window, cx)
                    })
                    .is_err()
                {
                    return;
                }
            }
            let outcome = task.await;
            let _ = app.update_in(cx, |app, window, cx| {
                app.finish_file_transfer(outcome, window, cx)
            });
        })
        .detach();
        cx.notify();
    }

    fn file_transfer_event(&mut self, event: Event, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            Event::Authorize { target, reply } => {
                let _ = reply.send(!self.dirty_transfer_target(&target));
            }
            Event::Conflict {
                source,
                target,
                merge,
                recovery,
                reply,
            } => {
                let previous_focus = window.focused(cx);
                let focus = cx.focus_handle();
                focus.focus(window, cx);
                self.file_transfers.prompt = Some(Prompt {
                    source,
                    target,
                    merge,
                    recovery,
                    discard: false,
                    subsequent: false,
                    reply,
                    focus,
                    previous_focus,
                });
            }
            Event::Progress {
                path,
                completed,
                bytes,
            } => {
                if let Some(active) = &mut self.file_transfers.active {
                    active.path = path;
                    active.completed = completed;
                    active.bytes = bytes;
                }
            }
            Event::Moved { old, new, reply } => {
                self.retarget_transferred_documents(&old, &new, window, cx);
                let _ = reply.send(());
            }
        }
        cx.notify();
    }

    /// Rename the existing document session and editor entity; no text/cursor/undo state is recreated.
    fn retarget_transferred_documents(
        &mut self,
        old: &Path,
        new: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tabs.iter().any(|tab| tab.path() == old)
            && self.tabs.iter().any(|tab| {
                tab.path() == new
                    && tab
                        .text
                        .as_ref()
                        .is_none_or(|text| !text.session.is_dirty())
            })
        {
            // The overwritten clean target no longer represents its old file; keep the source editor's identity.
            self.close_tab(new.to_path_buf(), window, cx);
        }
        self.apply_reconciliation(
            Reconciliation {
                snapshot: None,
                renames: vec![(old.to_path_buf(), new.to_path_buf())],
                documents: Vec::new(),
                native: true,
            },
            window,
            cx,
        );
    }

    fn finish_file_transfer(
        &mut self,
        outcome: Outcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let active = self.file_transfers.active.take();
        if outcome.error.is_none() && outcome.skipped.is_empty() && !outcome.cancelled {
            if let Some((offer, sequence)) = active.and_then(|active| active.cut_offer) {
                if cx.read_from_clipboard().as_ref() == Some(&offer)
                    && clipboard::sequence() == sequence
                {
                    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(String::new()));
                }
            }
        }
        self.file_transfers.prompt = None;
        if !outcome.receipt.changes.is_empty() {
            self.file_transfers.redo.clear();
            self.file_transfers.undo.push(outcome.receipt);
        }
        // Deferred external rename pairs still apply; stale disk reads are replaced with a fresh scan.
        let renames = std::mem::take(&mut self.file_transfers.deferred_renames);
        self.apply_reconciliation(
            Reconciliation {
                snapshot: None,
                renames,
                documents: Vec::new(),
                native: true,
            },
            window,
            cx,
        );
        self.capture_explorer_state(cx);
        self.apply_reconciliation(outcome.reconciliation, window, cx);
        self.file_watch.reconcile();
        self.status = t!(
            "transfer.result",
            completed = outcome.completed,
            failed = usize::from(outcome.error.is_some() && !outcome.cancelled),
            skipped = outcome.skipped.len(),
            remaining = outcome.remaining
        )
        .to_string();
        if outcome.error.is_some() || !outcome.skipped.is_empty() || outcome.cancelled {
            let mut message = self.status.clone();
            if let Some(error) = outcome.error {
                message.push_str(&format!("\n{error}"));
            }
            if !outcome.skipped.is_empty() {
                message.push_str(&format!(
                    "\n{}",
                    outcome
                        .skipped
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            }
            self.file_transfer_error(message, cx);
        }
        cx.notify();
    }

    pub(crate) fn file_transfer_error(&mut self, message: String, cx: &mut Context<Self>) {
        self.status = message.clone();
        self.notification = Some(cx.new(|_| {
            ui::controls::Notification::persistent(t!("transfer.title").to_string(), message)
        }));
        cx.notify();
    }

    fn choose_transfer_conflict(
        &mut self,
        choice: Choice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(prompt) = self.file_transfers.prompt.take() else {
            return;
        };
        if choice == Choice::Replace && !prompt.merge && self.dirty_transfer_target(&prompt.target)
        {
            self.file_transfers.prompt = Some(prompt);
            cx.notify();
            return;
        }
        let _ = prompt.reply.send(Decision {
            choice,
            subsequent: prompt.subsequent,
        });
        if let Some(previous) = prompt.previous_focus {
            previous.focus(window, cx);
        }
        cx.notify();
    }

    /// UI painting stays in local controls; the explorer owns only choices and operation state.
    pub(crate) fn render_file_transfer(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut surface = div();
        if let Some(active) = &self.file_transfers.active
            && active.visible
        {
            let cancel = active.cancel.clone();
            surface = surface.child(ui::controls::file_operation::progress(
                t!(
                    "transfer.progress",
                    completed = active.completed,
                    bytes = active.bytes,
                    path = active.path.display()
                )
                .to_string(),
                move |_, _| {
                    cancel.store(true, Ordering::Relaxed);
                },
                cx,
            ));
        }
        if let Some(prompt) = &self.file_transfers.prompt {
            // Merge preserves unique files; dirty protection applies to colliding leaf files.
            let protected = !prompt.merge && self.dirty_transfer_target(&prompt.target);
            let owner = cx.entity().downgrade();
            let choices = [
                (
                    "transfer-skip",
                    t!("transfer.skip").to_string(),
                    Choice::Skip,
                    true,
                ),
                (
                    "transfer-keep-both",
                    t!("transfer.keep_both").to_string(),
                    Choice::KeepBoth,
                    true,
                ),
                (
                    "transfer-replace",
                    if prompt.merge {
                        t!("transfer.merge")
                    } else {
                        t!("transfer.replace")
                    }
                    .to_string(),
                    Choice::Replace,
                    !protected,
                ),
                (
                    "transfer-conflict-cancel",
                    t!("common.cancel").to_string(),
                    Choice::Cancel,
                    true,
                ),
            ]
            .into_iter()
            .map(|(id, label, choice, enabled)| {
                let owner = owner.clone();
                ui::controls::file_operation::FileChoice {
                    id,
                    label: label.into(),
                    enabled,
                    activate: Box::new(move |window, cx| {
                        let _ = owner.update(cx, |app, cx| {
                            app.choose_transfer_conflict(choice, window, cx)
                        });
                    }),
                }
            })
            .collect();
            let subsequent_owner = owner.clone();
            let mut details = vec![
                prompt.source.display().to_string(),
                prompt.target.display().to_string(),
            ];
            if protected {
                details.push(t!("transfer.dirty_target").to_string());
            }
            surface = surface.child(ui::controls::file_operation::conflict_prompt(
                prompt.focus.clone(),
                t!("transfer.conflict").to_string(),
                details,
                choices,
                Some((
                    prompt.subsequent,
                    Box::new(move |checked, cx| {
                        let _ = subsequent_owner.update(cx, |app, cx| {
                            if let Some(prompt) = &mut app.file_transfers.prompt {
                                prompt.subsequent = checked;
                            }
                            cx.notify();
                        });
                    }),
                )),
                move |window, cx| {
                    let _ = owner.update(cx, |app, cx| {
                        app.choose_transfer_conflict(Choice::Cancel, window, cx)
                    });
                },
                cx,
            ));
        }
        surface.into_any_element()
    }
}
