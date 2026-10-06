//! Watches filesystem changes off the UI thread and reconciles explorer snapshots.

use editor_core::{Workspace, WorkspaceSnapshot};
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use notify::{Event, EventKind, RecursiveMode, Watcher, event::ModifyKind, event::RenameMode};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::mpsc::{self as sync_mpsc, RecvTimeoutError, Sender},
    time::{Duration, Instant},
};

/// Watch metadata chooses the authorized file model before any text decoding.
#[derive(Clone)]
pub(crate) struct WatchedFile {
    pub(crate) path: PathBuf,
    pub(crate) text: bool,
}

/// Opaque files publish only a fingerprint; their bytes are read by the owned resource loader.
pub(crate) enum DiskContent {
    Text(String),
    FileDigest([u8; 32]),
}

impl From<&str> for DiskContent {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

impl From<String> for DiskContent {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

const RETRY_INTERVAL: Duration = Duration::from_secs(30);
const MAX_SCAN_INTERVAL: Duration = Duration::from_secs(300);
const EVENT_DEBOUNCE: Duration = Duration::from_millis(180);

/// The editor applies a completed scan only after the worker finishes reading disk state.
pub(crate) struct Reconciliation {
    pub(crate) snapshot: Option<WorkspaceSnapshot>,
    pub(crate) documents: Vec<(PathBuf, std::io::Result<DiskContent>, Instant)>,
    pub(crate) renames: Vec<(PathBuf, PathBuf)>,
    pub(crate) native: bool,
}

enum Command {
    Event(notify::Result<Event>),
    Reconcile,
    Documents(Vec<WatchedFile>),
    Stop,
}

/// Owns a native watcher and a channel to its single background worker.
pub(crate) struct FileWatch {
    sender: Sender<Command>,
}

impl FileWatch {
    pub(crate) fn start(workspace: Workspace) -> (Self, UnboundedReceiver<Reconciliation>) {
        let (sender, receiver) = sync_mpsc::channel();
        let (updates, incoming) = mpsc::unbounded();
        // GPUI's deterministic test scheduler forbids untracked native threads.
        #[cfg(test)]
        {
            let _ = (workspace, receiver, updates);
            return (Self { sender }, incoming);
        }
        #[cfg(not(test))]
        {
            let worker_sender = sender.clone();
            std::thread::Builder::new()
                .name("editor-file-watch".into())
                .spawn(move || run_worker(workspace, receiver, worker_sender, updates))
                .expect("file watcher worker must start");
            (Self { sender }, incoming)
        }
    }

    /// Request a full comparison on explicit refresh or window activation.
    pub(crate) fn reconcile(&self) {
        let _ = self.sender.send(Command::Reconcile);
    }

