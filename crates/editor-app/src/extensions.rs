//! Generic runtime-plugin dock and manager. Feature behavior arrives from installed packages.
mod bundled;
#[cfg(test)]
mod capability_tests;
pub(crate) mod command_menus;
mod commands;
#[cfg(test)]
mod community_document_tests;
#[cfg(test)]
pub(crate) mod composable_tests;
pub(crate) mod contributions;
#[cfg(test)]
mod dependency_tests;
mod editor_requests;
#[cfg(test)]
mod execution_service_tests;
mod image_input;
use crate::ui::plugin::images;
#[cfg(test)]
mod dock_tests;
#[cfg(test)]
mod file_view_tests;
#[cfg(test)]
mod hot_update_tests;
mod installation;
#[cfg(test)]
mod installer_tests;
#[cfg(test)]
mod interaction_tests;
#[cfg(test)]
pub(crate) mod language_tests;
#[cfg(test)]
mod layout_fault_tests;
#[cfg(test)]
mod layout_tests;
#[cfg(test)]
pub(crate) mod lsp_tests;
mod management;
#[cfg(test)]
mod management_tests;
#[cfg(test)]
mod markdown_tests;
#[cfg(test)]
mod native_build_tests;
#[cfg(test)]
pub(crate) mod native_configuration_tests;
mod native_controls;
#[cfg(test)]
mod native_discovery_tests;
#[cfg(test)]
mod native_run_tests;
#[cfg(test)]
mod native_ui_tests;
#[cfg(test)]
mod package_ui_test_support;
mod preview;
#[cfg(test)]
mod preview_tests;
mod recovery;
mod tools;
#[cfg(test)]
mod tools_tests;
pub(crate) use recovery::{level_label, log_time, severity_icon};
pub(crate) use tools::FunctionContext;
#[cfg(test)]
mod recovery_tests;
#[cfg(test)]
mod request_tests;
#[cfg(test)]
mod service_tests;
mod settings;
#[cfg(test)]
mod settings_tests;
#[cfg(test)]
mod shortcut_query_tests;
mod surface;
#[cfg(test)]
mod ui_package_tests;
pub(crate) use settings::SettingsView;
#[cfg(test)]
mod tests;
mod worker;
use crate::ui::controls::Input;
use crate::*;
use gpui_base::input::InputState;
use gpui_kit::{AnyElement, ClipboardItem, PathPromptOptions, SharedString};
use plugin_runtime::{
    Installed, Package,
    plugin_protocol::{self as protocol, api::Notification as PluginEvent},
};
#[cfg(test)]
use std::cell::RefCell;
use std::collections::BTreeMap;
pub(crate) use worker::DebugAnswerMessage;
pub(crate) use worker::configurations::{ConfigurationCatalog, ConfigurationReply};
pub(crate) use worker::targets::TargetCatalog;
pub use worker::{HostRunSnapshot, RunStatus, Work as HostWork};
use worker::{LifecycleAction, OperationProgress, Work, Worker};

actions!(extensions, [ToggleExtensions, QuitEditor]);
/// Opens plugin management independently of plugin-owned panel visibility.
pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "ctrl-`",
        ToggleExtensions,
        Some("EditorShell"),
    )]);
}

