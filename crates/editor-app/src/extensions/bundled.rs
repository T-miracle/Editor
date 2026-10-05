//! Generic first-use installation keeps consent and immutable packages apart from plugin management UI.
use super::*;
use gpui_kit::component::WindowExt;
use plugin_runtime::Manager;
use std::sync::atomic::{AtomicBool, Ordering};

mod catalog;

/// A host-owned request is withdrawn immediately when its active file or workspace authority changes.
#[derive(Clone)]
pub(super) struct Request {
    pub token: u64,
    pub workspace: PathBuf,
    pub runtime_root: PathBuf,
    pub file: PathBuf,
    pub active: Arc<AtomicBool>,
}

impl Request {
    /// Capture immutable host paths; production catalog locations never come from project configuration.
    pub(super) fn new(
        token: u64,
        workspace: PathBuf,
        runtime_root: PathBuf,
        file: PathBuf,
    ) -> Self {
        Self {
            token,
            workspace,
            runtime_root,
            file,
            active: Arc::new(AtomicBool::new(true)),
        }
    }

    /// Worker-side checks share this flag with UI withdrawal, including requests still in its channel.
    pub(super) fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    /// Cancellation revokes the candidate without modifying plugin enablement or granting permissions.
    fn cancel(&self) {
        self.active.store(false, Ordering::Release);
    }
}

/// The displayed package is the exact hash-checked value later installed; confirmation never rereads its ZIP.
#[derive(Clone)]
pub(super) struct Candidate {
    pub request: Request,
    pub package: Arc<Package>,
}

/// A separate bounded reply does not set the manager's manual-package `pending` field or open its window.
pub(super) struct Reply {
    pub token: u64,
    pub result: Result<Option<Candidate>, String>,
}

/// A closed, redirected or foreign file cannot authorize first-use work on a different active workspace.
fn request_paths(manager: &Manager, request: &Request) -> anyhow::Result<()> {
    anyhow::ensure!(request.is_active(), "Bundled request was withdrawn");
    let workspace = request.workspace.canonicalize()?;
    let file = request.file.canonicalize()?;
    anyhow::ensure!(
        file.starts_with(&workspace)
            && file.is_file()
            && request.runtime_root.canonicalize()? == manager.root().canonicalize()?,
        "Bundled request does not own its file or runtime store"
    );
    Ok(())
}

/// All native consumers share the same extension mapping, including an explicitly disabled preview choice.
pub(super) fn matches_editor_preview(entry: &Installed, file: &Path) -> bool {
    file.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            entry.manifest.panels.iter().any(|panel| {
                panel.position == "editor"
                    && panel
                        .file_extensions
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(extension))
            })
        })
}

