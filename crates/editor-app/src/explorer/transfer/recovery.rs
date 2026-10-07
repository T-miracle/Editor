//! Restore verified preimages and journal actual inverse changes; move endpoints recover together.
use super::operation::{Change, Choice, Decision, Event, Outcome, Receipt, Worker};
use super::snapshot::{self, Stamp};
use crate::editor::file_watch::{Reconciliation, read_documents};
use futures::channel::oneshot;
use rust_i18n::t;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};

impl Worker {
    /// Undo and redo both restore a receipt's preimages after inspecting today's filesystem state.
    pub(super) async fn recover(mut self, mut original: Receipt) -> Outcome {
        let initial: HashMap<_, _> = original
            .changes
            .iter()
            .map(|change| (change.path.clone(), snapshot::stamp(&change.path)))
            .collect();
        let mut handled = HashSet::new();
        let mut applied = HashSet::new();
        let mut error = None;
        for index in (0..original.changes.len()).rev() {
            if !handled.insert(index) {
                continue;
            }
            let path = &original.changes[index].path;
            let movement = original
                .moves
                .iter()
                .find(|(old, new)| old == path || new == path)
                .cloned();
            let mut group = vec![index];
            if let Some((old, new)) = &movement {
                for (other, change) in original.changes.iter().enumerate() {
                    if (change.path == *old || change.path == *new) && handled.insert(other) {
                        group.push(other);
                    }
                }
                group.sort_by(|a, b| b.cmp(a));
            }
            let result = self
                .recover_group(&original, &group, movement, &initial)
                .await;
            match result {
                Ok(true) => {
                    applied.extend(group);
                }
                Ok(false) => {}
                Err(failure) => {
                    error = Some(failure);
                    break;
                }
            }
        }
        original.changes = original
            .changes
            .into_iter()
            .enumerate()
            .filter_map(|(index, change)| (!applied.contains(&index)).then_some(change))
            .collect();
        original.moves.retain(|(old, new)| {
            original
                .changes
                .iter()
                .any(|change| change.path == *old || change.path == *new)
        });
        // Inverse directory identities describe the complete result, not an intermediate empty container.
        for change in &mut self.receipt.changes {
            if let Ok(stamp) = snapshot::stamp(&change.path) {
                change.after = stamp;
            }
        }
        let reconciliation = self.reconcile_current_documents().await;
        Outcome {
            reconciliation,
            receipt: self.receipt,
            completed: self.completed,
            skipped: self.skipped,
            error,
            remaining: original.changes.len(),
            cancelled: self.cancelled.load(Ordering::Relaxed),
            pending: (!original.changes.is_empty()).then_some(original),
        }
    }