pub struct ExtensionPanel {
    /// Form observers redraw only when configuration publication changes.
    configuration_revision: u64,
    /// Log arrivals and viewing changes redraw every native consumer of the shared process history.
    log_generation: u64,
    parent: WeakEntity<EditorApp>,
    visible: Rc<Cell<bool>>,
    workspace: PathBuf,
    root: PathBuf,
    worker: Arc<Worker>,
    pub(crate) entries: Vec<Installed>,
    pub(crate) startup: BTreeMap<String, String>,
    views: HashMap<String, Arc<protocol::ui::Document>>,
    /// Raster images are paired with their exact scene and prepared outside the UI thread.
    images: images::SceneImages,
    active: Option<String>,
    surface_id: Option<String>,
    /// Editor-local surfaces are selected by the active document instead of the outer dock tree.
    editor_preview: bool,
    /// Auxiliary file functions receive compatible contexts but never own the center layout.
    editor_auxiliary: bool,
    /// The editor's revision token avoids copying unchanged documents on every shell repaint.
    preview_document: Option<(PathBuf, u64)>,
    /// New previews echo the open-document token so old drawing results cannot replace a newer file.
    preview_version: Option<protocol::api::DocumentVersion>,
    /// File-only scenes use their own resource authority, never a fake source revision.
    preview_file: Option<protocol::api::FileVersion>,
    /// True while the guest still owes a tree for the text this panel published last.
    /// One publication stays in flight so typing cannot rebuild the preview for every character.
    preview_pending: bool,
    /// Revisions that arrived while that publication was in flight; a bounded retry keeps a
    /// rejected notification from freezing the preview behind a guest that never answers.
    preview_superseded: u8,
    /// The newest revision already counted as superseded, so frames cannot inflate that count.
    preview_counted: Option<protocol::api::DocumentVersion>,
    /// A bounded publication cadence gives input priority without starving continuous typing.
    preview_waiting: bool,
    preview_ready: bool,
    /// An explicit toolbar click can flush coalesced text without applying against stale source bytes.
    pending_toolbar: Option<(
        u64,
        protocol::api::DocumentVersion,
        protocol::ui::Node,
        protocol::ui::UiEvent,
    )>,
    /// A host transport limit is shown against the current source instead of leaving an empty preview.
    preview_error: Option<String>,
    /// Only an authorized visible split enables either semantic viewport stream.
    viewport_sync_enabled: bool,
    source_viewport: preview::viewport::SourceTracking,
    panel_title: String,
    /// Cache package-owned artwork so repainting does not read from disk.
    panel_icons: [Option<Vec<u8>>; 2],
    /// One import attempt per live incarnation/type; failures retain the session record for retry.
    legacy_import: Option<(u64, String)>,
    panel_icon_digest: Option<String>,
    /// Immutable package icons are loaded once per version, outside the render path.
    tool_icons: HashMap<String, Option<Icon>>,
    focus: FocusHandle,
    /// Keep the footer control's keyboard focus stable when source toolbars change the render tree.
    footer_focus: FocusHandle,
    /// Actual command trigger bounds in window coordinates, refreshed during native prepaint.
    bounds: Bounds<Pixels>,
    /// Keyed native controls own text editing, canvas input and composition.
    native_ui: Option<Entity<crate::ui::plugin::PluginView>>,
    /// A source-local projection shares the approved UI version without duplicating document state.
    native_toolbar: Option<Entity<crate::ui::plugin::PluginView>>,
    manager_open: bool,
    commands_open: bool,
    command_popup: Option<Entity<crate::ui::controls::menu::PopupMenu>>,
    pending: Option<Package>,
    /// Prevent repainting from opening the same installation dialog repeatedly.
    pending_dialog_open: bool,
    /// First-use candidates belong to the active source, independently of the manager's manual ZIP flow.
    bundled: bundled::State,
    installation: Option<worker::InstallationProgress>,
    installation_dialog_open: bool,
    /// Search and selection belong to the manager window, not a plugin surface.
    manager_search: Option<Entity<InputState>>,
    manager_search_subscription: Option<Subscription>,
    manager_market: bool,
    manager_selected: Option<String>,
    /// Manager tabs own native focus independently of plugin surfaces and the search input.
    manager_tabs_focus: FocusHandle,
    manager_detail_focus: FocusHandle,
    /// Only the active detail page scrolls; the header and tab strip remain outside this handle.
    manager_detail_scroll: gpui_kit::ScrollHandle,
    manager_detail_tab: management::DetailTab,
    /// Entry boundary is captured once; later arrivals are read only when their message is visible.
    manager_log_view: Option<(String, u64)>,
    manager_packages: Vec<Package>,
    confirm: Option<(String, bool)>,
    /// Keep an uninstall confirmation to one overlay per click.
    confirm_dialog_open: bool,
    status: Option<worker::OperationStatus>,
    /// Viewing an ownerless failure confirms this publication; a later status always rearms it.
    manager_error_confirmed: std::cell::Cell<bool>,
    progress: Option<OperationProgress>,
    processes: HashMap<String, usize>,
    _task: gpui_kit::Task<()>,
    last_theme: Option<protocol::Environment>,
    last_size: (f32, f32, f32, f32),
    /// Last plugin instance generation measured against this panel's native canvas.
    instance_epoch: u64,
    /// Wait for the final snapshot before the native app shutdown deadline.
    _quit: Option<Subscription>,
    _focus_events: Vec<Subscription>,
}
impl ExtensionPanel {
    /// Protocol preparation runs on the worker; UI consumers receive only immutable approved leases.
    pub(crate) fn language_services(
        &self,
    ) -> BTreeMap<String, Result<Arc<plugin_runtime::LanguageService>, String>> {
        if !self
            .worker
            .trusted
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return BTreeMap::new();
        }
        self.worker.state.lock().unwrap().language_services.clone()
    }
    /// Outline consumers receive immutable structure leases; trust revocation masks queued publications.
    pub(crate) fn structure_providers(
        &self,
    ) -> BTreeMap<String, Result<Arc<plugin_runtime::StructureProvider>, String>> {
        if !self
            .worker
            .trusted
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return BTreeMap::new();
        }
        self.worker
            .state
            .lock()
            .unwrap()
            .structure_providers
            .clone()
    }
    /// Discover shipped ZIPs for the local market tab and inspect their manifests once.
    fn load_market_packages(&mut self) {
        let roots = crate::app::distribution::shipped_plugin_roots();
        let mut paths = Vec::new();
        for root in roots {
            if let Ok(files) = std::fs::read_dir(root) {
                paths.extend(
                    files.flatten().map(|file| file.path()).filter(|path| {
                        path.extension().is_some_and(|extension| extension == "zip")
                    }),
                );
            }
        }
        paths.sort();
        paths.dedup();
        self.manager_packages = paths
            .into_iter()
            .filter_map(|path| Package::read(&path).ok())
            .collect();
    }
    /// Select the plugin's cached SVG for the active editor palette.
    fn panel_icon(&self, dark: bool) -> Option<Icon> {
        self.panel_icons[usize::from(dark)]
            .as_deref()
            .map(|bytes| Icon::default().data(bytes).small())
    }
    /// Start a worker with explicit workspace/theme context and a private data root.
    pub fn new(
        parent: WeakEntity<EditorApp>,
        workspace: PathBuf,
        visible: Rc<Cell<bool>>,
        trusted: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        #[cfg(not(test))]
        let root = std::env::var_os("ME_EDITOR_PLUGIN_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs::config_dir()
                    .unwrap_or_else(std::env::temp_dir)
                    .join("MeEditor/runtime-plugins")
            });
        let environment = environment(&workspace, cx);
        #[cfg(test)]
        let root = workspace.join(".runtime-plugin-test");
        // Installed declarations are available before restored editor tabs are opened.
        crate::language::providers::configure(&root, &workspace);
        if trusted {
            if let Err(error) = contributions::refresh_for_workspace(&root, &workspace) {
                tracing::warn!(%error, "installed plugin contributions unavailable");
            }
        } else {
            contributions::refresh_entries(&root, &[]);
        }
        let worker = Arc::new(Worker::start(root.clone(), environment, trusted));
        // The initial registry snapshot is visible before the background worker starts WASM.
        let (startup, entries) = {
            let state = worker.state.lock().unwrap();
            // A fast cached worker can finish before this UI view is constructed.
            (state.startup.clone(), state.entries.clone())
        };
        let shutdown = Arc::downgrade(&worker);
        let quit = cx.on_app_quit(move |_, _| {
            let (tx, rx) = futures::channel::oneshot::channel();
            if let Some(worker) = shutdown.upgrade() {
                worker.cancel_installation();
                let _ = worker.tx.send(Work::Shutdown(Some(tx)));
            }
            async move {
                let _ = rx.await;
            }
        });
        let task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                if this.update(cx, |panel, cx| panel.poll(cx)).is_err() {
                    break;
                }
            }
        });
        Self {
            parent,
            visible,
            workspace,
            root,
            worker,
            entries,
            startup,
            views: HashMap::new(),
            images: Default::default(),
            active: None,
            surface_id: None,
            editor_preview: false,
            editor_auxiliary: false,
            viewport_sync_enabled: false,
            source_viewport: Default::default(),
            preview_document: None,
            preview_version: None,
            preview_file: None,
            preview_pending: false,
            preview_superseded: 0,
            preview_counted: None,
            preview_waiting: false,
            preview_ready: false,
            pending_toolbar: None,
            preview_error: None,
            panel_title: "插件管理".into(),
            panel_icons: [None, None],
            legacy_import: None,
            panel_icon_digest: None,
            tool_icons: HashMap::new(),
            focus: cx.focus_handle(),
            footer_focus: cx.focus_handle(),
            bounds: Bounds::default(),
            native_ui: None,
            native_toolbar: None,
            manager_open: false,
            configuration_revision: 0,
            log_generation: 0,
            commands_open: false,
            command_popup: None,
            pending: None,
            pending_dialog_open: false,
            bundled: Default::default(),
            installation: None,
            installation_dialog_open: false,
            manager_search: None,
            manager_search_subscription: None,
            manager_market: false,
            manager_selected: None,
            manager_tabs_focus: cx.focus_handle(),
            manager_detail_focus: cx.focus_handle(),
            manager_detail_scroll: gpui_kit::ScrollHandle::new(),
            manager_detail_tab: management::DetailTab::Overview,
            manager_log_view: None,
            manager_packages: vec![],
            confirm: None,
            confirm_dialog_open: false,
            status: None,
            manager_error_confirmed: std::cell::Cell::new(false),
            progress: None,
            processes: HashMap::new(),
            _task: task,
            last_theme: None,
            last_size: (0., 0., 0., 0.),
            instance_epoch: 0,
            _quit: Some(quit),
            _focus_events: vec![],
        }
    }
    /// Each declared dock surface gets its own native entity while sharing the plugin runtime.
    fn viewer_parts(
        parent: WeakEntity<EditorApp>,
        workspace: PathBuf,
        root: PathBuf,
        worker: Arc<Worker>,
        entries: Vec<Installed>,
        views: HashMap<String, Arc<protocol::ui::Document>>,
        id: String,
        panel: protocol::Panel,
        initially_visible: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let icon_entry = entries.iter().find(|entry| entry.manifest.id == id);
        let panel_icons = icon_entry
            .map(|entry| {
                [
                    entry.panel_icon(&root, &panel.id, false),
                    entry.panel_icon(&root, &panel.id, true),
                ]
            })
            .unwrap_or_default();
        let panel_icon_digest = icon_entry.map(|entry| entry.digest.clone());
        let task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                if this.update(cx, |panel, cx| panel.poll(cx)).is_err() {
                    break;
                }
            }
        });
        Self {
            parent,
            visible: Rc::new(Cell::new(initially_visible)),
            workspace,
            root,
            worker: worker.clone(),
            entries,
            startup: BTreeMap::new(),
            views,
            images: Default::default(),
            active: Some(id),
            surface_id: Some(panel.id),
            editor_preview: panel.position == "editor",
            editor_auxiliary: panel.auxiliary,
            viewport_sync_enabled: false,
            source_viewport: Default::default(),
            preview_document: None,
            preview_version: None,
            preview_file: None,
            preview_pending: false,
            preview_superseded: 0,
            preview_counted: None,
            preview_waiting: false,
            preview_ready: false,
            pending_toolbar: None,
            preview_error: None,
            panel_title: panel.title,
            panel_icons,
            legacy_import: None,
            panel_icon_digest,
            tool_icons: HashMap::new(),
            focus: cx.focus_handle(),
            footer_focus: cx.focus_handle(),
            bounds: Bounds::default(),
            native_ui: None,
            native_toolbar: None,
            manager_open: false,
            configuration_revision: 0,
            log_generation: 0,
            commands_open: false,
            command_popup: None,
            pending: None,
            pending_dialog_open: false,
            bundled: Default::default(),
            installation: None,
            installation_dialog_open: false,
            manager_search: None,
            manager_search_subscription: None,
            manager_market: false,
            manager_selected: None,
            manager_tabs_focus: cx.focus_handle(),
            manager_detail_focus: cx.focus_handle(),
            manager_detail_scroll: gpui_kit::ScrollHandle::new(),
            manager_detail_tab: management::DetailTab::Overview,
            manager_log_view: None,
            manager_packages: vec![],
            confirm: None,
            confirm_dialog_open: false,
            status: None,
            manager_error_confirmed: std::cell::Cell::new(false),
            progress: None,
            processes: HashMap::new(),
            _task: task,
            last_theme: None,
            last_size: (0., 0., 0., 0.),
            instance_epoch: 0,
            _quit: None,
            _focus_events: vec![],
        }
    }
    /// Publish worker results and hand native editor/clipboard requests to the UI thread.
    /// Apply host-local authority before accepting further worker views or contributions.
    pub(crate) fn set_workspace_trusted(&mut self, trusted: bool, cx: &mut Context<Self>) {
        if !trusted {
            self.bundled.cancel_request();
            self.worker.cancel_installation();
            // Startup declarations can exist before the worker publishes any installed entries.
            contributions::refresh_entries(&self.root, &[]);
        }
        self.worker
            .trusted
            .store(trusted, std::sync::atomic::Ordering::Release);
        let _ = self.worker.tx.send(Work::SetTrust(trusted));
        self.poll(cx);
    }

    fn poll(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        let mut contributions_changed = false;
        let editor_requests = {
            let mut state = self.worker.state.lock().unwrap();
            if self.surface_id.is_none() {
                changed |= self.bundled.ready != state.ready;
                self.bundled.ready = state.ready;
                if let Some(reply) = state.bundle_reply.take() {
                    if let Some(message) = self.bundled.accept(reply) {
                        // Integrity/recovery errors use the existing manager status; stale replies cannot set it.
                        state.status = Some(worker::OperationStatus {
                            plugin: None,
                            message,
                        });
                    }
                    changed = true;
                }
            }
            let log_generation = state.logs.generation();
            if self.log_generation != log_generation {
                self.log_generation = log_generation;
                changed = true;
            }
            if self.configuration_revision != state.configuration_revision {
                self.configuration_revision = state.configuration_revision;
                changed = true;
                contributions_changed = true;
            }
            if !self
                .worker
                .trusted
                .load(std::sync::atomic::Ordering::Acquire)
            {
                // An in-flight publication cannot resurrect resources after the user revokes trust.
                for entry in &mut state.entries {
                    entry.global_enabled.get_or_insert(entry.enabled);
                    entry.enabled = false;
                }
                state.startup.clear();
                state.views.clear();
                state.images.clear();
                state.processes.clear();
                state.document_events = Default::default();
                for (_, request) in state.editor_requests.drain(..) {
                    request.finish(Err(protocol::api::Failure::new(
                        protocol::api::ErrorCode::PermissionDenied,
                        "Workspace restricted",
                    )));
                }
            }
            // Keep the clicked confirmation visible until its operation finishes successfully.
            if self.surface_id.is_none() && self.progress.is_some() && state.progress.is_none() {
                if state.status.is_none() {
                    match self.progress.as_ref().map(|progress| progress.action) {
                        Some(LifecycleAction::Install) => self.pending = None,
                        Some(LifecycleAction::Uninstall) => {
                            self.confirm = None;
                            self.confirm_dialog_open = false;
                        }
                        _ => {}
                    }
                }
            }
            changed |= self.views.len() != state.views.len()
                || self.images.changed(&state.images)
                || state.views.iter().any(|(id, scene)| {
                    self.views
                        .get(id)
                        .is_none_or(|old| !Arc::ptr_eq(old, scene))
                })
                || self.entries.len() != state.entries.len()
                || self.entries.iter().zip(&state.entries).any(|(a, b)| {
                    a.enabled != b.enabled
                        || a.global_enabled != b.global_enabled
                        || a.project_enabled != b.project_enabled
                        || a.digest != b.digest
                        || a.error != b.error
                })
                || self.startup != state.startup
                || self.progress != state.progress
                || self.installation != state.installation;
            if self.surface_id.is_none() {
                contributions_changed |= self.entries.len() != state.entries.len()
                    || self.entries.iter().zip(&state.entries).any(|(old, new)| {
                        old.manifest.id != new.manifest.id
                            || old.enabled != new.enabled
                            || old.digest != new.digest
                            || old.error != new.error
                    });
            }
            self.entries = state.entries.clone();
            if let Some(id) = &self.active {
                let epoch = state.instance_epochs.get(id).copied().unwrap_or(0);
                if self.instance_epoch != epoch {
                    // The replacement guest starts at its default size; force one native measure.
                    self.instance_epoch = epoch;
                    self.last_size = (0., 0., 0., 0.);
                    self.preview_document = None;
                    self.preview_version = None;
                    self.preview_file = None;
                    self.preview_waiting = false;
                    self.preview_ready = false;
                    self.pending_toolbar = None;
                    self.preview_pending = false;
                    self.preview_superseded = 0;
                    self.preview_error = None;
                    // A replacement can reuse source/UI revisions; instance retirement still revokes its locate.
                    self.source_viewport.withdraw();
                    self.viewport_sync_enabled = false;
                    self.native_ui = None;
                    self.native_toolbar = None;
                    self._focus_events.clear();
                    self.command_popup = None;
                    self.commands_open = false;
                    changed = true;
                }
            }
            if let (Some(id), Some(panel_id)) = (&self.active, &self.surface_id) {
                let entry = self.entries.iter().find(|entry| &entry.manifest.id == id);
                let digest = entry.map(|entry| entry.digest.clone());
                if self.panel_icon_digest != digest {
                    self.tool_icons.clear();
                    // A hot update replaces the cached artwork with the new package version.
                    self.panel_icons = entry
                        .map(|entry| {
                            [
                                entry.panel_icon(&self.root, panel_id, false),
                                entry.panel_icon(&self.root, panel_id, true),
                            ]
                        })
                        .unwrap_or_default();
                    self.panel_icon_digest = digest;
                    changed = true;
                }
            }
            self.startup = state.startup.clone();
            self.views = state
                .views
                .iter()
                .map(|(id, s)| (id.clone(), s.clone()))
                .collect();
            self.images = state.images.clone();
            self.processes = state
                .processes
                .iter()
                .map(|(id, n)| (id.clone(), *n))
                .collect();
            self.progress = state.progress.clone();
            self.installation = state.installation.clone();
            if self.surface_id.is_none() {
                if let Some(p) = state.pending.take() {
                    changed = true;
                    self.pending = Some(p);
                    self.pending_dialog_open = false;
                    self.manager_open = true;
                    self.visible.set(true);
                }
            }
            if self.surface_id.is_none() {
                if let Some(status) = state.status.take() {
                    changed = true;
                    // Equal messages from a new publication still represent a fresh viewing round.
                    self.manager_error_confirmed.set(false);
                    self.status = Some(status);
                }
            }
            if self.surface_id.is_none() {
                std::mem::take(&mut state.editor_requests)
            } else {
                vec![]
            }
        };
        if !editor_requests.is_empty() {
            let parent = self.parent.clone();
            cx.defer(move |cx| {
                let _ = parent.update(cx, |app, cx| {
                    for request in editor_requests {
                        if app.pending_editor_requests.len() < 256 {
                            app.pending_editor_requests.push(request);
                        } else {
                            request.1.finish(Err(protocol::api::Failure::new(
                                protocol::api::ErrorCode::LimitExceeded,
                                "Editor queue is full",
                            )));
                        }
                    }
                    cx.notify();
                });
            });
        }
        if contributions_changed {
            contributions::refresh_entries(&self.root, &self.entries);
            let parent = self.parent.clone();
            cx.defer(move |cx| {
                let _ = parent.update(cx, |app, cx| {
                    app.pending_contribution_sync = true;
                    cx.notify();
                });
            });
        }
        self.refresh_tool_icons();
        if self.surface_id.is_none() {
            // The shortcut registry compares a stable same-lock lifecycle snapshot, including
            // instance epochs. Polling itself never changes bindings or redraws an unchanged UI.
            // Defer until this ExtensionPanel borrow ends before reading the shared publication.
            let parent = self.parent.clone();
            cx.defer(move |cx| {
                let _ = parent.update(cx, |app, cx| {
                    app.sync_shortcut_plugins(cx);
                    // Explicit resource release may leave package contributions unchanged. Wake the
                    // native shell so revoked readonly tabs cannot outlive their retained authority.
                    if app.tabs.iter().any(|tab| {
                        tab.virtual_document
                            .as_ref()
                            .is_some_and(|virtual_tab| !virtual_tab.resource.is_live())
                    }) {
                        cx.notify();
                    }
                });
            });
        }
        if changed {
            cx.notify();
            if self.surface_id.is_none() || self.editor_preview {
                let parent = self.parent.clone();
                let editor_preview = self.editor_preview;
                cx.defer(move |cx| {
                    let _ = parent.update(cx, |app, cx| {
                        if editor_preview {
                            // Source-local controls render in the dock's cached editor body, outside
                            // this preview entity. A new publication must invalidate that body too.
                            app.editor_panel.update(cx, |_, cx| cx.notify());
                        }
                        // Close the transient list when the final runtime plugin finishes.
                        if app.plugin_popup.is_some_and(|(kind, _)| {
                            kind == PluginPopupKind::Loading && app.plugin_count(kind, cx) == 0
                        }) {
                            app.plugin_popup = None;
                        }
                        cx.notify();
                    });
                });
            }
        }
    }
    fn send_to(&self, id: &str, panel: Option<String>, event: PluginEvent) {
        // Surface state belongs to its last observed publication, never the worker's newer incarnation.
        let epoch = if self.active.as_deref() == Some(id) {
            self.instance_epoch
        } else {
            self.worker
                .state
                .lock()
                .unwrap()
                .instance_epochs
                .get(id)
                .copied()
                .unwrap_or(0)
        };
        let _ = self
            .worker
            .tx
            .send(Work::Event(id.into(), epoch, panel, event));
    }

    /// The manager, language host and bottom popover use one in-memory history and viewing state.
    pub(crate) fn runtime_logs(&self) -> plugin_runtime::logs::RuntimeLogs {
        self.worker.state.lock().unwrap().logs.clone()
    }
    /// Queue a plugin lifecycle operation and expose its waiting state on this frame.
    fn queue_lifecycle(&mut self, work: Work) -> bool {
        let installing = matches!(&work, Work::Install(_) | Work::InstallBundle(_));
        if self.worker.queue_lifecycle(work) {
            if installing {
                self.installation_dialog_open = false;
            }
            self.status = None;
            self.progress = self.worker.state.lock().unwrap().progress.clone();
            true
        } else {
            false
        }
    }
    fn send(&self, event: PluginEvent) {
        if let Some(id) = &self.active {
            self.send_to(id, self.surface_id.clone(), event);
        }
    }
    fn command(&self, id: String) {
        self.send(PluginEvent::Command {
            id,
            arguments: None,
            context: None,
        });
    }
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.focus.focus(window, cx);
    }
    /// Reveal a plugin surface and notify its owner even when the old focus handle is retained.
    /// Plugins decide whether opening an empty view requires new state or process resources.
    pub(crate) fn show(&self, window: &mut Window, cx: &mut App) {
        self.visible.set(true);
        self.command("panel.opened".into());
        self.focus(window, cx);
    }
    /// Preview publication state for native tests: the revision sent and the one the guest echoed.
    #[cfg(test)]
    pub(crate) fn sent_and_published_revision(&self) -> (Option<u64>, Option<u64>) {
        (
            self.preview_version
                .as_ref()
                .map(|version| version.revision),
            self.published_source().map(|source| source.revision),
        )
    }

    fn current_document(&self) -> Option<Arc<protocol::ui::Document>> {
        self.active
            .as_ref()
            .and_then(|id| {
                self.views.get(&format!(
                    "{id}/{}",
                    self.surface_id.as_deref().unwrap_or_default()
                ))
            })
            .filter(|document| {
                !self.editor_preview
                    || (document.source == self.preview_version
                        && document.file == self.preview_file)
            })
            .cloned()
    }
    /// Retain readonly geometry during same-session parsing; exact `current_document` alone
    /// authorizes guest actions. A file switch or reopened session cannot reuse this tree.
    fn renderable_document(&self) -> Option<Arc<protocol::ui::Document>> {
        self.current_document().or_else(|| {
            let key = format!("{}/{}", self.active.as_ref()?, self.surface_id.as_ref()?);
            let scene = self.views.get(&key)?;
            let previous = scene.source.as_ref()?;
            let current = self.preview_version.as_ref()?;
            let same_file = match (&scene.file, &self.preview_file) {
                (None, None) => true,
                (Some(previous), Some(current)) => {
                    previous.id == current.id
                        && previous.path == current.path
                        && previous.revision <= current.revision
                }
                _ => false,
            };
            (self.editor_preview
                && same_file
                && previous.id == current.id
                && previous.path == current.path
                && previous.revision <= current.revision)
                .then(|| scene.clone())
        })
    }
    /// True when the guest has already published a tree for the text this panel sent last.
    /// While it has not, one publication stays in flight and the newest text waits for it.
    fn publication_settled(&self) -> bool {
        let Some(sent) = &self.preview_version else {
            return true;
        };
        self.published_source().is_some_and(|published| {
            published.id == sent.id
                && published.path == sent.path
                && published.revision >= sent.revision
        })
    }

    /// The source version echoed by this panel's guest publication, when the guest has one.
    fn published_source(&self) -> Option<&protocol::api::DocumentVersion> {
        self.active
            .as_ref()
            .and_then(|id| {
                self.views.get(&format!(
                    "{id}/{}",
                    self.surface_id.as_deref().unwrap_or_default()
                ))
            })
            .and_then(|document| document.source.as_ref())
    }
    /// Use the native file picker; unsigned packages show their requested capabilities before install.
    fn choose_package(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("选择插件包 (.zip)".into()),
        });
        let worker = self.worker.clone();
        cx.spawn(async move |_, _| {
            if let Ok(Ok(Some(paths))) = receiver.await {
                if let Some(path) = paths.first() {
                    // Native file picks enter the same progress state as market installs.
                    worker.queue_lifecycle(Work::Inspect(path.clone()));
                }
            }
        })
        .detach();
    }
}
/// Convert current host palette into explicit plugin context.
pub(crate) fn environment(workspace: &Path, cx: &App) -> protocol::Environment {
    fn color(c: gpui_kit::Hsla) -> u32 {
        let c = gpui_kit::Rgba::from(c);
        // Round float channels back to their exact theme bytes before sending them to plugins.
        let byte = |channel: f32| (channel * 255.).round().clamp(0., 255.) as u32;
        byte(c.r) << 16 | byte(c.g) << 8 | byte(c.b)
    }
    let typography = theme::typography(cx);
    // Legacy theme files are normalized on import; active guests see only current namespaces.
    protocol::Environment {
        workspace: workspace.display().to_string(),
        os: std::env::consts::OS.into(),
        locale: rust_i18n::locale().to_string(),
        background: color(cx.theme().background),
        foreground: color(cx.theme().foreground),
        muted: color(cx.theme().tab_bar),
        muted_foreground: color(cx.theme().muted_foreground),
        border: color(cx.theme().border),
        accent: color(cx.theme().primary),
        selection: color(cx.theme().list_active),
        dark: cx.theme().is_dark(),
        theme_colors: theme::plugin_colors(cx),
        ui_font: protocol::FontStyle {
            family: Some(cx.theme().font_family.to_string()),
            size_px: Some(cx.theme().font_size / gpui_kit::px(1.)),
            bold: typography.ui.bold,
        },
        mono_font: protocol::FontStyle {
            family: Some(cx.theme().mono_font_family.to_string()),
            size_px: Some(cx.theme().mono_font_size / gpui_kit::px(1.)),
            bold: typography.mono.bold,
        },
        theme_text_styles: theme::plugin_text_styles(cx)
            .into_iter()
            .map(|(key, style)| {
                (
                    key,
                    protocol::FontStyle {
                        family: style.family,
                        size_px: style.size_px,
                        bold: style.bold,
                    },
                )
            })
            .collect(),
    }
}

