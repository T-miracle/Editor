//! Built-in terminal sessions: native UI and upstream VT state, independent of installed packages.

mod config;
pub(crate) mod configurations;
mod engine;
mod input;
mod interaction;
mod io;
mod persistence;
mod shell;
pub(crate) mod task_view;
mod tasks;
#[cfg(test)]
mod tests;
mod view;

use crate::*;
use config::{Profile, Settings};
use engine::{Engine, GridSize};
use gpui_kit::ClipboardItem;
use plugin_runtime::{
    native_processes::{NativeLaunch, NativeProcessEvent, NativeProcessGroup},
    plugin_protocol::{
        self as protocol,
        process::{Transport, Update},
        ui::{Action, CanvasEvent, SideTab, SideTabs},
    },
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Native/OS details retain their diagnostic content inside a localized operation message.
enum FailureKind {
    Restore,
    Save,
    Settings,
    Process,
}

/// A session identity survives repaint and rename but never identifies another native process.
struct Session {
    id: u64,
    name: String,
    profile: Profile,
    cwd: String,
    engine: Engine,
    launched: bool,
    exited: bool,
    /// Restored shells answer ConPTY's cursor handshake from the restored logical grid.
    restored: bool,
    /// Tasks retain view identity while each accepted round owns distinct execution resources.
    task: Option<tasks::Task>,
}

/// This window owns terminal policy; the process supervisor owns only controlled system resources.
pub(crate) struct TerminalPanel {
    parent: WeakEntity<EditorApp>,
    /// IO can be issued during the parent's update without re-borrowing that same EditorApp.
    io_host: Entity<extensions::ExtensionPanel>,
    workspace: PathBuf,
    storage: PathBuf,
    settings: Settings,
    sessions: Vec<Session>,
    active: Option<u64>,
    next_id: u64,
    visible: bool,
    trusted: bool,
    focus: FocusHandle,
    _focus_subscription: Subscription,
    group: Option<WeakEntity<gpui_base::dock::TabGroup>>,
    supervisor: NativeProcessGroup,
    canvas: Option<Entity<ui::plugin::CanvasView>>,
    sidebar: Option<Entity<ui::controls::side_tabs::SideTabBar>>,
    width: f32,
    height: f32,
    cell_width: f32,
    cell_height: f32,
    tab_width: f32,
    preview_width: Rc<Cell<Option<f32>>>,
    popup: Option<Entity<ui::controls::menu::PopupMenu>>,
    requested_menu: Option<bool>,
    error: Option<String>,
    dirty: bool,
    last_save: Instant,
    /// A new tab needs a fresh measurement even when the preceding tab had identical geometry.
    measure_pending: bool,
    focus_pending: bool,
    settings_stamp: Option<std::time::SystemTime>,
    /// A rejected snapshot remains untouched until an explicit recovery/import replaces it.
    persistence_blocked: bool,
    pending_close: Option<u64>,
    /// A delayed public Locate cannot resurrect a task whose view the user already closed.
    closed_tasks: std::collections::BTreeSet<String>,
}

impl TerminalPanel {
    fn report_error(&mut self, kind: FailureKind, details: impl std::fmt::Display) {
        let key = match kind {
            FailureKind::Restore => "terminal.error.restore",
            FailureKind::Save => "terminal.error.save",
            FailureKind::Settings => "terminal.error.settings",
            FailureKind::Process => "terminal.error.process",
        };
        self.error = Some(t!(key, details = details.to_string()).to_string());
    }
    /// Prepare settings and logical history without launching anything in a restricted workspace.
    pub(crate) fn new(
        parent: WeakEntity<EditorApp>,
        io_host: Entity<extensions::ExtensionPanel>,
        workspace: PathBuf,
        trusted: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let storage = persistence::directory(&workspace);
        let focus = cx.focus_handle();
        // A dock can retain the pre-render handle; entering it must still land on the current input canvas.
        let focus_subscription = cx.on_focus(&focus, window, |panel, window, cx| {
            if let Some(canvas) = &panel.canvas {
                canvas.read(cx).focus_handle().focus(window, cx);
            } else {
                panel.focus_pending = true;
                cx.notify();
            }
        });
        let mut panel = Self {
            parent,
            io_host,
            workspace,
            storage,
            settings: Settings::default(),
            sessions: vec![],
            active: None,
            next_id: 0,
            visible: false,
            trusted,
            focus,
            _focus_subscription: focus_subscription,
            group: None,
            supervisor: NativeProcessGroup::new(trusted),
            canvas: None,
            sidebar: None,
            width: 640.,
            height: 240.,
            cell_width: 8.4,
            cell_height: 21.,
            tab_width: 180.,
            preview_width: Rc::new(Cell::new(None)),
            popup: None,
            requested_menu: None,
            error: None,
            dirty: false,
            last_save: Instant::now(),
            measure_pending: true,
            focus_pending: false,
            settings_stamp: None,
            persistence_blocked: false,
            pending_close: None,
            closed_tasks: Default::default(),
        };
        if let Err(error) = panel.restore() {
            panel.report_error(FailureKind::Restore, format!("{error:#}"));
            panel.persistence_blocked = true;
        }
        panel.settings_stamp = std::fs::metadata(panel.storage.join("settings.json"))
            .and_then(|meta| meta.modified())
            .ok();
        // Poll regardless of visibility: hiding the dock must not block output or process cleanup.
        cx.spawn(async move |panel, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(30))
                    .await;
                if panel.update(cx, |panel, cx| panel.poll(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        panel
    }

    pub(crate) fn visible(&self) -> bool {
        self.visible
    }
    /// Commit logical history before joining native cleanup at the editor's close gate.
    pub(crate) fn shutdown(&mut self) -> anyhow::Result<std::sync::mpsc::Receiver<()>> {
        let saved = self.save();
        if let Err(error) = saved {
            self.report_error(FailureKind::Save, error);
        }
        self.supervisor.shutdown()
    }
    /// Showing an empty panel creates a shell; hiding it preserves all native sessions.
    pub(crate) fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.visible = visible;
        self.focus_pending = visible;
        if visible && self.sessions.is_empty() {
            self.new_shell(self.settings.default_profile, cx);
        }
        if let Some(group) = self.group.as_ref().and_then(WeakEntity::upgrade) {
            group.update(cx, |_, cx| cx.notify());
        }
        cx.emit(PanelEvent::LayoutChanged);
        cx.notify();
    }

    /// Trust revocation reaches the supervisor before any delayed launch can execute.
    pub(crate) fn set_trusted(&mut self, trusted: bool, cx: &mut Context<Self>) {
        if self.trusted == trusted {
            return;
        }
        self.trusted = trusted;
        if let Err(error) = self.supervisor.set_trusted(trusted) {
            self.report_error(FailureKind::Process, error);
        }
        if trusted && self.visible {
            self.launch_active();
        }
        cx.notify();
    }

    fn active_index(&self) -> Option<usize> {
        self.sessions
            .iter()
            .position(|session| Some(session.id) == self.active)
    }
    fn grid_size(&self) -> GridSize {
        GridSize {
            columns: ((self.width - 16.) / self.cell_width)
                .floor()
                .clamp(2., 1000.) as usize,
            rows: ((self.height - 16.) / self.cell_height)
                .floor()
                .clamp(1., 500.) as usize,
        }
    }

    /// New tabs use the tool name, without an automatically increasing suffix.
    fn new_shell(&mut self, profile: usize, cx: &mut Context<Self>) {
        if self.sessions.len() >= 32 {
            self.error = Some(t!("terminal.session_limit").to_string());
            cx.notify();
            return;
        }
        let Some(profile) = self.settings.profiles.get(profile).cloned() else {
            return;
        };
        let Some(next_id) = self.next_id.checked_add(1) else {
            self.error = Some(t!("terminal.session_limit").to_string());
            cx.notify();
            return;
        };
        self.next_id = next_id;
        let name = Path::new(&profile.program)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let size = self.grid_size();
        self.sessions.push(Session {
            id: self.next_id,
            name,
            profile,
            cwd: self.workspace.to_string_lossy().into_owned(),
            engine: Engine::new(size, self.settings.history),
            launched: false,
            exited: false,
            restored: false,
            task: None,
        });
        self.active = Some(self.next_id);
        self.dirty = true;
        self.measure_pending = true;
        self.focus_pending = true;
        // Canvas measurement starts this shell, avoiding an initial guessed PTY size and redraw.
        cx.notify();
    }

    fn launch_active(&mut self) {
        let Some(index) = self.active_index() else {
            return;
        };
        if !self.trusted
            || self.sessions[index].task.is_some()
            || self.sessions[index].launched
            || self.sessions[index].exited
        {
            return;
        }
        let size = self.grid_size();
        let session = &mut self.sessions[index];
        let prompt = shell::default_prompt(&session.profile, &session.cwd);
        session
            .engine
            .begin_process(cfg!(windows) && session.restored, prompt.as_deref());
        let launch = NativeLaunch {
            program: session.profile.program.clone(),
            args: shell::arguments(&session.profile),
            cwd: session.cwd.clone(),
            env: BTreeMap::new(),
            transport: Transport::Pty {
                columns: size.columns as u16,
                rows: size.rows as u16,
                inherit_cursor: cfg!(windows) && session.restored,
            },
        };
        match self.supervisor.launch(session.id, launch) {
            Ok(()) => {
                session.launched = true;
            }
            Err(error) => {
                self.report_error(FailureKind::Process, error);
            }
        }
    }

    fn write(&mut self, bytes: Vec<u8>, cx: &mut App) {
        if let Some(index) = self.active_index() {
            let session = &mut self.sessions[index];
            if session.exited || !session.launched {
                return;
            }
            session.engine.set_offset(0);
            let id = session.id;
            self.send_input(id, bytes, cx);
        }
    }

    fn remove_session(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(index) = self.sessions.iter().position(|session| session.id == id) else {
            return;
        };
        if self.sessions[index].task.is_none()
            && let Err(error) = self.supervisor.close(id)
        {
            self.report_error(FailureKind::Process, error);
            return;
        }
        if let Some(task) = &self.sessions[index].task {
            self.closed_tasks.insert(task.key.clone());
        }
        self.sessions.remove(index);
        if self.pending_close == Some(id) {
            self.pending_close = None;
        }
        if self.active == Some(id) {
            self.active = self
                .sessions
                .get(index.min(self.sessions.len().saturating_sub(1)))
                .map(|session| session.id);
            // Automatic fallback is an identity switch too, even when canvas geometry is unchanged.
            self.measure_pending = true;
            self.focus_pending = true;
        }
        self.dirty = true;
        if self.sessions.is_empty() {
            self.set_visible(false, cx);
        }
        cx.notify();
    }

    fn tabs(&self) -> SideTabs {
        SideTabs {
            id: "native-terminal-sessions".into(),
            position: self.settings.tab_position,
            width: self.tab_width,
            min_width: 112.,
            max_width: 480.,
            items: self
                .sessions
                .iter()
                .map(|session| SideTab {
                    id: session.id.to_string(),
                    label: session.name.clone(),
                    status: session.exited.then(|| t!("terminal.exited").to_string()),
                    disabled: false,
                    closable: true,
                })
                .collect(),
            selected: self.active.map(|id| id.to_string()),
            rename: None,
        }
    }

    fn tab_action(&mut self, action: Action, cx: &mut Context<Self>) {
        match action {
            Action::Select(id) => {
                self.active = id
                    .parse()
                    .ok()
                    .filter(|id| self.sessions.iter().any(|session| session.id == *id));
                self.dirty = true;
                self.measure_pending = true;
            }
            Action::Close(id) => {
                if let Ok(id) = id.parse() {
                    self.close(id, cx);
                }
            }
            Action::Rename { id, value } => {
                if let Some(session) = self
                    .sessions
                    .iter_mut()
                    .find(|session| session.id.to_string() == id)
                    && !value.trim().is_empty()
                {
                    session.name = value.trim().chars().take(128).collect();
                    self.dirty = true;
                }
            }
            Action::Move { from, to } => {
                if let (Some(from), Some(to)) = (
                    self.sessions
                        .iter()
                        .position(|tab| tab.id.to_string() == from),
                    self.sessions
                        .iter()
                        .position(|tab| tab.id.to_string() == to),
                ) {
                    let item = self.sessions.remove(from);
                    self.sessions.insert(to, item);
                    self.dirty = true;
                }
            }
            Action::Resize(width) => {
                self.tab_width = width.clamp(112., 480.);
                self.dirty = true;
            }
            _ => {}
        }
        cx.notify();
    }
}

impl Drop for TerminalPanel {
    fn drop(&mut self) {
        let _ = self.save();
    }
}
impl EventEmitter<PanelEvent> for TerminalPanel {}
impl Focusable for TerminalPanel {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.canvas
            .as_ref()
            .map(|canvas| canvas.read(cx).focus_handle())
            .unwrap_or_else(|| self.focus.clone())
    }
}
impl dock::BasePanel for TerminalPanel {
    fn panel_name(&self) -> &'static str {
        "NativeTerminal"
    }
    fn closable(&self, _: &App) -> bool {
        false
    }
    fn zoomable(&self, _: &App) -> bool {
        false
    }
    fn visible(&self, _: &App) -> bool {
        self.visible
    }
    fn on_added_to(
        &mut self,
        group: WeakEntity<gpui_base::dock::TabGroup>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        self.group = Some(group);
    }
    fn on_removed(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.group = None;
    }
}