/// Alternative preview or recognition declarations reserve their selectors even when explicitly disabled.
fn has_provider(manager: &Manager, file: &Path) -> anyhow::Result<bool> {
    let extension = file
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let filename = file
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    for entry in manager.installed.values() {
        if matches_editor_preview(entry, file) {
            return Ok(true);
        }
        if let Some(contribution) = catalog::contribution(manager, entry)?
            && contribution.language_definitions.iter().any(|language| {
                language
                    .extensions
                    .iter()
                    .any(|value| value.eq_ignore_ascii_case(extension))
                    || language
                        .filenames
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(filename))
            })
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Shipped selectors must be backed by real validated package declarations, not an unaudited index label.
fn declared(package: &Package, extensions: &[String]) -> anyhow::Result<()> {
    let contribution = package
        .manifest
        .contributions
        .as_ref()
        .map(|name| {
            let bytes = package
                .files
                .get(name)
                .ok_or_else(|| anyhow::anyhow!("Missing bundled contributions"))?;
            plugin_schema::PluginManifest::parse(std::str::from_utf8(bytes)?)
                .map_err(anyhow::Error::from)
        })
        .transpose()?;
    let supported = extensions.iter().all(|extension| {
        let preview = package.manifest.panels.iter().any(|panel| {
            panel.position == "editor"
                && panel
                    .file_extensions
                    .iter()
                    .any(|value| value.eq_ignore_ascii_case(extension))
        });
        let recognition = contribution.as_ref().is_some_and(|manifest| {
            manifest.language_definitions.iter().any(|language| {
                language
                    .extensions
                    .iter()
                    .any(|value| value.eq_ignore_ascii_case(extension))
            })
        });
        preview || recognition
    });
    anyhow::ensure!(
        supported,
        "Bundled index selectors are not declared by its package"
    );
    Ok(())
}

/// Inspect through ordinary package validation and record exposure before publishing consent, without execution.
pub(super) fn prepare_offer(
    manager: &mut Manager,
    request: &Request,
) -> anyhow::Result<Option<Candidate>> {
    if !request.is_active() {
        return Ok(None);
    }
    request_paths(manager, request)?;
    if has_provider(manager, &request.file)? {
        return Ok(None);
    }
    let Some((package, extensions)) = catalog::matching(request)? else {
        return Ok(None);
    };
    declared(&package, &extensions)?;
    if !request.is_active()
        || !manager.can_offer_bundle(&package.manifest.id, &request.workspace)?
    {
        return Ok(None);
    }
    manager.record_bundle_offer(&package.manifest.id)?;
    if !request.is_active() {
        return Ok(None);
    }
    Ok(Some(Candidate {
        request: request.clone(),
        package: Arc::new(package),
    }))
}

/// Recheck the current store, native request and competing providers immediately before preparation/cutover.
pub(super) fn validate_candidate(manager: &Manager, candidate: &Candidate) -> anyhow::Result<()> {
    request_paths(manager, &candidate.request)?;
    anyhow::ensure!(
        manager.can_install_bundle(&candidate.package.manifest.id, &candidate.request.workspace)?
            && !has_provider(manager, &candidate.request.file)?,
        "Bundled candidate is no longer available"
    );
    Ok(())
}

/// Only document identity and paths govern exposure; typing does not recreate a first-use installation prompt.
#[derive(Clone, PartialEq, Eq)]
struct Source {
    document: String,
    file: PathBuf,
    workspace: PathBuf,
}

/// One in-flight request and one immutable candidate bound resource retention and repeated shell repaints.
#[derive(Default)]
pub(super) struct State {
    pub ready: bool,
    source: Option<Source>,
    request: Option<Request>,
    candidate: Option<Candidate>,
    pub dialog_open: bool,
    token: u64,
}

impl State {
    /// Trust withdrawal is immediate; the next shell render closes only the still-owned confirmation.
    pub(super) fn cancel_request(&self) {
        if let Some(request) = &self.request {
            request.cancel();
        }
    }
    /// Accept only the still-current reply; late replies drop their package without granting authority.
    pub(super) fn accept(&mut self, reply: Reply) -> Option<String> {
        if !self
            .request
            .as_ref()
            .is_some_and(|request| request.token == reply.token && request.is_active())
        {
            return None;
        }
        match reply.result {
            Ok(candidate) => {
                self.candidate = candidate.filter(|candidate| {
                    candidate.request.is_active()
                        && self.request.as_ref().is_some_and(|request| {
                            Arc::ptr_eq(&candidate.request.active, &request.active)
                        })
                });
                None
            }
            Err(error) => {
                self.candidate = None;
                Some(error)
            }
        }
    }

    /// Release retained bytes and cancel queued work before a new active-document identity can request a bundle.
    fn withdraw(&mut self) -> bool {
        if let Some(request) = self.request.take() {
            request.cancel();
        }
        self.candidate = None;
        self.source = None;
        std::mem::take(&mut self.dialog_open)
    }

    /// The common consent builder detects bundled ownership by the already-validated package digest.
    pub(super) fn confirmation(&self, package: &Package) -> Option<Candidate> {
        self.candidate
            .as_ref()
            .filter(|candidate| {
                self.dialog_open
                    && candidate.request.is_active()
                    && candidate.package.digest == package.digest
            })
            .cloned()
    }

    /// Confirmation rechecks native document identity, rather than waiting for the next shell repaint.
    pub(super) fn source_current(&self, app: &EditorApp) -> bool {
        self.source.as_ref().is_some_and(|source| {
            app.session_state.workspace_trusted
                && app.active_path.as_ref() == Some(&source.file)
                && app.workspace.root() == source.workspace
                && app
                    .tabs
                    .iter()
                    .position(|tab| tab.owns_editor(&app.editor))
                    .and_then(|index| app.plugin_document_version(index).ok())
                    .is_some_and(|document| document.id == source.document)
                && crate::language::providers::language_for_path(&source.file).is_none()
        })
    }

    /// Closing consent leaves the request token alive for an accepted installation's final cutover check.
    pub(super) fn finish(&mut self) {
        self.dialog_open = false;
        self.candidate = None;
    }
}

impl Drop for State {
    /// Workspace/window destruction revokes pending work even when no further shell frame can run.
    fn drop(&mut self) {
        self.cancel_request();
    }
}

impl EditorApp {
    /// Document-identity mutation revokes queued/confirmed work immediately, before another worker turn.
    /// Call only when the active entity/path truly changes; ordinary edits and repeated activation retain consent.
    pub(crate) fn withdraw_bundled_request(&self, cx: &mut Context<Self>) {
        self.extensions
            .update(cx, |panel, _| panel.bundled.cancel_request());
    }

    /// First-use consent lives in the main native editor window and never forces plugin management open.
    pub(crate) fn sync_bundled_first_use(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let source = self
            .tabs
            .iter()
            .position(|tab| tab.owns_editor(&self.editor))
            .and_then(|index| self.plugin_document_version(index).ok())
            .zip(self.active_path.clone())
            .map(|(document, file)| Source {
                document: document.id,
                file,
                workspace: self.workspace.root().to_owned(),
            });
        let trusted = self.session_state.workspace_trusted;
        let language_selected = self
            .active_path
            .as_ref()
            .and_then(|path| crate::language::providers::language_for_path(path))
            .is_some();
        let root = self.extensions.clone();
        root.update(cx, |panel, cx| {
            let ready = panel.bundled.ready && panel.worker.trusted.load(Ordering::Acquire);
            let reserved = source.as_ref().is_some_and(|source| {
                panel
                    .entries
                    .iter()
                    .any(|entry| matches_editor_preview(entry, &source.file))
            });
            let current = if trusted && ready && !language_selected && !reserved {
                source
            } else {
                None
            };
            let withdrawn = panel
                .bundled
                .request
                .as_ref()
                .is_some_and(|request| !request.is_active());
            if panel.bundled.source != current || withdrawn {
                if panel.bundled.withdraw() {
                    window.close_dialog(cx);
                }
                panel.bundled.source = current;
            }
            let Some(source) = panel.bundled.source.clone() else {
                return;
            };
            if panel.progress.is_some()
                || panel.pending.is_some()
                || panel.confirm.is_some()
                || self.plugin_popup.is_some()
                || self.explorer_menu.is_some()
                || self.explorer_edit.is_some()
                || self.explorer_delete.is_some()
                || window.has_active_sheet(cx)
            {
                return;
            }
            if !panel.bundled.dialog_open && window.has_active_dialog(cx) {
                return;
            }
            if panel.bundled.request.is_none() {
                panel.bundled.token = panel.bundled.token.wrapping_add(1);
                let request = Request::new(
                    panel.bundled.token,
                    source.workspace,
                    panel.root.clone(),
                    source.file,
                );
                panel.bundled.request = Some(request.clone());
                let _ = panel.worker.tx.send(Work::InspectBundle(request));
            }
            if !panel.bundled.dialog_open
                && let Some(candidate) = panel.bundled.candidate.clone()
            {
                panel.bundled.dialog_open = true;
                panel.open_install_dialog(candidate.package.clone(), window, cx);
            }
        });
    }
}