impl EventEmitter<PanelEvent> for ExtensionPanel {}
impl Focusable for ExtensionPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl dock::BasePanel for ExtensionPanel {
    fn panel_name(&self) -> &'static str {
        "Extensions"
    }
    /// A stable plugin/panel key distinguishes different contributions across process restarts.
    fn dump(&self, _: &App) -> gpui_base::dock::PanelState {
        let name = match (&self.active, &self.surface_id) {
            (Some(plugin), Some(panel)) => format!("plugin:{plugin}/{panel}"),
            _ => "Extensions".into(),
        };
        gpui_base::dock::PanelState::new(name)
    }
    fn closable(&self, _: &App) -> bool {
        false
    }
    fn zoomable(&self, _: &App) -> bool {
        false
    }
    fn visible(&self, _: &App) -> bool {
        self.visible.get()
    }
}
impl DockPanel for ExtensionPanel {
    /// Native window chrome remains generic; domain functions are drawn from the plugin's live tools.
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_base::ElementExt as _;
        let title = self.panel_title.clone();
        let icon = self.panel_icon(cx.theme().is_dark());
        let controls = h_flex().items_center();
        let command_owner = cx.entity().downgrade();
        h_flex()
            .w_full()
            .justify_between()
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .when_some(icon, |row, icon| row.child(icon))
                    .child(title),
            )
            .child(
                controls
                    .child(
                        div()
                            .id("plugin-command-menu-anchor")
                            .on_prepaint(move |bounds, _, cx| {
                                let _ = command_owner.update(cx, |this, cx| {
                                    this.bounds = bounds;
                                    if let Some(popup) = &this.command_popup {
                                        popup.update(cx, |popup, cx| popup.anchor_to(bounds, cx));
                                    }
                                });
                            })
                            .child(
                                Button::new("plugin-command-menu")
                                    .icon(Icon::default().path("icons/menu.svg"))
                                    .tooltip(t!("plugins.command_menu").to_string())
                                    .small()
                                    .compact()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.commands_open = !this.commands_open;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        Button::new("plugin-hide")
                            .icon(Icon::default().path("icons/window-minimize.svg"))
                            .tooltip(t!("plugins.hide_panel").to_string())
                            .small()
                            .compact()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                let panel_key = this
                                    .active
                                    .as_ref()
                                    .zip(this.surface_id.as_ref())
                                    .map(|(id, surface)| format!("{id}/{surface}"));
                                this.hide();
                                let _ = this.parent.update(cx, |app, cx| {
                                    // Hiding from the title bar is a saved visibility choice too.
                                    if let Some(key) = panel_key {
                                        app.session_state
                                            .plugin_panel_visibility
                                            .insert(key, false);
                                        app.persist_session();
                                    }
                                    app.dock_area.update(cx, |_, cx| cx.notify());
                                    cx.notify();
                                });
                            })),
                    ),
            )
    }
    fn title_bar(&self, _: &App) -> bool {
        true
    }
}
impl ExtensionPanel {
    /// Whether this workspace may start programs and language tools at all.
    ///
    /// The run controls read the same host-local authority the worker enforces, so a restricted
    /// workspace never offers a launch.
    pub(crate) fn workspace_trusted(&self) -> bool {
        self.worker
            .trusted
            .load(std::sync::atomic::Ordering::Acquire)
    }

