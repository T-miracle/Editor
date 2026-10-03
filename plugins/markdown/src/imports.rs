//! Image-import intents own opaque host inputs and saved-file receipts, never clipboard bytes or document state.

use super::Source;
use notices::{History, Notice, Outcome, Reason};
use plugin_protocol::api;
use std::collections::{BTreeMap, VecDeque};

mod notices;
mod plans;

/// Retired intents retain request correlation until file-side-effect receipts have been observed.
#[derive(Default)]
pub(super) struct Imports {
    active: Option<Batch>,
    observing: Vec<Batch>,
    /// A closed document's outcome and retained-file facts remain available when its path is reopened.
    notices: BTreeMap<String, History>,
}

/// Only origin metadata, opaque handles and confirmed basenames survive an asynchronous boundary.
struct Batch {
    document: api::DocumentVersion,
    selection: api::TextRange,
    remaining: VecDeque<api::ImageInput>,
    saved: Vec<String>,
    /// One batch shares a numbering sequence across formats; a later independent batch starts at zero.
    cursor: usize,
    stopped: Option<Reason>,
    pending: Option<Pending>,
}

struct Pending {
    task: api::guest::EditorTask,
    phase: Phase,
}

enum Phase {
    Saving {
        input: api::ImageInput,
        name: String,
    },
    Inserting {
        selection: api::TextRange,
    },
}

impl Imports {
    /// Begin a user-initiated batch only against the exact readonly source captured by native input.
    /// No bytes or path reads cross the guest boundary; the host validates and creates each sibling file.
    pub(super) fn start(
        &mut self,
        document: api::DocumentVersion,
        selection: api::TextRange,
        images: Vec<api::ImageInput>,
        source: Option<&Source>,
        english: bool,
    ) {
        let valid_source = source.is_some_and(|source| {
            source.version == document && source.text.get(selection.start..selection.end).is_some()
        });
        let bytes = images.iter().try_fold(0u64, |sum, input| {
            (input.byte_len > 0 && input.byte_len <= 8 * 1024 * 1024)
                .then(|| sum.checked_add(input.byte_len))
                .flatten()
        });
        let valid_batch = !images.is_empty()
            && images.len() <= 8
            && bytes.is_some_and(|bytes| bytes <= 32 * 1024 * 1024)
            && images.iter().enumerate().all(|(index, input)| {
                images[..index]
                    .iter()
                    .all(|earlier| earlier.handle != input.handle)
            });
        // A delayed offer for another source is not a newer intent for the currently active import.
        if valid_source {
            self.stop(Reason::Superseded);
        }
        if !valid_source || !valid_batch {
            release(images);
            self.publish(
                document.path,
                Notice {
                    saved: Vec::new(),
                    outcome: Outcome::NotInserted(if valid_source {
                        Reason::InvalidInput
                    } else {
                        Reason::SourceChanged
                    }),
                },
            );
            return;
        }
        let mut batch = Batch {
            document,
            selection,
            remaining: images.into(),
            saved: Vec::new(),
            cursor: 0,
            stopped: None,
            pending: None,
        };
        if self.advance(&mut batch, source, english) {
            self.active = Some(batch);
        }
    }

    /// Source changes prevent every later edit while an accepted save is still observed for its receipt.
    pub(super) fn source_changed(&mut self) {
        self.stop(Reason::SourceChanged);
    }

    /// Formatting supersedes imports, but stopping an intent never discards a completed file's name.
    /// Returns whether stopping published a receipt that changes the native toolbar.
    pub(super) fn superseded(&mut self) -> bool {
        self.stop(Reason::Superseded)
    }

    /// Route by EditorTask ownership rather than current focus, panel or the currently visible document.
    /// Returns true after a correlated update so the source-bound native toolbar can show the outcome.
    pub(super) fn request(
        &mut self,
        notification: &api::Notification,
        source: Option<&Source>,
        english: bool,
    ) -> bool {
        if let Some(mut batch) = self.active.take() {
            if let Some(update) = batch
                .pending
                .as_mut()
                .and_then(|pending| pending.task.update(notification))
            {
                if self.update(&mut batch, update, source, english) {
                    self.active = Some(batch);
                }
                return true;
            }
            self.active = Some(batch);
        }
        for index in 0..self.observing.len() {
            let update = self.observing[index]
                .pending
                .as_mut()
                .and_then(|pending| pending.task.update(notification));
            if let Some(update) = update {
                let mut batch = self.observing.swap_remove(index);
                if self.update(&mut batch, update, source, english) {
                    self.observing.push(batch);
                }
                return true;
            }
        }
        false
    }

