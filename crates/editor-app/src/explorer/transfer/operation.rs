//! One background transfer policy shared by clipboard, tree drag and native file drops.

use super::snapshot::{self, Snapshot, Stamp};
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
    io::{Read, Write},
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
}

/// Responses cross executors as values; GPUI entities and document text remain on the UI thread.
pub(super) enum Event {
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
    Progress {
        path: PathBuf,
        completed: usize,
        bytes: u64,
    },
    Moved {
        old: PathBuf,
        new: PathBuf,
        reply: oneshot::Sender<()>,
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
    workspace: Workspace,
    watched: Vec<WatchedFile>,
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
    ) -> Result<Self, String> {
        let backup = Arc::new(
            tempfile::Builder::new()
                .prefix("me-editor-files-")
                .tempdir()
                .map_err(|error| error.to_string())?,
        );
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
        })
    }

    /// Stop after the first failure; completed leaf changes survive and remain undoable.
    pub async fn run(mut self, sources: Vec<PathBuf>, target: PathBuf) -> Outcome {
        let mut error = None;
        let mut remaining = 0;
        for (index, source) in sources.iter().enumerate() {
            let result = async {
                snapshot::check_target(&self.root, &target)?;
                if !target.is_dir() {
                    return Err(format!("{}: target is not a directory", target.display()));
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
        }
        // Directory stamps are finalized after child operations, so new descendants are detectable during undo.
        for change in &mut self.receipt.changes {
            if let Ok(stamp) = snapshot::stamp(&change.path) {
                change.after = stamp;
            }
        }
        let cancelled = self.cancelled.load(Ordering::Relaxed);
        let reconciliation = Reconciliation {
            snapshot: Some(self.workspace.snapshot()),
            documents: read_documents(&self.watched),
            renames: Vec::new(),
            native: true,
        };
        Outcome {
            reconciliation,
            receipt: self.receipt,
            completed: self.completed,
            skipped: self.skipped,
            error,
            remaining,
            cancelled,
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
            if original != &Stamp::Missing {
                fs::remove_file(target).map_err(|error| error.to_string())?;
            }
            if let Err(error) = fs::create_dir(target) {
                snapshot::restore(&before, target)?;
                return Err(error.to_string());
            }
            self.receipt.changes.push(Change {
                path: target.to_path_buf(),
                before,
                after: snapshot::stamp(target)?,
            });
        }
        let mut entries = fs::read_dir(source)
            .map_err(|error| format!("{}: {error}", source.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        entries.sort_by_key(|entry| entry.file_name());
        let mut all_moved = true;
        for entry in entries {
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
        Ok(all_moved)
    }

    /// Write into a temporary sibling and publish only after a complete, verified copy.
    async fn file(&mut self, source: &Path, target: &Path, original: &Stamp) -> Result<(), String> {
        self.check_cancel()?;
        let source_stamp = snapshot::stamp(source)?;
        let before = snapshot::capture(target, self.receipt.backup.path())?;
        let source_before = if self.kind == Kind::Move {
            Some(snapshot::capture(source, self.receipt.backup.path())?)
        } else {
            None
        };
        let mut input =
            fs::File::open(source).map_err(|error| format!("{}: {error}", source.display()))?;
        let mut temporary = tempfile::Builder::new()
            .prefix(".me-transfer-")
            .tempfile_in(target.parent().unwrap())
            .map_err(|error| error.to_string())?;
        let mut buffer = vec![0; 256 * 1024];
        loop {
            self.check_cancel()?;
            let count = input
                .read(&mut buffer)
                .map_err(|error| format!("{}: {error}", source.display()))?;
            if count == 0 {
                break;
            }
            temporary
                .write_all(&buffer[..count])
                .map_err(|error| format!("{}: {error}", target.display()))?;
            self.bytes += count as u64;
            if self.bytes % (4 * 1024 * 1024) < count as u64 {
                let _ = self.events.unbounded_send(Event::Progress {
                    path: source.to_path_buf(),
                    completed: self.completed,
                    bytes: self.bytes,
                });
            }
        }
        temporary.flush().map_err(|error| error.to_string())?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| error.to_string())?;
        temporary
            .as_file()
            .set_permissions(
                fs::metadata(source)
                    .map_err(|error| error.to_string())?
                    .permissions(),
            )
            .map_err(|error| error.to_string())?;
        self.check_cancel()?;
        snapshot::check_target(&self.root, target)?;
        if snapshot::stamp(source)? != source_stamp || snapshot::stamp(target)? != *original {
            return Err(format!("{}: {}", target.display(), t!("transfer.changed")));
        }
        if !self.authorize(target).await? {
            return Err(format!(
                "{}: {}",
                target.display(),
                t!("transfer.dirty_target")
            ));
        }
        // Consent may take time; repeat disk checks before publishing or deleting anything.
        self.check_cancel()?;
        snapshot::check_target(&self.root, target)?;
        if snapshot::stamp(source)? != source_stamp || snapshot::stamp(target)? != *original {
            return Err(format!("{}: {}", target.display(), t!("transfer.changed")));
        }
        if target.is_dir() {
            fs::remove_dir_all(target).map_err(|error| error.to_string())?;
        }
        if let Err(error) = temporary.persist(target) {
            snapshot::restore(&before, target)?;
            return Err(format!("{}: {error}", target.display()));
        }
        let after = snapshot::stamp(target)?;
        if self.kind == Kind::Move {
            if let Err(error) = fs::remove_file(source) {
                snapshot::restore(&before, target)?;
                return Err(format!("{}: {error}", source.display()));
            }
        }
        self.receipt.changes.push(Change {
            path: target.to_path_buf(),
            before,
            after,
        });
        if let Some(before) = source_before {
            self.receipt.changes.push(Change {
                path: source.to_path_buf(),
                before,
                after: Stamp::Missing,
            });
            self.receipt
                .moves
                .push((source.to_path_buf(), target.to_path_buf()));
            for file in &mut self.watched {
                if file.path == source {
                    file.path = target.to_path_buf();
                }
            }
            let (reply, applied) = oneshot::channel();
            self.events
                .unbounded_send(Event::Moved {
                    old: source.to_path_buf(),
                    new: target.to_path_buf(),
                    reply,
                })
                .map_err(|_| t!("transfer.window_closed").to_string())?;
            let _ = applied.await;
        }
        self.completed += 1;
        let _ = self.events.unbounded_send(Event::Progress {
            path: target.to_path_buf(),
            completed: self.completed,
            bytes: self.bytes,
        });
        Ok(())
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
