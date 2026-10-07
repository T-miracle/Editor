//! Prepare disk data in the background and publish with a short UI-owned authorization boundary.
use super::fingerprint::Node;
use super::operation::Approval;
use super::snapshot::{self, Snapshot};
use rust_i18n::t;
use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

/// A fast final identity check supplements the complete content checks made before submission.
#[derive(PartialEq)]
struct Identity {
    len: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    directory: bool,
    readonly: bool,
}
fn identity(path: &Path) -> Result<Option<Identity>, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if !snapshot::is_link(&meta) => Ok(Some(Identity {
            len: meta.len(),
            modified: meta.modified().ok(),
            created: meta.created().ok(),
            directory: meta.is_dir(),
            readonly: meta.permissions().readonly(),
        })),
        Ok(_) => Err(t!("transfer.link_target").to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

/// Same-volume parking makes failure rollback a rename instead of copying a large private backup.
pub(super) struct Entry {
    pub path: PathBuf,
    pub before: Snapshot,
    stage: tempfile::TempDir,
    expected: Option<Identity>,
    replacement: bool,
    parked: bool,
    installed: bool,
    pub after: Node,
}
impl Entry {
    fn new(path: &Path, before: Snapshot) -> Result<Self, String> {
        let stage = tempfile::Builder::new()
            .prefix(".me-transfer-stage-")
            .tempdir_in(
                path.parent()
                    .ok_or_else(|| t!("transfer.missing_parent").to_string())?,
            )
            .map_err(|error| error.to_string())?;
        Ok(Self {
            path: path.to_path_buf(),
            before,
            stage,
            expected: identity(path)?,
            replacement: false,
            parked: false,
            installed: false,
            after: Node::Missing,
        })
    }
    /// All copying, syncing and recursive directory preparation happens before the UI receives this entry.
    pub fn restore(path: &Path, saved: &Snapshot, before: Snapshot) -> Result<Self, String> {
        let mut entry = Self::new(path, before)?;
        if !matches!(saved, Snapshot::Missing) {
            snapshot::restore(saved, &entry.stage.path().join("new"))?;
            entry.after = Node::read(&entry.stage.path().join("new"))?;
            entry.replacement = true;
        }
        Ok(entry)
    }
    /// Adopt fully copied and synced bytes; the temporary file must already live on the destination volume.
    pub fn file(
        path: &Path,
        file: tempfile::NamedTempFile,
        before: Snapshot,
    ) -> Result<Self, String> {
        let mut entry = Self::new(path, before)?;
        file.persist(entry.stage.path().join("new"))
            .map_err(|error| error.to_string())?;
        entry.after = Node::read(&entry.stage.path().join("new"))?;
        entry.replacement = true;
        Ok(entry)
    }
    /// Current source removals are parked first, exposing delete/rename permission errors before publication.
    pub fn remove(path: &Path, before: Snapshot) -> Result<Self, String> {
        Self::new(path, before)
    }

    /// Only incomplete rollback endpoints differ from their preimage after a failed transaction.
    pub fn changed(&self) -> bool {
        self.parked || self.installed
    }
}

pub(super) enum Protection {
    Ordinary {
        targets: Vec<PathBuf>,
    },
    Recovery {
        paths: Vec<PathBuf>,
        preserve: Vec<PathBuf>,
        approvals: Vec<Approval>,
        discard: bool,
    },
}

/// Ownership returns to the worker even after failure, so recursive RAII cleanup never runs on the UI.
pub(super) struct Plan {
    pub entries: Vec<Entry>,
    pub protection: Protection,
    pub root: Option<PathBuf>,
    pub movement: Option<(PathBuf, PathBuf)>,
}
pub(super) struct ResultWithPlan {
    pub plan: Plan,
    pub result: Result<(), String>,
    pub approvals: Vec<Approval>,
}

impl Plan {
    /// Validate every endpoint, then perform only renames; no hashes or recursive removal are allowed here.
    pub fn commit(&mut self) -> Result<(), String> {
        for entry in &self.entries {
            if let Some(root) = &self.root {
                // Ordinary moves may remove an offered external source, but writes remain inside the workspace.
                if entry.path.starts_with(root) {
                    snapshot::check_target(root, &entry.path)?;
                } else if entry.replacement {
                    return Err(t!("transfer.outside_workspace").to_string());
                } else {
                    snapshot::check_ancestors(&entry.path)?;
                }
            } else {
                snapshot::check_ancestors(&entry.path)?;
            }
            if identity(&entry.path)? != entry.expected {
                return Err(format!(
                    "{}: {}",
                    entry.path.display(),
                    t!("transfer.changed")
                ));
            }
        }
        for index in 0..self.entries.len() {
            let entry = &mut self.entries[index];
            let result = (|| {
                if entry.expected.is_some() {
                    fs::rename(&entry.path, entry.stage.path().join("old"))
                        .map_err(|error| error.to_string())?;
                    entry.parked = true;
                }
                if entry.replacement {
                    fs::rename(entry.stage.path().join("new"), &entry.path)
                        .map_err(|error| error.to_string())?;
                    entry.installed = true;
                }
                Ok::<_, String>(())
            })();
            if let Err(error) = result {
                let mut failures = vec![format!("{}: {error}", entry.path.display())];
                for entry in self.entries.iter_mut().take(index + 1).rev() {
                    if entry.installed {
                        match fs::rename(&entry.path, entry.stage.path().join("new")) {
                            Ok(()) => entry.installed = false,
                            Err(error) => {
                                failures.push(format!("{}: {error}", entry.path.display()))
                            }
                        }
                    }
                    if entry.parked {
                        match fs::rename(entry.stage.path().join("old"), &entry.path) {
                            Ok(()) => entry.parked = false,
                            Err(error) => {
                                failures.push(format!("{}: {error}", entry.path.display()));
                                // A failed rollback retains the parked original instead of deleting the only remaining copy.
                                entry.stage.disable_cleanup(true);
                                failures.push(format!(
                                    "{}: {}",
                                    entry.stage.path().display(),
                                    t!("transfer.rollback_backup")
                                ));
                            }
                        }
                    }
                }
                return Err(failures.join("\n"));
            }
        }
        Ok(())
    }

    /// Cleanup is explicit on the worker so failed deletions are reported and retained originals stay recoverable.
    pub fn cleanup(self, committed: bool) -> Result<(), String> {
        let mut failures = Vec::new();
        for entry in self.entries {
            if !committed && entry.parked {
                let _ = entry.stage.keep();
                continue;
            }
            let path = entry.stage.path().to_path_buf();
            if let Err(error) = entry.stage.close() {
                failures.push(format!("{}: {error}", path.display()));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("\n"))
        }
    }
}