/// One native icon retains the user's terminal artwork through light and dark theme projection.
pub(crate) fn icon(cx: &App) -> Icon {
    let bytes: &'static [u8] = if cx.theme().is_dark() {
        include_bytes!("icon_dark.svg")
    } else {
        include_bytes!("icon_light.svg")
    };
    Icon::default().data(bytes).small()
}

impl EditorApp {
    /// Restore the leaf after loading an older dock tree, keeping unrelated peers and their sizes.
    pub(crate) fn ensure_terminal_dock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let panel = dock::panel_handle(self.terminal.clone());
        let id = gpui_base::dock::PanelId::from(self.terminal.entity_id());
        let height = px(self
            .session_state
            .plugin_dock_sizes
            .get("bottom")
            .copied()
            .unwrap_or(260.));
        self.dock_area.update(cx, |area, cx| {
            if area.panel(id).is_none() {
                local_dock::add_panel_view(
                    area,
                    panel,
                    gpui_base::dock::DockPlacement::Bottom,
                    Some(height),
                    window,
                    cx,
                );
            }
        });
    }

    /// Dock-level hiding also affects the footer, without changing whether Shell resources are alive.
    pub(crate) fn terminal_visible(&self, cx: &App) -> bool {
        self.terminal.read(cx).visible()
            && self
                .dock_area
                .read(cx)
                .is_dock_open(gpui_base::dock::DockPlacement::Bottom)
    }

    /// Reveal the native dock leaf; opening it never depends on installed package contributions.
    pub(crate) fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let visible = !self.terminal_visible(cx);
        let trusted = self.extensions.read(cx).workspace_trusted();
        self.terminal.update(cx, |panel, cx| {
            panel.set_trusted(trusted, cx);
            panel.set_visible(visible, cx);
        });
        if visible {
            self.ensure_terminal_dock(window, cx);
            let id = gpui_base::dock::PanelId::from(self.terminal.entity_id());
            self.dock_area.update(cx, |area, cx| {
                if !area.is_dock_open(gpui_base::dock::DockPlacement::Bottom) {
                    area.toggle_dock(gpui_base::dock::DockPlacement::Bottom, window, cx);
                }
                area.select_panel(id, window, cx);
            });
        }
        self.dock_area.update(cx, |_, cx| cx.notify());
        cx.notify();
    }
}