    /// Reopening a path exposes its receipt without granting an edit against the reopened identity.
    pub(super) fn message(&self, source: Option<&Source>, english: bool) -> Option<String> {
        self.notices.get(&source?.version.path)?.message(english)
    }

    /// Close unused inputs immediately. Accepted saves keep their completion gate: cancelling with
    /// WaitingStopped would make the request terminal and conceal an already-created external file.
    fn stop(&mut self, reason: Reason) -> bool {
        let Some(mut batch) = self.active.take() else {
            return false;
        };
        batch.stopped = Some(reason);
        release(batch.remaining.drain(..));
        let outcome = match batch.pending.as_ref() {
            Some(Pending {
                task,
                phase: Phase::Inserting { .. },
            }) => {
                let cancellation = task.cancel(api::CancelMode::TryTerminate);
                if matches!(cancellation, Ok(api::CancellationEffect::NotExecuted)) {
                    Outcome::NotInserted(reason)
                } else {
                    // A newer Preview may describe our own finished edit before its receipt arrives.
                    Outcome::UnconfirmedReferences(reason)
                }
            }
            _ => Outcome::NotInserted(reason),
        };
        let published = !batch.saved.is_empty();
        if published {
            self.publish(
                batch.document.path.clone(),
                Notice {
                    saved: batch.saved.clone(),
                    outcome,
                },
            );
        }
        if batch.pending.is_some() {
            self.observing.push(batch);
        }
        published
    }

    /// Sequential saves make mixed-format numbering deterministic. Only a full batch advances to one edit.
    fn advance(&mut self, batch: &mut Batch, source: Option<&Source>, english: bool) -> bool {
        if let Some(reason) = batch.stopped {
            return self.finish(batch, Outcome::NotInserted(reason));
        }
        let Some(source) = source.filter(|source| source.version == batch.document) else {
            return self.finish(batch, Outcome::NotInserted(Reason::SourceChanged));
        };
        let (operation, phase) = if let Some(input) = batch.remaining.pop_front() {
            let Some(name) = plans::candidate(input.format.extension(), batch.cursor) else {
                release([input]);
                return self.finish(batch, Outcome::NotInserted(Reason::NamesExhausted));
            };
            (
                api::EditorOperation::SaveImageInput {
                    input: input.handle.clone(),
                    name: name.clone(),
                },
                Phase::Saving { input, name },
            )
        } else {
            let Some(edit) = plans::insertion(
                &source.text,
                batch.selection.start..batch.selection.end,
                &batch.saved,
                english,
            ) else {
                return self.finish(batch, Outcome::NotInserted(Reason::InvalidSelection));
            };
            let selection = api::TextRange {
                start: edit.selection.start,
                end: edit.selection.end,
            };
            (
                api::EditorOperation::ReplaceDocumentRange {
                    document: batch.document.clone(),
                    range: batch.selection,
                    text: edit.text,
                    selection,
                    expected_selection: Some(batch.selection),
                },
                Phase::Inserting { selection },
            )
        };
        match api::guest::EditorTask::start(operation, 30_000) {
            Ok(task) => {
                batch.pending = Some(Pending { task, phase });
                true
            }
            Err(error) => {
                if let Phase::Saving { input, .. } = phase {
                    release([input]);
                }
                self.finish(batch, Outcome::NotInserted(Reason::Host(error.code)))
            }
        }
    }

