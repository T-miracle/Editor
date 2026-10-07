//! One background transfer policy shared by clipboard, tree drag and native file drops.

use super::fingerprint::Tree;
use super::publication::{Entry, Plan, Protection, ResultWithPlan};
use super::snapshot::{self, Snapshot, Stamp};
mod copy;
use crate::editor::file_watch::{Reconciliation, WatchedFile, read_documents};
use editor_core::Workspace;
use futures::{
    FutureExt as _,
    channel::{mpsc, oneshot},
    future::BoxFuture,
};
use rust_i18n::t;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Copy,
    Move,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    Skip,
    KeepBoth,
    Replace,
    Cancel,
    Force,
}

/// A batch decision affects only subsequent conflicts; protected targets still require a safe choice.
pub(super) struct Decision {
    pub choice: Choice,
    pub subsequent: bool,
    pub discard: bool,
    pub approvals: Vec<Approval>,
}

/// Consent names a particular live session and revision; reopening or new input revokes it.
#[derive(Clone)]
pub(super) struct Approval {
    pub path: PathBuf,
    pub file_id: u64,
    pub revision: u64,
    pub capability_revision: u64,
}

/// A completed atomic disk change, with its preimage and expected postimage for later undo.
#[derive(Clone, Debug)]
pub(super) struct Change {
    pub path: PathBuf,
    pub before: Snapshot,
    pub after: Stamp,
}

/// Records retain private backups until their session/history entry is discarded.
pub(super) struct Receipt {
    pub backup: Arc<tempfile::TempDir>,
    pub changes: Vec<Change>,
    pub moves: Vec<(PathBuf, PathBuf)>,
    pub label: String,
}

pub(super) struct Outcome {
    pub reconciliation: Reconciliation,
    pub receipt: Receipt,
    pub completed: usize,
    pub skipped: Vec<PathBuf>,
    pub error: Option<String>,
    pub remaining: usize,
    pub cancelled: bool,
    pub pending: Option<Receipt>,
}

/// Responses cross executors as values; GPUI entities and document text remain on the UI thread.
pub(super) enum Event {
    Documents {
        reply: oneshot::Sender<Vec<WatchedFile>>,
    },
    ReviewRecovery {
        paths: Vec<PathBuf>,
        preserve: Vec<PathBuf>,
        changed: bool,
        reply: oneshot::Sender<Decision>,
    },
    Discard {
        update: Reconciliation,
        approvals: Vec<Approval>,
        reply: oneshot::Sender<()>,
    },
    Conflict {
        source: PathBuf,
        target: PathBuf,
        merge: bool,
        recovery: bool,
        reply: oneshot::Sender<Decision>,
    },
    Authorize {
        target: PathBuf,
        reply: oneshot::Sender<bool>,
    },
    Publish {
        plan: Plan,
        reply: oneshot::Sender<ResultWithPlan>,
    },
    Progress {
        path: PathBuf,
        completed: usize,
        bytes: u64,
    },
}

pub(super) struct Worker {
    pub root: PathBuf,
    pub kind: Kind,
    pub events: mpsc::UnboundedSender<Event>,
    pub cancelled: Arc<AtomicBool>,
    pub receipt: Receipt,
    pub completed: usize,
    pub skipped: Vec<PathBuf>,
    policy: Option<Choice>,
    bytes: u64,
    pub(super) workspace: Workspace,
    pub(super) watched: Vec<WatchedFile>,
    pub(super) cleanup_error: Option<String>,
    expected: Tree,
    _lifetime: super::resources::Lifetime,
}

impl Worker {
    /// Start a session-owned batch; no backup is placed under the user's workspace.
    pub fn new(
        workspace: Workspace,
        watched: Vec<WatchedFile>,
        kind: Kind,
        events: mpsc::UnboundedSender<Event>,
        cancelled: Arc<AtomicBool>,
        label: String,
        resources: Arc<super::resources::Resources>,
    ) -> Result<Self, String> {
        let backup = Arc::new(
            tempfile::Builder::new()
                .prefix("me-editor-files-")
                .tempdir()
                .map_err(|error| error.to_string())?,
        );
        let lifetime = resources.register(backup.path().to_path_buf(), &cancelled);
        Ok(Self {
            root: workspace.root().to_path_buf(),
            workspace,
            watched,
            kind,
            events,
            cancelled,
            receipt: Receipt {
                backup,
                changes: Vec::new(),
                moves: Vec::new(),
                label,
            },
            completed: 0,
            skipped: Vec::new(),
            policy: None,
            bytes: 0,
            cleanup_error: None,
            expected: Tree::default(),
            _lifetime: lifetime,
        })
    }