    /// Watch parent directories of open files outside the workspace.
    pub(crate) fn set_documents(&self, paths: Vec<WatchedFile>) {
        let _ = self.sender.send(Command::Documents(paths));
    }
}

impl Drop for FileWatch {
    fn drop(&mut self) {
        let _ = self.sender.send(Command::Stop);
    }
}

/// Keep all filesystem traversal, document reads, and fallback retry off the UI thread.
fn run_worker(
    workspace: Workspace,
    receiver: sync_mpsc::Receiver<Command>,
    sender: Sender<Command>,
    updates: UnboundedSender<Reconciliation>,
) {
    let root = workspace.root().to_path_buf();
    let mut documents = Vec::new();
    let mut watched_external = HashSet::new();
    let mut watcher = match start_native(&root, &sender) {
        Ok(native) => Some(native),
        Err(error) => {
            tracing::warn!(%error, "native workspace watch unavailable; using adaptive scan");
            None
        }
    };
    let mut scan_interval = RETRY_INTERVAL;
    let mut pending_renames = Vec::new();
    let mut rename_sources = HashMap::new();
    let mut last_snapshot = None;
    // The first scan establishes the tree after the UI has restored its tabs.
    if !send_snapshot(
        &workspace,
        &documents,
        &mut pending_renames,
        watcher.is_some(),
        true,
        &mut last_snapshot,
        &updates,
    ) {
        return;
    }
    loop {
        let command = if watcher.is_some() {
            match receiver.recv() {
                Ok(command) => command,
                Err(_) => break,
            }
        } else {
            match receiver.recv_timeout(scan_interval) {
                Ok(command) => command,
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    // A failed native watcher retries while a low-frequency scan repairs missed events.
                    watcher = start_native(&root, &sender).ok();
                    if let Some(native) = watcher.as_mut() {
                        watched_external.clear();
                        if !watch_external(native, &root, &documents, &mut watched_external) {
                            watcher = None;
                        }
                        scan_interval = RETRY_INTERVAL;
                    }
                    let started = Instant::now();
                    if !send_snapshot(
                        &workspace,
                        &documents,
                        &mut pending_renames,
                        watcher.is_some(),
                        true,
                        &mut last_snapshot,
                        &updates,
                    ) {
                        break;
                    }
                    if watcher.is_none() {
                        scan_interval = (scan_interval + started.elapsed()).min(MAX_SCAN_INTERVAL);
                    }
                    continue;
                }
            }
        };
        match command {
            Command::Stop => break,
            Command::Documents(paths) => {
                documents = paths;
                if let Some(native) = watcher.as_mut() {
                    if !watch_external(native, &root, &documents, &mut watched_external) {
                        watcher = None;
                    }
                }
                if !send_snapshot(
                    &workspace,
                    &documents,
                    &mut pending_renames,
                    watcher.is_some(),
                    false,
                    &mut last_snapshot,
                    &updates,
                ) {
                    break;
                }
            }
            Command::Reconcile => {
                if !send_snapshot(
                    &workspace,
                    &documents,
                    &mut pending_renames,
                    watcher.is_some(),
                    true,
                    &mut last_snapshot,
                    &updates,
                ) {
                    break;
                }
            }
            Command::Event(result) => {
                let Some(mut scan_tree) = record_event(
                    result,
                    &root,
                    &documents,
                    &mut pending_renames,
                    &mut rename_sources,
                    &mut watcher,
                ) else {
                    continue;
                };
                // Coalesce editor saves and bulk file operations into a single traversal.
                let until = Instant::now() + EVENT_DEBOUNCE;
                while let Some(wait) = until.checked_duration_since(Instant::now()) {
                    match receiver.recv_timeout(wait) {
                        Ok(Command::Event(result)) => {
                            scan_tree |= record_event(
                                result,
                                &root,
                                &documents,
                                &mut pending_renames,
                                &mut rename_sources,
                                &mut watcher,
                            )
                            .unwrap_or(false);
                        }
                        Ok(Command::Documents(paths)) => {
                            documents = paths;
                            if let Some(native) = watcher.as_mut() {
                                if !watch_external(native, &root, &documents, &mut watched_external)
                                {
                                    watcher = None;
                                }
                            }
                        }
                        Ok(Command::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                        Ok(Command::Reconcile) => {
                            scan_tree = true;
                            break;
                        }
                        Err(RecvTimeoutError::Timeout) => break,
                    }
                }
                if !send_snapshot(
                    &workspace,
                    &documents,
                    &mut pending_renames,
                    watcher.is_some(),
                    scan_tree,
                    &mut last_snapshot,
                    &updates,
                ) {
                    break;
                }
                // A late unmatched rename half is ambiguous and remains a delete/add.
                rename_sources.clear();
            }
        }
    }
}

/// Retry the OS-native implementation; never silently substitute notify's polling watcher.
fn start_native(
    root: &Path,
    sender: &Sender<Command>,
) -> notify::Result<notify::RecommendedWatcher> {
    let sender = sender.clone();
    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = sender.send(Command::Event(event));
    })?;
    watcher.watch(root, RecursiveMode::Recursive)?;
    Ok(watcher)
}

/// Reinstall external parent watches when the open-tab set changes.
fn watch_external(
    watcher: &mut notify::RecommendedWatcher,
    root: &Path,
    documents: &[WatchedFile],
    watched: &mut HashSet<PathBuf>,
) -> bool {
    let desired: HashSet<_> = documents
        .iter()
        .filter(|file| !file.path.starts_with(root))
        .filter_map(|file| file.path.parent().map(Path::to_path_buf))
        .collect();
    for path in watched.difference(&desired) {
        let _ = watcher.unwatch(path);
    }
    let mut healthy = true;
    for path in desired.difference(watched) {
        if let Err(error) = watcher.watch(path, RecursiveMode::NonRecursive) {
            tracing::warn!(%error, path = %path.display(), "external file watch failed");
            healthy = false;
        }
    }
    *watched = desired;
    healthy
}

/// Only a paired rename event is safe enough to transfer an open tab's path.
fn record_event(
    result: notify::Result<Event>,
    root: &Path,
    documents: &[WatchedFile],
    renames: &mut Vec<(PathBuf, PathBuf)>,
    rename_sources: &mut HashMap<usize, PathBuf>,
    watcher: &mut Option<notify::RecommendedWatcher>,
) -> Option<bool> {
    let event = match result {
        Ok(event) => event,
        Err(error) => {
            tracing::warn!(%error, "native file watch failed; using adaptive scan");
            *watcher = None;
            return Some(true);
        }
    };
    if event.need_rescan() || event.paths.is_empty() {
        // Overflow and pathless notices cannot be safely handled incrementally.
        return Some(true);
    }
    match event.kind {
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
            if let [old, new] = event.paths.as_slice() {
                renames.push((old.clone(), new.clone()));
            }
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
            if let (Some(tracker), [old]) = (event.attrs.tracker(), event.paths.as_slice()) {
                rename_sources.insert(tracker, old.clone());
            }
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
            if let (Some(tracker), [new]) = (event.attrs.tracker(), event.paths.as_slice())
                && let Some(old) = rename_sources.remove(&tracker)
            {
                renames.push((old, new.clone()));
            }
        }
        _ => {}
    }
    let relevant = event.paths.iter().any(|path| {
        if documents.iter().any(|document| document.path == *path) {
            return true;
        }
        if path.starts_with(root) {
            // Build output and Git metadata never appear in the explorer tree.
            !path.strip_prefix(root).is_ok_and(|relative| {
                relative
                    .components()
                    .any(|part| part.as_os_str() == ".git" || part.as_os_str() == "target")
            })
        } else {
            documents.iter().any(|document| document.path == *path)
        }
    });
    if !relevant {
        return None;
    }
    // Content and metadata changes never alter the explorer's path set.
    let content_only = matches!(
        event.kind,
        EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Metadata(_))
    ) && !event.paths.iter().any(|path| {
        path.file_name()
            .is_some_and(|name| name == ".gitignore" || name == ".ignore")
    });
    Some(!content_only)
}