    /// Read the actor's latest immutable provider snapshot, including revocation between UI frames.
    pub(crate) fn configuration_origins(&self) -> Vec<plugin_runtime::TargetOrigin> {
        self.worker
            .state
            .lock()
            .unwrap()
            .configuration_origins
            .clone()
    }

    /// Stage one host start for the plugin worker, reporting whether it could be queued.
    pub(crate) fn stage_host_run(&self, work: HostWork) -> bool {
        match self.worker.tx.send(work) {
            Ok(()) => true,
            Err(error) => {
                // A stopped actor terminates configuration waits rather than leaving Save spinning.
                let mut state = self.worker.state.lock().unwrap();
                let reason = t!("run.plugin_worker_unavailable").to_string();
                match error.0 {
                    Work::ConfigurationCall { request, .. } => {
                        state.configuration_replies.push(ConfigurationReply {
                            request,
                            origin: None,
                            result: Err(reason),
                        })
                    }
                    Work::ConfigurationCatalog { request, .. } => {
                        state.configuration_catalogs.push((
                            request,
                            ConfigurationCatalog {
                                templates: vec![],
                                failures: [("runtime".into(), reason)].into(),
                                origins: vec![],
                            },
                        ))
                    }
                    _ => return false,
                }
                state.configuration_revision += 1;
                false
            }
        }
    }

