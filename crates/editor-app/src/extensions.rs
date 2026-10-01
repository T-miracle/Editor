//! Generic runtime-plugin dock and manager. Feature behavior arrives from installed packages.
mod commands;
pub(crate) mod contributions;
mod images;
mod native_controls;
#[cfg(test)]
mod native_ui_tests;
mod preview;
#[cfg(test)]
mod preview_tests;
mod surface;
#[cfg(test)]
mod tests;
mod worker;
use crate::ui::controls::Input;
use crate::*;
use gpui_base::input::InputState;
use gpui_kit::{AnyElement, ClipboardItem, KeyDownEvent, PathPromptOptions, SharedString};
use plugin_runtime::{
    Installed, Package,
    plugin_protocol::{self as protocol, Event as PluginEvent, Request, Scene},
};
use std::cell::RefCell;
use std::collections::BTreeMap;
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

/// Generic editable field lifecycle; values are returned to the owning plugin on commit.
struct Editing {
    id: String,
    input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}
pub struct ExtensionPanel {
    parent: WeakEntity<EditorApp>,
    visible: Rc<Cell<bool>>,
    workspace: PathBuf,
    root: PathBuf,
    worker: Arc<Worker>,
    pub(crate) entries: Vec<Installed>,
    pub(crate) startup: BTreeMap<String, String>,
    scenes: HashMap<String, Arc<Scene>>,
    /// Raster images are paired with their exact scene and prepared outside the UI thread.
    images: images::SceneImages,
    active: Option<String>,
    surface_id: Option<String>,
    /// Editor-local surfaces are selected by the active document instead of the outer dock tree.
    editor_preview: bool,
    /// The editor's revision token avoids copying unchanged documents on every shell repaint.
    preview_document: Option<(PathBuf, u64)>,
    panel_title: String,
    /// Cache package-owned artwork so repainting does not read from disk.
    panel_icons: [Option<Vec<u8>>; 2],
    panel_icon_digest: Option<String>,
    focus: FocusHandle,
    bounds: Bounds<Pixels>,
    composition: String,
    editing: Option<Editing>,
    /// Protocol 2 UI owns keyed native input state independently from the legacy canvas.
    native_ui: Option<Entity<crate::ui::plugin::PluginView>>,
    canvas_controls: Option<Entity<crate::ui::plugin::controls::CanvasControlsView>>,
    /// Prevent a just-committed native input from reopening before the guest reply arrives.
    committed_edit: Option<String>,
    scroll: surface::PluginScroll,
    manager_open: bool,
    commands_open: bool,
    command_popup: Option<Entity<crate::ui::controls::menu::PopupMenu>>,
    pending: Option<Package>,
    /// Prevent repainting from opening the same installation dialog repeatedly.
    pending_dialog_open: bool,
    /// Search and selection belong to the manager window, not a plugin surface.
    manager_search: Option<Entity<InputState>>,
    manager_search_subscription: Option<Subscription>,
    manager_market: bool,
    manager_selected: Option<String>,
    manager_packages: Vec<Package>,
    confirm: Option<(String, bool)>,
    /// Keep an uninstall confirmation to one overlay per click.
    confirm_dialog_open: bool,
    status: Option<String>,
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
    /// Discover shipped ZIPs for the local market tab and inspect their manifests once.
    fn load_market_packages(&mut self) {
        let exe = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_owned));
        let roots = exe
            .into_iter()
            .map(|path| path.join("plugins"))
            .chain(std::iter::once(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins"),
            ));
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
        if let Err(error) = contributions::refresh_for_workspace(&root, &workspace) {
            tracing::warn!(%error, "installed plugin contributions unavailable");
        }
        let worker = Arc::new(Worker::start(root.clone(), environment));
        // The initial registry snapshot is visible before the background worker starts WASM.
        let (startup, entries) = {
            let state = worker.state.lock().unwrap();
            // A fast cached worker can finish before this UI view is constructed.
            (state.startup.clone(), state.entries.clone())
        };
        let shutdown = worker.tx.clone();
        let quit = cx.on_app_quit(move |_, _| {
            let (tx, rx) = futures::channel::oneshot::channel();
            let _ = shutdown.send(Work::Shutdown(Some(tx)));
            async move {
                let _ = rx.await;
            }
        });
        let scroll = surface::PluginScroll::new(worker.tx.clone());
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
            scenes: HashMap::new(),
            images: BTreeMap::new(),
            active: None,
            surface_id: None,
            editor_preview: false,
            preview_document: None,
            panel_title: "插件管理".into(),
            panel_icons: [None, None],
            panel_icon_digest: None,
            focus: cx.focus_handle(),
            bounds: Bounds::default(),
            composition: String::new(),
            editing: None,
            native_ui: None,
            canvas_controls: None,
            committed_edit: None,
            scroll,
            manager_open: false,
            commands_open: false,
            command_popup: None,
            pending: None,
            pending_dialog_open: false,
            manager_search: None,
            manager_search_subscription: None,
            manager_market: false,
            manager_selected: None,
            manager_packages: vec![],
            confirm: None,
            confirm_dialog_open: false,
            status: None,
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
        scenes: HashMap<String, Arc<Scene>>,
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
            scenes,
            images: BTreeMap::new(),
            active: Some(id),
            surface_id: Some(panel.id),
            editor_preview: panel.position == "editor",
            preview_document: None,
            panel_title: panel.title,
            panel_icons,
            panel_icon_digest,
            focus: cx.focus_handle(),
            bounds: Bounds::default(),
            composition: String::new(),
            editing: None,
            native_ui: None,
            canvas_controls: None,
            committed_edit: None,
            scroll: surface::PluginScroll::new(worker.tx.clone()),
            manager_open: false,
            commands_open: false,
            command_popup: None,
            pending: None,
            pending_dialog_open: false,
            manager_search: None,
            manager_search_subscription: None,
            manager_market: false,
            manager_selected: None,
            manager_packages: vec![],
            confirm: None,
            confirm_dialog_open: false,
            status: None,
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
    fn poll(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        let mut contributions_changed = false;
        let effects = {
            let mut state = self.worker.state.lock().unwrap();
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
            changed |= self.scenes.len() != state.scenes.len()
                || state.scenes.iter().any(|(id, scene)| {
                    self.scenes
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
                || self.progress != state.progress;
            if self.surface_id.is_none() {
                contributions_changed = self.entries.len() != state.entries.len()
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
                    self.native_ui = None;
                    self.canvas_controls = None;
                    self.command_popup = None;
                    self.commands_open = false;
                    changed = true;
                }
            }
            if let (Some(id), Some(panel_id)) = (&self.active, &self.surface_id) {
                let entry = self.entries.iter().find(|entry| &entry.manifest.id == id);
                let digest = entry.map(|entry| entry.digest.clone());
                if self.panel_icon_digest != digest {
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
            self.scenes = state
                .scenes
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
                    self.status = Some(status);
                }
            }
            if self.surface_id.is_none() {
                std::mem::take(&mut state.effects)
            } else {
                vec![]
            }
        };
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
        for (id, effect) in effects {
            match effect {
                Request::ClipboardWrite(text) => {
                    cx.write_to_clipboard(ClipboardItem::new_string(text))
                }
                Request::ClipboardRead => {
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        self.send_to(&id, PluginEvent::Paste(text));
                    }
                }
                Request::Editor { command } => {
                    let parent = self.parent.clone();
                    let tx = self.worker.tx.clone();
                    let root = self.root.clone();
                    cx.defer(move |cx| {
                        let _ = parent.update(cx, |app, cx| {
                            // Scope panel operations to the plugin that emitted this host request.
                            if let Some(panel) = command.strip_prefix("hide_panel:") {
                                app.hide_plugin_panel(&id, panel, cx);
                                return;
                            }
                            let mut cwd = None;
                            let mut text = None;
                            match command.as_str() {
                                "selection" => {
                                    text = Some(app.editor.read(cx).selected_text().to_string())
                                }
                                "active_directory" => {
                                    cwd = Some(
                                        app.active_path
                                            .as_deref()
                                            .and_then(Path::parent)
                                            .unwrap_or(app.workspace.root())
                                            .display()
                                            .to_string(),
                                    )
                                }
                                "save" => app.save_current(cx),
                                _ => {}
                            }
                            let _ = tx.send(Work::Event(
                                id.clone(),
                                PluginEvent::Command {
                                    id: format!("{command}.result"),
                                    cwd,
                                    text,
                                    arguments: None,
                                },
                            ));
                            if let Some(relative) = command.strip_prefix("open_data:") {
                                if !relative.contains(['/', '\\', ':'])
                                    && !relative.starts_with('.')
                                {
                                    let path = root.join("data").join(&id).join(relative);
                                    app.status = format!("插件配置：{}", path.display());
                                    app.pending_plugin_file = Some(path);
                                    cx.notify();
                                }
                            }
                        });
                    });
                }
                _ => {}
            }
        }
        changed |= self.scroll.visibility_changed();
        if changed {
            cx.notify();
            if self.surface_id.is_none() {
                let parent = self.parent.clone();
                cx.defer(move |cx| {
                    let _ = parent.update(cx, |app, cx| {
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
    fn send_to(&self, id: &str, event: PluginEvent) {
        let _ = self.worker.tx.send(Work::Event(id.into(), event));
    }
    /// Queue a plugin lifecycle operation and expose its waiting state on this frame.
    fn queue_lifecycle(&mut self, work: Work) -> bool {
        if self.worker.queue_lifecycle(work) {
            self.status = None;
            self.progress = self.worker.state.lock().unwrap().progress.clone();
            true
        } else {
            false
        }
    }
    fn send(&self, event: PluginEvent) {
        if let Some(id) = &self.active {
            let event = if let Some(panel) = &self.surface_id {
                PluginEvent::Surface {
                    panel: panel.clone(),
                    event: Box::new(event),
                }
            } else {
                event
            };
            self.send_to(id, event);
        }
    }
    fn command(&self, id: String) {
        self.send(PluginEvent::Command {
            id,
            cwd: None,
            text: None,
            arguments: None,
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
    fn current_scene(&self) -> Option<Arc<Scene>> {
        self.active
            .as_ref()
            .and_then(|id| {
                self.scenes.get(&format!(
                    "{id}/{}",
                    self.surface_id.as_deref().unwrap_or_default()
                ))
            })
            .cloned()
    }
    /// Dispatch manifest shortcuts without registering terminal-specific native actions.
    pub fn shortcut(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        for entry in &self.entries {
            if !entry.enabled {
                continue;
            }
            for command in &entry.manifest.commands {
                if command
                    .shortcut
                    .as_ref()
                    .is_some_and(|s| key_matches(s, event))
                {
                    let id = entry.manifest.id.clone();
                    let command = command.id.clone();
                    let parent = self.parent.clone();
                    window.defer(cx, move |window, cx| {
                        let _ = parent.update(cx, |app, cx| {
                            // Manifest shortcuts use the same checked API as future project actions.
                            if let Err(error) = app.invoke_plugin_command(
                                &id,
                                &command,
                                serde_json::Value::Null,
                                window,
                                cx,
                            ) {
                                app.status = error;
                                cx.notify();
                            }
                        });
                    });
                    cx.stop_propagation();
                    return true;
                }
            }
        }
        false
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
    fn commit_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(editing) = self.editing.take() {
            self.committed_edit = Some(editing.id.clone());
            self.send(PluginEvent::Edit {
                id: editing.id,
                text: editing.input.read(cx).value().to_string(),
            });
            cx.notify();
        }
    }
    /// Plugin fields use the same native input control as the rest of the editor.
    fn sync_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            return;
        }
        let Some(widget) = self
            .current_scene()
            .and_then(|s| s.widgets.iter().find(|w| w.edit).cloned())
        else {
            self.committed_edit = None;
            return;
        };
        if self.committed_edit.as_ref() == Some(&widget.id) {
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).default_value(widget.label));
        let enter = cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.commit_edit(cx);
                this.focus(window, cx);
            }
        });
        let blur = cx.on_focus_out(&input.focus_handle(cx), window, |this, _, _, cx| {
            this.commit_edit(cx)
        });
        let focus = input.focus_handle(cx);
        let weak_input = input.downgrade();
        self.editing = Some(Editing {
            id: widget.id,
            input,
            _subscriptions: vec![enter, blur],
        });
        // Focus after the native input is mounted, then select its original tab name.
        window.defer(cx, move |window, cx| {
            let _ = weak_input.update(cx, |input, cx| {
                focus.focus(window, cx);
                input.select_all(window, cx);
            });
        });
    }
}
/// Convert current host palette into explicit plugin context.
fn environment(workspace: &Path, cx: &App) -> protocol::Environment {
    fn color(c: gpui_kit::Hsla) -> u32 {
        let c = gpui_kit::Rgba::from(c);
        // Round float channels back to their exact theme bytes before sending them to plugins.
        let byte = |channel: f32| (channel * 255.).round().clamp(0., 255.) as u32;
        byte(c.r) << 16 | byte(c.g) << 8 | byte(c.b)
    }
    let typography = theme::typography(cx);
    let mut environment = protocol::Environment {
        workspace: workspace.display().to_string(),
        os: std::env::consts::OS.into(),
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
    };
    // Installed older WASM guests keep their original theme namespace until the package is updated.
    for (key, value) in environment.theme_colors.clone() {
        environment
            .theme_colors
            .entry(format!("me.{key}"))
            .or_insert(value);
    }
    for (key, value) in environment.theme_text_styles.clone() {
        environment
            .theme_text_styles
            .entry(format!("me.{key}"))
            .or_insert(value);
    }
    environment
}
fn key_matches(shortcut: &str, event: &KeyDownEvent) -> bool {
    let parts: Vec<_> = shortcut.split('-').collect();
    let m = event.keystroke.modifiers;
    parts.last().is_some_and(|key| *key == event.keystroke.key)
        && parts.contains(&"ctrl") == m.control
        && parts.contains(&"alt") == m.alt
        && parts.contains(&"shift") == m.shift
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
    /// Toolbar labels and commands come from installed manifests.
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self.panel_title.clone();
        let icon = self.panel_icon(cx.theme().is_dark());
        let mut controls = h_flex().items_center();
        if let Some(entry) = self
            .entries
            .iter()
            .find(|p| Some(&p.manifest.id) == self.active.as_ref())
        {
            for command in &entry.manifest.commands {
                if command.toolbar.is_some() || command.toolbar_icon.is_some() {
                    let id = command.id.clone();
                    // Plugin toolbars may request a shared host icon or retain a text fallback.
                    let button =
                        Button::new(SharedString::from(id.clone())).tooltip(command.title.clone());
                    let button = if let Some(path) = &command.toolbar_icon {
                        button.icon(Icon::default().path(path.clone()))
                    } else {
                        button.label(command.toolbar.clone().unwrap_or_default())
                    };
                    controls =
                        controls.child(
                            button.small().compact().ghost().on_click(
                                cx.listener(move |this, _, _, _| this.command(id.clone())),
                            ),
                        );
                }
            }
        }
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
                        Button::new("plugin-command-menu")
                            .icon(Icon::default().path("icons/menu.svg"))
                            .tooltip("插件命令")
                            .small()
                            .compact()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.commands_open = !this.commands_open;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("plugin-hide")
                            .icon(Icon::default().path("icons/window-minimize.svg"))
                            .tooltip("隐藏面板")
                            .small()
                            .compact()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                let panel_key = this
                                    .active
                                    .as_ref()
                                    .zip(this.surface_id.as_ref())
                                    .map(|(id, surface)| format!("{id}/{surface}"));
                                this.visible.set(false);
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
impl EditorApp {
    /// Keep the window alive while snapshots finish; GPUI's final quit grace is only 200 ms.
    pub(crate) fn shutdown_plugins(&mut self, cx: &mut Context<Self>) {
        if self.shutting_down {
            return;
        }
        self.shutting_down = true;
        self.capture_dock_layout(cx);
        // Read the current tree before saving, including the last click immediately before exit.
        self.capture_explorer_state(cx);
        self.persist_session();
        self.status = "正在保存插件状态…".into();
        cx.notify();
        let (tx, rx) = futures::channel::oneshot::channel();
        let _ = self
            .extensions
            .read(cx)
            .worker
            .tx
            .send(Work::Shutdown(Some(tx)));
        cx.spawn(async move |_, cx| {
            let _ = rx.await;
            let _ = cx.update(|cx| cx.quit());
        })
        .detach();
    }
    /// Each installed dock contribution has its own visibility toggle in the editor status bar.
    pub(crate) fn plugin_panel_buttons(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        // Keep the status order declarative; plugin identities remain opaque to the host.
        let selected_style = component_styles(cx, ThemeComponent::PanelToggle).selected;
        let orders = self
            .extensions
            .read(cx)
            .entries
            .iter()
            .flat_map(|entry| {
                entry.manifest.panels.iter().map(|panel| {
                    (
                        format!("{}/{}", entry.manifest.id, panel.id),
                        panel.status_order.unwrap_or(1000),
                    )
                })
            })
            .collect::<HashMap<_, _>>();
        let mut panels = self.plugin_panels.iter().collect::<Vec<_>>();
        panels.sort_by_key(|(key, _)| (orders.get(*key).copied().unwrap_or(1000), (*key).clone()));
        panels
            .into_iter()
            .map(|(key, panel)| {
                let panel = panel.clone();
                let key = key.clone();
                let title = panel.read(cx).panel_title.clone();
                let icon = panel.read(cx).panel_icon(cx.theme().is_dark());
                let visible = panel.read(cx).visible.get();
                // A declared panel icon replaces its label while the tooltip keeps its name.
                let button =
                    Button::new(SharedString::from(format!("toggle-{key}"))).tooltip(title.clone());
                let button = if let Some(icon) = icon {
                    button.icon(icon)
                } else {
                    button.label(title)
                };
                button
                    .small()
                    .compact()
                    .ghost()
                    // Match the existing 24px compact icon width without changing button widths.
                    .h(px(24.))
                    .when(visible, |button| {
                        button
                            .bg(selected_style.background.unwrap_or(cx.theme().list_active))
                            .text_color(selected_style.foreground.unwrap_or(cx.theme().foreground))
                    })
                    .on_click(cx.listener(move |app, _, window, cx| {
                        panel.update(cx, |panel, cx| {
                            let visible = !panel.visible.get();
                            if visible {
                                panel.show(window, cx);
                            } else {
                                panel.visible.set(false);
                            }
                            cx.notify();
                        });
                        app.dock_area.update(cx, |_, cx| cx.notify());
                        app.session_state
                            .plugin_panel_visibility
                            .insert(key.clone(), panel.read(cx).visible.get());
                        app.persist_session();
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect()
    }
    /// Register and remove native panels directly from installed manifest contributions.
    pub(crate) fn sync_plugin_panels(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
                let scenes = owner.scenes.clone();
                ExtensionPanel::viewer_parts(
                    parent,
                    workspace,
                    root,
                    worker,
                    entries,
                    scenes,
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
                self.dock_area
                    .update(cx, |area, cx| area.remove_dock(placement, window, cx));
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
        let (_, handle) = app_dialog::open_dialog_sized(
            "插件管理",
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
                    cx.notify();
                });
            }
        }));
        self.extensions_window = Some(handle);
        cx.notify();
    }
}