    /// Stop after the first failure; completed leaf changes survive and remain undoable.
    pub async fn run(mut self, sources: Vec<PathBuf>, target: PathBuf) -> Outcome {
        let mut error = None;
        let mut remaining = 0;
        for (index, source) in sources.iter().enumerate() {
            let result = async {
                snapshot::check_native_path(source)?;
                snapshot::check_target(&self.root, &target)?;
                if !target.is_dir() {
                    return Err(format!(
                        "{}: {}",
                        target.display(),
                        t!("transfer.invalid_directory")
                    ));
                }
                let meta = fs::symlink_metadata(source)
                    .map_err(|error| format!("{}: {error}", source.display()))?;
                if snapshot::is_link(&meta) {
                    self.skipped.push(source.clone());
                    return Ok(());
                }
                let source = source.canonicalize().map_err(|error| error.to_string())?;
                let name = source
                    .file_name()
                    .ok_or_else(|| t!("explorer.invalid_source").to_string())?;
                let destination = target.join(name);
                if destination == source || destination.starts_with(&source) {
                    return Err(format!(
                        "{}: {}",
                        source.display(),
                        t!("explorer.self_paste")
                    ));
                }
                self.transfer(source, destination).await.map(|_| ())
            }
            .await;
            if let Err(failure) = result {
                error = Some(failure);
                remaining = sources.len() - index - 1;
                break;
            }
            if let Some(failure) = self.cleanup_error.take() {
                error = Some(failure);
                remaining = sources.len() - index - 1;
                break;
            }
        }
        // Directory stamps are finalized after child operations, so new descendants are detectable during undo.
        for change in &mut self.receipt.changes {
            if let Ok(stamp) = self.expected.stamp(&change.path) {
                change.after = stamp;
            }
        }
        let cancelled = self.cancelled.load(Ordering::Relaxed);
        let reconciliation = self.reconcile_current_documents().await;
        Outcome {
            reconciliation,
            receipt: self.receipt,
            completed: self.completed,
            skipped: self.skipped,
            error,
            remaining,
            cancelled,
            pending: None,
        }
    }

    /// A tab opened during a long operation belongs in the final reload too; only path metadata crosses threads.
    pub(super) async fn reconcile_current_documents(&mut self) -> Reconciliation {
        let (reply, current) = oneshot::channel();
        let _ = self.events.unbounded_send(Event::Documents { reply });
        if let Ok(watched) = current.await {
            self.watched = watched;
        }
        Reconciliation {
            snapshot: Some(self.workspace.snapshot()),
            documents: read_documents(&self.watched),
            renames: Vec::new(),
            native: true,
        }
    }