    /// Ask the runtime which run execution providers it has; the answer arrives with the next pump.
    pub(crate) fn ask_run_providers(&self) {
        let _ = self
            .worker
            .tx
            .send(crate::extensions::worker::Work::ListRunProviders);
    }

    /// The provider listing the runtime last published, if one has been asked for.
    pub(crate) fn run_providers(&self) -> Option<Vec<plugin_runtime::ProviderCandidate>> {
        self.worker.state.lock().unwrap().run_providers.clone()
    }

    /// Queue a debug call for the runtime, keyed by the request the editor will join the answer to.
    ///
    /// The method and arguments are the debug contract's, so nothing here decides what a call means,
    /// and a request that cannot be queued produces no answer at all rather than a pretended one.
    pub(crate) fn stage_debug_call(
        &self,
        request: u64,
        method: &str,
        arguments: serde_json::Value,
    ) -> bool {
        self.worker
            .tx
            .send(crate::extensions::worker::Work::DebugCall {
                request,
                configuration: None,
                method: method.to_owned(),
                arguments,
            })
            .is_ok()
    }

    /// Debug answers published since the last read, keyed by the request that asked.
    pub(crate) fn stage_debug_launch(
        &self,
        request: u64,
        configuration: &str,
        arguments: serde_json::Value,
    ) -> bool {
        self.worker
            .tx
            .send(worker::Work::DebugCall {
                request,
                configuration: Some(configuration.into()),
                method: "start".into(),
                arguments,
            })
            .is_ok()
    }