/// A scan visits the workspace once; opaque file checks use a bounded streaming buffer.
fn send_snapshot(
    workspace: &Workspace,
    documents: &[WatchedFile],
    renames: &mut Vec<(PathBuf, PathBuf)>,
    native: bool,
    scan_tree: bool,
    last_snapshot: &mut Option<WorkspaceSnapshot>,
    updates: &UnboundedSender<Reconciliation>,
) -> bool {
    // Compare in the worker so unchanged trees never cross into UI rebuild work.
    let snapshot = scan_tree
        .then(|| workspace.snapshot())
        .and_then(|snapshot| {
            if last_snapshot.as_ref() == Some(&snapshot) {
                None
            } else {
                *last_snapshot = Some(snapshot.clone());
                Some(snapshot)
            }
        });
    let documents = documents
        .iter()
        .map(|file| {
            // Timestamp before I/O so a later local save can reject an older read.
            let read_at = Instant::now();
            let contents = if file.text {
                std::fs::read_to_string(&file.path).map(DiskContent::Text)
            } else {
                file_fingerprint(&file.path).map(DiskContent::FileDigest)
            };
            (file.path.clone(), contents, read_at)
        })
        .collect();
    updates
        .unbounded_send(Reconciliation {
            snapshot,
            documents,
            renames: std::mem::take(renames),
            native,
        })
        .is_ok()
}

/// Fingerprints do not retain binary contents or allocate in proportion to the file size.
fn file_fingerprint(path: &Path) -> std::io::Result<[u8; 32]> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[test]
    fn background_scan_emits_tree_only_when_paths_change() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::open(directory.path()).unwrap();
        let (sender, mut receiver) = mpsc::unbounded();
        let mut renames = Vec::new();
        let mut previous = None;
        for _ in 0..2 {
            assert!(send_snapshot(
                &workspace,
                &[],
                &mut renames,
                true,
                true,
                &mut previous,
                &sender
            ));
        }
        let first = futures::executor::block_on(receiver.next()).unwrap();
        let unchanged = futures::executor::block_on(receiver.next()).unwrap();
        assert!(first.snapshot.is_some());
        assert!(unchanged.snapshot.is_none());

        // A newly created path must produce a fresh tree snapshot.
        std::fs::write(directory.path().join("new.txt"), "content").unwrap();
        assert!(send_snapshot(
            &workspace,
            &[],
            &mut renames,
            true,
            true,
            &mut previous,
            &sender
        ));
        let changed = futures::executor::block_on(receiver.next()).unwrap();
        assert_eq!(changed.snapshot.unwrap().files.len(), 1);
    }

    #[test]
    fn only_paired_rename_updates_a_tab_path() {
        let root = PathBuf::from("/workspace");
        let mut renames = Vec::new();
        let mut sources = HashMap::new();
        let mut watcher = None;
        let event = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(root.join("old.rs"))
            .add_path(root.join("new.rs"));
        assert_eq!(
            record_event(
                Ok(event),
                &root,
                &[],
                &mut renames,
                &mut sources,
                &mut watcher
            ),
            Some(true)
        );
        assert_eq!(renames, vec![(root.join("old.rs"), root.join("new.rs"))]);
    }

    #[test]
    fn tracked_rename_halves_pair_but_untracked_halves_do_not() {
        let root = PathBuf::from("/workspace");
        let mut renames = Vec::new();
        let mut sources = HashMap::new();
        let mut watcher = None;
        let from = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::From)))
            .add_path(root.join("old.rs"))
            .set_tracker(17);
        let to = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::To)))
            .add_path(root.join("new.rs"))
            .set_tracker(17);
        record_event(
            Ok(from),
            &root,
            &[],
            &mut renames,
            &mut sources,
            &mut watcher,
        );
        record_event(Ok(to), &root, &[], &mut renames, &mut sources, &mut watcher);
        assert_eq!(renames, vec![(root.join("old.rs"), root.join("new.rs"))]);

        let lone = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::To)))
            .add_path(root.join("other.rs"));
        record_event(
            Ok(lone),
            &root,
            &[],
            &mut renames,
            &mut sources,
            &mut watcher,
        );
        assert_eq!(renames.len(), 1);
    }
}