    pub fn check_cancel(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::Relaxed) {
            Err(t!("transfer.cancelled").to_string())
        } else {
            Ok(())
        }
    }

    /// Wait without blocking the executor while the user reviews an actual conflicting path.
    pub async fn decision(
        &mut self,
        source: &Path,
        target: &Path,
        merge: bool,
        recovery: bool,
    ) -> Result<Choice, String> {
        self.check_cancel()?;
        if !recovery && let Some(choice) = self.policy {
            return Ok(choice);
        }
        let (reply, answer) = oneshot::channel();
        self.events
            .unbounded_send(Event::Conflict {
                source: source.to_path_buf(),
                target: target.to_path_buf(),
                merge,
                recovery,
                reply,
            })
            .map_err(|_| t!("transfer.window_closed").to_string())?;
        let decision = answer
            .await
            .map_err(|_| t!("transfer.cancelled").to_string())?;
        if decision.choice == Choice::Cancel {
            self.cancelled.store(true, Ordering::Relaxed);
            return self.check_cancel().map(|_| decision.choice);
        }
        if decision.subsequent && !recovery {
            self.policy = Some(decision.choice);
        }
        Ok(decision.choice)
    }

    pub async fn authorize(&self, target: &Path) -> Result<bool, String> {
        let (reply, answer) = oneshot::channel();
        self.events
            .unbounded_send(Event::Authorize {
                target: target.to_path_buf(),
                reply,
            })
            .map_err(|_| t!("transfer.window_closed").to_string())?;
        answer
            .await
            .map_err(|_| t!("transfer.window_closed").to_string())
    }

    /// The UI owns final session authorization and a short rename transaction; cleanup returns to this executor.
    pub(super) async fn publish(&mut self, plan: Plan) -> Result<Vec<Approval>, String> {
        let (reply, answer) = oneshot::channel();
        self.events
            .unbounded_send(Event::Publish { plan, reply })
            .map_err(|_| t!("transfer.window_closed").to_string())?;
        let result = answer
            .await
            .map_err(|_| t!("transfer.window_closed").to_string())?;
        if result.result.is_ok() {
            for entry in &result.plan.entries {
                self.expected.put(&entry.path, entry.after.clone())?;
            }
        }
        if result.result.is_err() {
            // A rollback failure is still an actual disk change and must remain recoverable in session history.
            for entry in result.plan.entries.iter().filter(|entry| entry.changed()) {
                self.receipt.changes.push(Change {
                    path: entry.path.clone(),
                    before: entry.before.clone(),
                    after: snapshot::stamp(&entry.path).unwrap_or(Stamp::Missing),
                });
            }
        }
        if let Err(error) = result.plan.cleanup(result.result.is_ok()) {
            self.cleanup_error = Some(error);
        }
        result.result.map(|_| result.approvals)
    }

    /// Recursive directories contribute leaf records; skipped links keep their source directory alive.
    fn transfer(
        &mut self,
        source: PathBuf,
        mut target: PathBuf,
    ) -> BoxFuture<'_, Result<bool, String>> {
        async move {
            self.check_cancel()?;
            let meta = fs::symlink_metadata(&source)
                .map_err(|error| format!("{}: {error}", source.display()))?;
            if snapshot::is_link(&meta) {
                self.skipped.push(source);
                return Ok(false);
            }
            snapshot::check_target(&self.root, &target)?;
            let mut original = snapshot::stamp(&target)?;
            if original != Stamp::Missing {
                let merge = meta.is_dir() && target.is_dir();
                let mut choice = self.decision(&source, &target, merge, false).await?;
                if choice == Choice::Replace && !merge && !self.authorize(&target).await? {
                    // A batch replacement policy cannot override a dirty buffer opened during the batch.
                    self.policy = None;
                    choice = self.decision(&source, &target, merge, false).await?;
                }
                match choice {
                    Choice::Skip => {
                        self.skipped.push(source);
                        return Ok(false);
                    }
                    Choice::KeepBoth => {
                        target = free_name(&target)?;
                        original = Stamp::Missing;
                    }
                    Choice::Replace => {}
                    _ => return Err(t!("transfer.cancelled").to_string()),
                }
            }
            if meta.is_dir() {
                return self.directory(&source, &target, &original).await;
            }
            if !meta.is_file() {
                return Err(format!(
                    "{}: {}",
                    source.display(),
                    t!("transfer.unsupported")
                ));
            }
            self.file(&source, &target, &original).await?;
            Ok(true)
        }
        .boxed()
    }

    async fn directory(
        &mut self,
        source: &Path,
        target: &Path,
        original: &Stamp,
    ) -> Result<bool, String> {
        let first_change = self.receipt.changes.len();
        if !target.is_dir() {
            let before = snapshot::capture(target, self.receipt.backup.path())?;
            let entry =
                Entry::restore(target, &Snapshot::Directory { asset: None }, before.clone())?;
            if snapshot::stamp(target)? != *original {
                return Err(t!("transfer.changed").to_string());
            }
            self.publish(Plan {
                entries: vec![entry],
                root: Some(self.root.clone()),
                movement: None,
                protection: Protection::Ordinary {
                    targets: vec![target.to_path_buf()],
                },
            })
            .await?;
            self.receipt.changes.push(Change {
                path: target.to_path_buf(),
                before,
                after: self.expected.stamp(target)?,
            });
        }
        let mut entries = fs::read_dir(source)
            .map_err(|error| format!("{}: {error}", source.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        entries.sort_by_key(|entry| entry.file_name());
        let empty = entries.is_empty();
        let mut all_moved = true;
        for entry in entries {
            if let Some(error) = self.cleanup_error.take() {
                return Err(error);
            }
            match self
                .transfer(entry.path(), target.join(entry.file_name()))
                .await
            {
                Ok(done) => all_moved &= done,
                Err(error) => {
                    // A newly created empty directory is the current unfinished result, not a completed import.
                    if self.receipt.changes.len() == first_change + 1
                        && fs::read_dir(target)
                            .ok()
                            .is_some_and(|mut entries| entries.next().is_none())
                    {
                        let change = self.receipt.changes.pop().unwrap();
                        snapshot::restore(&change.before, target)?;
                    }
                    return Err(error);
                }
            }
        }
        if self.kind == Kind::Move && all_moved {
            // Only empty source directories are removed; merge-skipped children remain at their original path.
            fs::remove_dir(source).map_err(|error| format!("{}: {error}", source.display()))?;
            self.receipt.changes.push(Change {
                path: source.to_path_buf(),
                before: Snapshot::Directory { asset: None },
                after: Stamp::Missing,
            });
        }
        if empty {
            self.completed += 1;
        }
        Ok(all_moved)
    }
}

/// Preserve extensions and choose an unused native filename; no existing entry is overwritten.
fn free_name(path: &Path) -> Result<PathBuf, String> {
    let stem = if path.is_dir() {
        path.file_name()
    } else {
        path.file_stem()
    }
    .unwrap_or_default()
    .to_string_lossy();
    let extension = if path.is_dir() {
        None
    } else {
        path.extension()
    };
    for index in 2.. {
        let mut name = format!("{stem} ({index})");
        if let Some(extension) = extension {
            name.push('.');
            name.push_str(&extension.to_string_lossy());
        }
        let candidate = path.with_file_name(name);
        match fs::symlink_metadata(&candidate) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(candidate),
            Err(error) => return Err(format!("{}: {error}", candidate.display())),
        }
    }
    unreachable!()
}