    /// Drain actual observations; the immutable host IDs associate them with their owning config.
    pub(crate) fn take_debug_observations(&self) -> Vec<plugin_runtime::DebugSession> {
        std::mem::take(&mut self.worker.state.lock().unwrap().debug_observations)
    }

    /// Debug answers published since the last read, keyed by the request that asked.
    pub(crate) fn take_debug_answers(
        &self,
    ) -> Vec<(u64, crate::extensions::worker::DebugAnswerMessage)> {
        std::mem::take(&mut self.worker.state.lock().unwrap().debug_answers)
    }

    /// Whether a debug session could start, as the runtime last reported it.
    ///
    /// `None` means the question has not been asked yet, which an entry point treats as "not
    /// available" rather than "available".
    pub(crate) fn debug_availability(&self) -> Option<Result<String, String>> {
        self.worker.state.lock().unwrap().debug_availability.clone()
    }

    /// What the provider that would serve a debug session says it can do, in the editor's own words.
    ///
    /// The host's ability names are translated here rather than at each control, so the panel and the
    /// calls it stages read one value. A missing answer stays `None`, which leaves every ability
    /// disabled with a reason: an ability the editor has not been told about is not one it may offer.
    pub(crate) fn debug_capabilities(&self) -> Option<editor_core::DebugCapabilities> {
        let abilities = self.worker.state.lock().unwrap().debug_abilities.clone()?;
        Some(editor_core::DebugCapabilities {
            breakpoints: abilities.breakpoints,
            resume_pause: abilities.resume_pause,
            step: abilities.step,
            // The host states inspection as one ability, because a frame list without variables is not
            // an inspection view; the editor mirrors that rather than inventing a split.
            inspect: abilities.inspect,
        })
    }