    /// A terminal save owns its file receipt even if another document is now visible.
    /// Conflict is the sole retryable save error; successful files are never deleted or silently reinserted.
    fn update(
        &mut self,
        batch: &mut Batch,
        update: api::RequestUpdate,
        source: Option<&Source>,
        english: bool,
    ) -> bool {
        let pending = batch
            .pending
            .take()
            .expect("observed batches own one request");
        match update {
            api::RequestUpdate::Accepted | api::RequestUpdate::Progress { .. } => {
                batch.pending = Some(pending);
                true
            }
            api::RequestUpdate::Completed { result } => match (pending.phase, result) {
                (
                    Phase::Saving { input, name },
                    Ok(api::EditorValue::ImageSaved {
                        input: saved_input,
                        document,
                        name: saved_name,
                    }),
                ) => {
                    if input.handle != saved_input
                        || document != batch.document
                        || name != saved_name
                    {
                        release([input]);
                        return self.finish(batch, Outcome::NotInserted(Reason::InvalidInput));
                    }
                    batch.saved.push(saved_name);
                    batch.cursor += 1;
                    self.advance(batch, source, english)
                }
                (Phase::Saving { input, .. }, Err(error))
                    if error.code == api::ErrorCode::Conflict && batch.stopped.is_none() =>
                {
                    // The token remains owned on a name collision; retry atomically at the next shared index.
                    batch.remaining.push_front(input);
                    batch.cursor += 1;
                    self.advance(batch, source, english)
                }
                (Phase::Saving { input, .. }, result) => {
                    release([input]);
                    let reason = batch.stopped.unwrap_or_else(|| {
                        result
                            .err()
                            .map_or(Reason::InvalidInput, |error| Reason::Host(error.code))
                    });
                    self.finish(batch, Outcome::NotInserted(reason))
                }
                (
                    Phase::Inserting {
                        selection: expected,
                    },
                    Ok(api::EditorValue::Edited {
                        document,
                        selection,
                    }),
                ) => {
                    // Preview alone owns source bytes; an edit receipt confirms behavior without replacing our snapshot.
                    if document.id == batch.document.id
                        && document.path == batch.document.path
                        && document.revision > batch.document.revision
                        && selection == expected
                    {
                        self.finish(batch, Outcome::Inserted)
                    } else {
                        self.finish(batch, Outcome::UnconfirmedReferences(Reason::InvalidInput))
                    }
                }
                (Phase::Inserting { .. }, result) => {
                    let reason = batch.stopped.unwrap_or_else(|| {
                        result
                            .err()
                            .map_or(Reason::InvalidInput, |error| Reason::Host(error.code))
                    });
                    self.finish(batch, Outcome::NotInserted(reason))
                }
            },
            api::RequestUpdate::Cancelled { reason, effect } => {
                let reason = batch.stopped.unwrap_or(Reason::Host(reason));
                let outcome = match pending.phase {
                    Phase::Saving { input, name } => {
                        release([input]);
                        if matches!(effect, api::CancellationEffect::WaitingStopped) {
                            Outcome::UnconfirmedFile { name, reason }
                        } else {
                            Outcome::NotInserted(reason)
                        }
                    }
                    Phase::Inserting { .. } => {
                        if matches!(effect, api::CancellationEffect::WaitingStopped) {
                            Outcome::UnconfirmedReferences(reason)
                        } else {
                            Outcome::NotInserted(reason)
                        }
                    }
                };
                self.finish(batch, outcome)
            }
        }
    }

    /// Clear remaining inputs, keep all complete files, and retain a path-bound localized outcome.
    fn finish(&mut self, batch: &mut Batch, outcome: Outcome) -> bool {
        release(batch.remaining.drain(..));
        batch.pending = None;
        self.publish(
            batch.document.path.clone(),
            Notice {
                saved: std::mem::take(&mut batch.saved),
                outcome,
            },
        );
        false
    }

    /// Keep external-file facts when a later independent edit changes the most recent explanation.
    fn publish(&mut self, path: String, notice: Notice) {
        self.notices.entry(path).or_default().record(notice);
    }
}

/// Release metadata tokens rather than reading or deleting external files; retirement and TTL are host fallbacks.
fn release(inputs: impl IntoIterator<Item = api::ImageInput>) {
    for input in inputs {
        let _ = api::guest::close_resource(input.handle);
    }
}