    /// A move pair is one recovery unit, so declining an occupied source never deletes the destination.
    async fn recover_group(
        &mut self,
        original: &Receipt,
        group: &[usize],
        movement: Option<(PathBuf, PathBuf)>,
        initial: &HashMap<PathBuf, Result<Stamp, String>>,
    ) -> Result<bool, String> {
        self.check_cancel()?;
        let paths: Vec<_> = group
            .iter()
            .map(|index| original.changes[*index].path.clone())
            .collect();
        for path in &paths {
            check_recovery_path(path)?;
        }
        let changed = group.iter().any(|index| {
            initial
                .get(&original.changes[*index].path)
                .is_none_or(|stamp| stamp.as_ref().ok() != Some(&original.changes[*index].after))
        });
        let preserve = movement
            .as_ref()
            .map(|(_, new)| vec![new.clone()])
            .unwrap_or_default();
        let (reply, answer) = oneshot::channel();
        self.events
            .unbounded_send(Event::ReviewRecovery {
                paths: paths.clone(),
                preserve,
                changed,
                reply,
            })
            .map_err(|_| t!("transfer.window_closed").to_string())?;
        let Decision {
            choice, discard, ..
        } = answer
            .await
            .map_err(|_| t!("transfer.cancelled").to_string())?;
        match choice {
            Choice::Skip => {
                self.skipped.extend(paths);
                return Ok(false);
            }
            Choice::Cancel => {
                self.cancelled.store(true, Ordering::Relaxed);
                return self.check_cancel().map(|_| false);
            }
            Choice::Force => {}
            _ => return Err(t!("transfer.cancelled").to_string()),
        }
        self.check_cancel()?;
        let mut before = Vec::new();
        for index in group {
            let change = &original.changes[*index];
            check_recovery_path(&change.path)?;
            let current = snapshot::stamp(&change.path)?;
            if !matches!(change.after, Stamp::Directory(_))
                && initial
                    .get(&change.path)
                    .and_then(|stamp| stamp.as_ref().ok())
                    != Some(&current)
            {
                // A change after the review's inspection is new information, not covered by force consent.
                return Err(format!(
                    "{}: {}",
                    change.path.display(),
                    t!("transfer.changed")
                ));
            }
            // A parent container cannot remove a leaf the user just skipped, or a new unreviewed child.
            if matches!(change.after, Stamp::Directory(_))
                && !changed
                && std::fs::read_dir(&change.path)
                    .ok()
                    .is_some_and(|mut entries| entries.next().is_some())
            {
                self.skipped.push(change.path.clone());
                return Ok(false);
            }
            before.push((
                change.path.clone(),
                snapshot::capture(&change.path, self.receipt.backup.path())?,
                snapshot::stamp(&change.path)?,
            ));
        }
        let mut restored = 0;
        for (position, index) in group.iter().enumerate() {
            let change = &original.changes[*index];
            let result = (|| {
                check_recovery_path(&change.path)?;
                if snapshot::stamp(&change.path)? != before[position].2 {
                    return Err(t!("transfer.changed").to_string());
                }
                snapshot::restore(&change.before, &change.path)
            })();
            if let Err(failure) = result {
                let mut failures = vec![format!("{}: {failure}", change.path.display())];
                // Restore every attempted endpoint, including a directory restoration that failed midway.
                for (path, saved, _) in before.iter().take(restored + 1).rev() {
                    if let Err(rollback) = snapshot::restore(saved, path) {
                        failures.push(format!("{}: {rollback}", path.display()));
                    }
                }
                return Err(failures.join("\n"));
            }
            restored += 1;
        }
        for (path, before, _) in before {
            self.receipt.changes.push(Change {
                after: snapshot::stamp(&path)?,
                path,
                before,
            });
        }
        if discard {
            let documents = self
                .watched
                .iter()
                .filter(|file| paths.iter().any(|path| file.path.starts_with(path)))
                .cloned()
                .collect::<Vec<_>>();
            let (reply, applied) = oneshot::channel();
            let _ = self.events.unbounded_send(Event::Discard {
                update: Reconciliation {
                    snapshot: None,
                    documents: read_documents(&documents),
                    renames: Vec::new(),
                    native: true,
                },
                reply,
            });
            let _ = applied.await;
        }
        if let Some((old, new)) = movement {
            self.receipt.moves.push((new.clone(), old.clone()));
            for file in &mut self.watched {
                if file.path == new {
                    file.path = old.clone();
                }
            }
            let (reply, applied) = oneshot::channel();
            let _ = self.events.unbounded_send(Event::Moved {
                old: new,
                new: old,
                reply,
            });
            let _ = applied.await;
        }
        self.completed += 1;
        let _ = self.events.unbounded_send(Event::Progress {
            path: paths[0].clone(),
            completed: self.completed,
            bytes: 0,
        });
        Ok(true)
    }
}

/// Recovery may return to an explicitly recorded external source; every existing ancestor must remain a real directory.
fn check_recovery_path(path: &Path) -> Result<(), String> {
    snapshot::check_native_path(path)?;
    let mut ancestor = path;
    loop {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) if snapshot::is_link(&metadata) => {
                return Err(format!(
                    "{}: {}",
                    ancestor.display(),
                    t!("transfer.link_target")
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("{}: {error}", ancestor.display())),
        }
        match ancestor.parent() {
            Some(parent) => ancestor = parent,
            None => break,
        }
    }
    Ok(())
}