    /// Translate the actual registry once; each session then reads the capabilities of its owner.
    pub(crate) fn all_debug_capabilities(
        &self,
    ) -> BTreeMap<String, editor_core::DebugCapabilities> {
        self.worker
            .state
            .lock()
            .unwrap()
            .debug_provider_abilities
            .iter()
            .map(|(id, abilities)| {
                (
                    id.clone(),
                    editor_core::DebugCapabilities {
                        breakpoints: abilities.breakpoints,
                        resume_pause: abilities.resume_pause,
                        step: abilities.step,
                        inspect: abilities.inspect,
                    },
                )
            })
            .collect()
    }

    /// Drain each target receipt once; UI applies only the request identity belonging to its plan.
    /// Consume actual per-request output independently from provider terminal receipts.
    pub(crate) fn take_target_snapshots(
        &self,
    ) -> BTreeMap<u64, (String, usize, plugin_runtime::PreparationSnapshot)> {
        std::mem::take(&mut self.worker.state.lock().unwrap().target_snapshots)
    }
    pub(crate) fn take_target_preparations(
        &self,
    ) -> Vec<(String, usize, u64, Result<String, String>)> {
        std::mem::take(&mut self.worker.state.lock().unwrap().target_preparations)
    }
    /// Discovery is asynchronous and never implicitly creates a saved configuration.
    pub(crate) fn take_target_discoveries(
        &self,
    ) -> Vec<(String, u64, Result<worker::targets::TargetCatalog, String>)> {
        std::mem::take(&mut self.worker.state.lock().unwrap().target_discoveries)
    }

    /// Drain immutable configuration receipts; each consumer retains its own original request identity.
    pub(crate) fn take_configuration_replies(
        &self,
    ) -> (Vec<(u64, ConfigurationCatalog)>, Vec<ConfigurationReply>) {
        let mut published = self.worker.state.lock().unwrap();
        (
            std::mem::take(&mut published.configuration_catalogs),
            std::mem::take(&mut published.configuration_replies),
        )
    }

    /// Published host sessions, start refusals and stop answers reported by the worker.
    ///
    /// Reading drains the answer lists, so each outcome is explained exactly once.
    pub(crate) fn take_host_runs(
        &self,
    ) -> (
        Vec<HostRunSnapshot>,
        Vec<(String, u64, String)>,
        Vec<(String, u64, Result<(), String>)>,
        Vec<(String, u64, RunStatus)>,
    ) {
        let mut state = self.worker.state.lock().unwrap();
        (
            state.host_executions.clone(),
            std::mem::take(&mut state.run_errors),
            std::mem::take(&mut state.stop_results),
            std::mem::take(&mut state.run_status),
        )
    }
    /// Drain provider location replies once; the window checks whether that session is still selected.
    pub(crate) fn take_run_locations(&self) -> Vec<(u64, u64, Result<(), String>)> {
        std::mem::take(&mut self.worker.state.lock().unwrap().locate_results)
    }
}

impl EditorApp {
    /// Keep the window alive while snapshots finish; GPUI's final quit grace is only 200 ms.
    pub(crate) fn shutdown_plugins(&mut self, cx: &mut Context<Self>) {
        self.plugin_configuration_bridge.jobs.stop_all();
        if self.shutting_down {
            return;
        }
        self.shutting_down = true;
        self.capture_dock_layout(cx);
        // Read the current tree before saving, including the last click immediately before exit.
        self.capture_explorer_state(cx);
        self.persist_session();
        self.status = "正在保存插件状态…".into();
        self.extensions.read(cx).worker.cancel_installation();
        // File workers and their private backups may outlive the platform's 200 ms quit grace.
        // Share the native close gate's cleanup and prevent new file commands while it runs.
        let file_cleanup = self.shutdown_file_transfers(cx);
        cx.notify();
        let (tx, rx) = futures::channel::oneshot::channel();
        let _ = self
            .extensions
            .read(cx)
            .worker
            .tx
            .send(Work::Shutdown(Some(tx)));
        cx.spawn(async move |_, cx| {
            file_cleanup.await;
            let _ = rx.await;
            let _ = cx.update(|cx| cx.quit());
        })
        .detach();
    }
    /// Show every panel one provider declared, because a session's output lives in its own surface.
    ///
    /// The host holds no panel naming convention beyond the provider's own declaration: it reveals
    /// what that package contributed and reports the first failure rather than guessing a name.
    pub(crate) fn show_provider_panel(
        &mut self,
        plugin: &str,
        window: &mut Window,
        cx: &mut Context<EditorApp>,
    ) -> Result<(), String> {
        let prefix = format!("{plugin}/");
        let panels = self
            .plugin_panels
            .iter()
            .filter(|(key, _)| key.starts_with(&prefix))
            .map(|(key, panel)| (key.clone(), panel.clone()))
            .collect::<Vec<_>>();
        if panels.is_empty() {
            return Err("提供者没有可显示的界面".into());
        }
        for (key, panel) in panels {
            panel.update(cx, |panel, cx| panel.show(window, cx));
            self.session_state.plugin_panel_visibility.insert(key, true);
        }
        self.persist_session();
        self.dock_area.update(cx, |_, cx| cx.notify());
        Ok(())
    }

    /// Register and remove native panels directly from installed manifest contributions.
    pub(crate) fn sync_plugin_panels(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_preference_imports(cx);
        // Explicit manager publication and normal rendering share the same live shortcut path.
        self.sync_shortcut_plugins(cx);
        let contributions: Vec<_> = self
            .extensions
            .read(cx)
            .entries
            .iter()
            .filter(|entry| entry.enabled)
            .flat_map(|entry| {
                entry.manifest.panels.iter().map(|panel| {
                    (
                        format!("{}/{}", entry.manifest.id, panel.id),
                        entry.manifest.id.clone(),
                        panel.clone(),
                    )
                })
            })
            .collect();
        let obsolete: Vec<_> = self
            .plugin_panels
            .keys()
            .filter(|key| !contributions.iter().any(|(id, _, _)| id == *key))
            .cloned()
            .collect();
        for key in obsolete {
            if let Some(panel) = self.plugin_panels.remove(&key) {
                self.dock_area
                    .update(cx, |area, cx| area.remove_panel(panel, window, cx));
            }
        }
        for (key, id, descriptor) in contributions {
            let owner = self.extensions.clone();
            // A saved choice wins; first installation follows the manifest default.
            let initially_visible = self
                .session_state
                .plugin_panel_visibility
                .get(&key)
                .copied()
                .unwrap_or(descriptor.default_visible);
            let dock_size = self
                .session_state
                .plugin_dock_sizes
                .get(&descriptor.position)
                .copied()
                .unwrap_or(280.)
                .clamp(140., 1200.);
            let placement = match descriptor.position.as_str() {
                "left" => gpui_base::dock::DockPlacement::Left,
                "right" => gpui_base::dock::DockPlacement::Right,
                _ => gpui_base::dock::DockPlacement::Bottom,
            };
            if let Some(panel) = self.plugin_panels.get(&key) {
                // Editor-local contributions are rendered inside the current document's panel.
                if descriptor.position == "editor" {
                    panel.update(cx, |panel, _| panel.editor_auxiliary = descriptor.auxiliary);
                    continue;
                }
                // Reattach a hidden panel when reopened after its empty region was removed.
                let panel_id = gpui_base::dock::PanelId::from(panel.entity_id());
                if panel.read(cx).visible.get() && self.dock_area.read(cx).panel(panel_id).is_none()
                {
                    self.dock_area.update(cx, |area, cx| {
                        crate::local_dock::add_panel_view(
                            area,
                            dock::panel_handle(panel.clone()),
                            placement,
                            Some(px(dock_size)),
                            window,
                            cx,
                        );
                    });
                }
                continue;
            }
            let panel = cx.new(|cx| {
                let owner = owner.read(cx);
                let parent = owner.parent.clone();
                let workspace = owner.workspace.clone();
                let root = owner.root.clone();
                let worker = owner.worker.clone();
                let entries = owner.entries.clone();
                let views = owner.views.clone();
                ExtensionPanel::viewer_parts(
                    parent,
                    workspace,
                    root,
                    worker,
                    entries,
                    views,
                    id,
                    descriptor,
                    initially_visible,
                    cx,
                )
            });
            if initially_visible && !panel.read(cx).editor_preview {
                self.dock_area.update(cx, |area, cx| {
                    crate::local_dock::add_panel_view(
                        area,
                        dock::panel_handle(panel.clone()),
                        placement,
                        Some(px(dock_size)),
                        window,
                        cx,
                    )
                });
            }
            self.plugin_panels.insert(key, panel);
        }
        // Startup factories now have every available plugin view to bind to the saved layout.
        self.sync_editor_previews(cx);
        self.restore_dock_layout(window, cx);
        // Upstream retains empty dock regions. Remove their layout boxes at the app boundary,
        // while plugin entities and saved sizes remain available for reopening.
        for placement in [
            gpui_base::dock::DockPlacement::Left,
            gpui_base::dock::DockPlacement::Right,
            gpui_base::dock::DockPlacement::Bottom,
        ] {
            if self.dock_area.read(cx).has_dock(placement)
                && self.dock_area.read(cx).is_empty(placement, cx)
            {
                let message_id = gpui_base::dock::PanelId::from(self.messages.entity_id());
                self.dock_area.update(cx, |area, cx| {
                    if area
                        .layout(placement)
                        .is_some_and(|tree| tree.panels().any(|panel| panel == message_id))
                    {
                        // The host message panel retains its saved split and peers while hidden.
                        // Closing the region releases its width without removing the panel identity.
                        area.set_dock_collapsible(placement, true, window, cx);
                        if area.is_dock_open(placement) {
                            area.toggle_dock(placement, window, cx);
                        }
                    } else {
                        area.remove_dock(placement, window, cx);
                    }
                });
            }
        }
    }
    /// Places the supplied plugin artwork beside settings in the window title bar.
    pub(crate) fn render_extensions_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div().debug_selector(|| "extensions-trigger".into()).child(
            Button::new("open-extensions")
                .icon(Icon::default().data(include_bytes!("../assets/plugin-status/plugins.svg")))
                .small()
                .compact()
                .ghost()
                .tooltip("插件管理 Ctrl+`")
                .on_click(cx.listener(|this, _, window, cx| this.toggle_extensions(window, cx))),
        )
    }

    /// Opens or activates the manager without recreating any running plugin surface.
    pub(crate) fn toggle_extensions(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(handle) = self.extensions_window
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }
        let manager = self.extensions.clone();
        // A newly opened manager starts at Overview; activating an existing window keeps its current page.
        manager.update(cx, |manager, cx| {
            manager.manager_open = true;
            manager.manager_detail_tab = management::DetailTab::Overview;
            manager.manager_log_view = None;
            manager.manager_detail_scroll.set_offset(Default::default());
            cx.notify();
        });
        let (_, handle) = app_dialog::open_dialog_sized(
            t!("plugins.manager_title").to_string(),
            1080.,
            680.,
            move |content, _, _| content.h_full().child(manager.clone()),
            cx,
        );
        let window_id = handle.window_id();
        let owner = cx.entity().downgrade();
        self._extensions_closed_subscription = Some(cx.on_window_closed(move |cx, closed_id| {
            if closed_id == window_id {
                let _ = owner.update(cx, |this, cx| {
                    this.extensions_window = None;
                    this.extensions.update(cx, |panel, cx| {
                        // Delayed acknowledgements cannot survive the native dialog's close boundary.
                        panel.manager_open = false;
                        panel.manager_log_view = None;
                        cx.notify();
                    });
                    cx.notify();
                });
            }
        }));
        self.extensions_window = Some(handle);
        cx.notify();
    }

    /// Open the installed plugin's complete log from a status summary, even with an active search filter.
    pub(crate) fn open_plugin_logs(
        &mut self,
        plugin: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Retained summaries can outlive uninstall; validate the owner before changing selection or focus.
        if !self
            .extensions
            .read(cx)
            .entries
            .iter()
            .any(|entry| entry.manifest.id == plugin)
        {
            return;
        }
        self.close_plugin_popup(window, cx);
        self.toggle_extensions(window, cx);
        self.extensions.update(cx, |panel, cx| {
            // Recreate the empty search input instead of emitting a delayed Change that could undo routing.
            panel.manager_search_subscription = None;
            panel.manager_search = None;
            panel.manager_market = false;
            panel.manager_selected = Some(plugin);
            panel.manager_detail_tab = management::DetailTab::RuntimeLog;
            panel.manager_log_view = None;
            panel.manager_detail_scroll.set_offset(Default::default());
            cx.notify();
        });
        if let Some(handle) = self.extensions_window {
            let manager = self.extensions.clone();
            let _ = handle.update(cx, |_, window, cx| {
                // Release the panel read before focus mutates the application context.
                let detail_focus = manager.read(cx).manager_detail_focus.clone();
                detail_focus.focus(window, cx);
            });
        }
        cx.notify();
    }
}

/// Native UI fixtures reuse only declarative labels/layouts; their published package contract is current.
#[cfg(test)]
pub(crate) fn test_manifest(source: &str) -> protocol::Manifest {
    let mut value: serde_json::Value = serde_json::from_str(source).unwrap();
    value["protocol"] = serde_json::json!(7);
    value["api"] = serde_json::json!({"base":"^1"});
    serde_json::from_value(value).unwrap()
}
