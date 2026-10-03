//! Terminal application: VT parsing, layout, profiles and interaction live in this guest.
mod commands;
mod config;
mod controls;
mod emulator;
mod events;
mod host;
mod input;
mod interaction;
mod scene;
mod shell;
#[cfg(test)]
mod tests;
mod theme;
use config::{Profile, Settings};
use events::Event;
use plugin_protocol::bindings::{Guest, export};
use plugin_protocol::*;
use plugin_protocol::{api, process};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, collections::BTreeMap};
struct TerminalPlugin;
thread_local! { static APP: RefCell<Option<Terminal>> = const { RefCell::new(None) }; }
// Keep the terminal usable while allowing a wider session list for long names.
const DEFAULT_TAB_WIDTH: f32 = 180.;
const MIN_TAB_WIDTH: f32 = 112.;
const MAX_TAB_WIDTH: f32 = 480.;

/// Older snapshots did not store the width of the terminal's tab list.
fn default_tab_width() -> f32 {
    DEFAULT_TAB_WIDTH
}
impl Guest for TerminalPlugin {
    /// A serial event loop keeps parsing and UI mutation in the same isolated instance.
    fn dispatch(payload: String) -> Result<String, String> {
        api::guest::dispatch(&payload, |message| {
            APP.with(|cell| {
                if let api::Input::Event {
                    event: api::Notification::MigrateData { snapshot, .. },
                    ..
                } = &message
                {
                    // Version one preserves the existing JSON configuration and logical snapshot; no shell starts here.
                    if let Some(source) =
                        host::read_optional("settings.json", true).map_err(failure)?
                    {
                        let os = cell
                            .borrow()
                            .as_ref()
                            .map(|app| app.env.os.clone())
                            .ok_or_else(|| failure("Migration requires prepared environment"))?;
                        Settings::parse_for_os(&source, &os)
                            .map_err(|error| failure(error.to_string()))?;
                    }
                    return Ok(api::Output {
                        snapshot: snapshot.clone(),
                        ..Default::default()
                    });
                }
                let mut app = cell.borrow_mut();
                if let api::Input::Prepare {
                    environment,
                    snapshot,
                    ..
                } = &message
                {
                    *app = Some(
                        Terminal::prepare(environment.clone(), snapshot.clone())
                            .map_err(failure)?,
                    );
                }
                let terminal = app
                    .as_mut()
                    .ok_or_else(|| failure("Plugin has not been prepared"))?;
                let previous = terminal.interaction_identity();
                let mut reply = match message {
                    api::Input::Prepare { .. } => api::Output::default(),
                    api::Input::Activate => {
                        terminal.activate();
                        terminal.reply()
                    }
                    api::Input::Event { event, .. } => {
                        terminal.notify(event);
                        terminal.reply()
                    }
                    api::Input::Snapshot => api::Output {
                        snapshot: Some(terminal.snapshot()),
                        ..Default::default()
                    },
                };
                // Paint/output changes cannot expire already queued keys from the same native frame.
                if previous != terminal.interaction_identity() {
                    terminal.ui_revision = terminal.ui_revision.wrapping_add(1);
                    for view in &mut reply.views {
                        view.document.revision = terminal.ui_revision;
                    }
                }
                Ok(reply)
            })
        })
    }
}
export!(TerminalPlugin);

/// Lifecycle failures remain typed; recoverable shell errors are displayed inside the terminal view.
fn failure(message: impl Into<String>) -> api::Failure {
    api::Failure::new(api::ErrorCode::OperationFailed, message)
}
#[derive(Clone, Copy)]
struct Extent {
    columns: usize,
    rows: usize,
}
struct Tab {
    id: u64,
    name: String,
    profile: Profile,
    cwd: String,
    handle: Option<api::ResourceHandle>,
    /// Upstream screen and plugin selection stay inside WASM.
    term: emulator::Emulator,
    exited: bool,
    /// A second lightweight VT observer captures only shell integration metadata.
    metadata_parser: vte::Parser,
    metadata: shell::Metadata,
}
/// Plugin-owned snapshot schema deliberately contains no native process handles.
#[derive(Serialize, Deserialize)]
struct Saved {
    tabs: Vec<SavedTab>,
    active: usize,
    next_id: u64,
    /// Keep an empty legacy field so an older package can still restore a rollback snapshot.
    #[serde(default)]
    counts: BTreeMap<String, usize>,
    settings: Settings,
    #[serde(default = "default_tab_width")]
    tab_width: f32,
    /// Version zero may contain prompt gaps generated by the former ConPTY startup handshake.
    #[serde(default)]
    recovery_version: u32,
}
#[derive(Serialize, Deserialize)]
struct SavedTab {
    /// Finished sessions are historical views; restoring them must not execute their program again.
    #[serde(default)]
    exited: bool,
    id: u64,
    name: String,
    profile: Profile,
    cwd: String,
    output: String,
    /// Older packages saved only ANSI text; new snapshots also retain the grid and caret.
    #[serde(default)]
    display: Option<emulator::DisplayState>,
}
/// Keep the command menu separate from the output area's smaller context menu.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TerminalMenu {
    Commands,
    Output,
}
impl TerminalMenu {
    /// Distinct menu identities prevent a delayed click from reaching another popup.
    fn id(self) -> &'static str {
        match self {
            Self::Commands => "terminal-menu",
            Self::Output => "terminal-output-menu",
        }
    }
}
struct Terminal {
    env: Environment,
    settings: Settings,
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    /// Preserve each host invocation while awaiting the editor's asynchronous save response.
    pending_editor: BTreeMap<u64, commands::PendingEditor>,
    width: f32,
    height: f32,
    tab_width: f32,
    cw: f32,
    ch: f32,
    menu: Option<TerminalMenu>,
    menu_position: (f32, f32),
    rename: Option<u64>,
    ui_revision: u64,
    selecting: bool,
    error: Option<String>,
}
impl Terminal {
    /// Validate and migrate saved data without acquiring any OS resources.
    fn prepare(env: Environment, snapshot: Option<Snapshot>) -> Result<Self, String> {
        let settings = Settings::for_os(&env.os);
        let mut app = Self {
            env,
            settings,
            tabs: vec![],
            active: 0,
            next_id: 1,
            pending_editor: BTreeMap::new(),
            width: 800.,
            height: 240.,
            tab_width: DEFAULT_TAB_WIDTH,
            cw: 8.4,
            ch: 21.,
            menu: None,
            menu_position: (0., 0.),
            rename: None,
            ui_revision: 0,
            selecting: false,
            error: None,
        };
        if let Some(snapshot) = snapshot {
            if !matches!(snapshot.schema, 1 | 2) {
                return Err(format!(
                    "Unsupported terminal snapshot schema {}",
                    snapshot.schema
                ));
            }
            let saved: Saved = serde_json::from_str(&snapshot.data).map_err(|e| e.to_string())?;
            app.settings = Settings::parse(&serde_json::to_string(&saved.settings).unwrap())
                .map_err(|e| e.to_string())?;
            if saved.tabs.len() > 32 {
                return Err("Snapshot has too many sessions".into());
            }
            app.next_id = saved.next_id;
            // Snapshot data is user-controlled; reject non-finite layout values.
            if saved.tab_width.is_finite() {
                app.tab_width = saved.tab_width.clamp(MIN_TAB_WIDTH, MAX_TAB_WIDTH);
            }
            for tab in saved.tabs {
                app.restore_tab(tab);
                if saved.recovery_version == 0 && app.env.os == "windows" {
                    let tab = app.tabs.last_mut().unwrap();
                    if let Some(prompt) = shell::default_prompt(&tab.profile, &tab.cwd) {
                        tab.term.repair_legacy_prompt_gap(&prompt);
                    }
                }
            }
            app.active = saved.active.min(app.tabs.len().saturating_sub(1));
        }
        Ok(app)
    }
    /// Read private settings and launch fresh shells only after host commit starts.
    fn activate(&mut self) {
        match host::read_optional("settings.json", true) {
            Ok(Some(text)) => match Settings::parse_for_os(&text, &self.env.os) {
                Ok(s) => self.settings = s,
                Err(e) => self.error = Some(e.to_string()),
            },
            Ok(None) => {}
            Err(error) => self.error = Some(error),
        }
        if self.tabs.is_empty() {
            self.add(self.settings.default_profile, self.env.workspace.clone());
        } else {
            for index in 0..self.tabs.len() {
                if !self.tabs[index].exited {
                    self.spawn(index);
                }
            }
        }
    }
    /// Restore historical bytes to the emulator, never to a shell input pipe.
    fn restore_tab(&mut self, saved: SavedTab) {
        let extent = self.extent();
        let (rows, columns) = saved
            .display
            .as_ref()
            .map(|display| display.size())
            .unwrap_or((extent.rows as u16, extent.columns as u16));
        let mut term = emulator::Emulator::new(rows, columns, self.settings.history);
        term.restore(&saved.output, saved.display.as_ref());
        // Never replay query responses when restoring a snapshot.
        term.replies_mut().bytes.clear();
        self.tabs.push(Tab {
            id: saved.id,
            name: saved.name,
            profile: saved.profile,
            cwd: saved.cwd,
            handle: None,
            term,
            exited: saved.exited,
            metadata_parser: vte::Parser::new(),
            metadata: shell::Metadata::default(),
        });
    }
    fn extent(&self) -> Extent {
        Extent {
            columns: ((self.content_width() - 24.) / self.cw)
                .floor()
                .clamp(2., 1000.) as usize,
            rows: ((self.height - 16.) / self.ch).floor().clamp(1., 500.) as usize,
        }
    }
    /// Window resizing only changes the canvas; keep the user's divider width unchanged.
    fn effective_tab_width(&self) -> f32 {
        self.tab_width.clamp(MIN_TAB_WIDTH, MAX_TAB_WIDTH)
    }
    /// A left sidebar offsets every canvas cell, cursor and mouse coordinate equally.
    fn content_left(&self) -> f32 {
        0.
    }
    /// Reserve the same canvas width regardless of which side owns the tabs.
    fn content_width(&self) -> f32 {
        self.width.max(0.)
    }
    /// Share the canvas boundary across painting, scrolling and hit testing.
    fn content_right(&self) -> f32 {
        self.content_left() + self.content_width()
    }
    /// Native sidebar context menus report coordinates relative to their own dock edge.
    fn tab_origin(&self) -> f32 {
        match self.settings.tab_position {
            ui::SideTabsPosition::Left => 0.,
            ui::SideTabsPosition::Right => self.content_right(),
        }
    }
    /// Interactive creation uses the shell tool name without a growing numeric suffix.
    fn add(&mut self, profile: usize, cwd: String) {
        self.add_named(profile, cwd, None);
    }
    /// Use an explicit invocation name when supplied, preserving independent stable tab IDs.
    fn add_named(&mut self, profile: usize, cwd: String, name: Option<String>) -> Option<usize> {
        if !self.settings.enabled {
            self.error = Some("终端配置 enabled=false，不能创建新会话".into());
            return None;
        }
        if self.tabs.len() >= 32 {
            self.error = Some("最多可开启 32 个终端".into());
            return None;
        }
        let Some(profile) = self.settings.profiles.get(profile).cloned() else {
            self.error = Some("终端 Shell 配置不存在".into());
            return None;
        };
        let tool = profile
            .program
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("shell")
            .trim_end_matches(".exe")
            .to_lowercase();
        // Empty names fall back to the tool; labels never contain controls or exceed 80 characters.
        let name = name
            .map(|value| {
                value
                    .chars()
                    .filter(|ch| !ch.is_control())
                    .collect::<String>()
            })
            .map(|value| value.trim().chars().take(80).collect::<String>())
            .filter(|value| !value.is_empty())
            .unwrap_or(tool);
        let saved = SavedTab {
            exited: false,
            id: self.next_id,
            name,
            profile,
            cwd,
            output: String::new(),
            display: None,
        };
        self.next_id += 1;
        self.restore_tab(saved);
        self.active = self.tabs.len() - 1;
        self.spawn(self.active);
        self.menu = None;
        Some(self.active)
    }
    /// The host returns a resource handle owned only by this plugin instance.
    fn spawn(&mut self, index: usize) {
        let tab = &mut self.tabs[index];
        let prompt = shell::default_prompt(&tab.profile, &tab.cwd);
        tab.term
            .begin_process(self.env.os == "windows", prompt.as_deref());
        // A restored PTY starts at the saved grid size until the host reports its actual layout.
        let (rows, columns) = tab.term.screen().size();
        match host::process(process::Operation::Execute {
            program: tab.profile.program.clone(),
            args: shell::arguments(&tab.profile),
            cwd: (!tab.cwd.is_empty()).then(|| tab.cwd.clone()),
            transport: process::Transport::Pty { columns, rows },
        }) {
            Ok(api::Value::Resource(handle)) => {
                tab.handle = Some(handle);
                tab.exited = false;
            }
            Err(error) => {
                self.error = Some(error);
                tab.exited = true;
            }
            Ok(_) => {
                self.error = Some("Expected process handle".into());
                tab.exited = true;
            }
        }
    }
    fn send(&mut self, bytes: Vec<u8>) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.term.set_scrollback(0);
            if let Some(handle) = tab.handle.clone() {
                if let Err(e) = host::process(process::Operation::Write { handle, bytes }) {
                    self.error = Some(e);
                }
            }
        }
    }
    /// Close the selected process and ask the generic host to hide an empty terminal panel.
    fn close(&mut self, index: usize) {
        if index < self.tabs.len() {
            let active_id = self.tabs.get(self.active).map(|tab| tab.id);
            let tab = self.tabs.remove(index);
            if let Some(handle) = tab.handle {
                let _ = host::process(process::Operation::Terminate { handle });
            }
            self.active = self
                .tabs
                .iter()
                .position(|tab| Some(tab.id) == active_id)
                .unwrap_or(index.min(self.tabs.len().saturating_sub(1)));
            if self.tabs.is_empty() {
                self.menu = None;
                self.rename = None;
                self.selecting = false;
                // Closing a session is plugin behavior; native dock visibility belongs to the host.
                if let Err(error) = self.editor_request(
                    api::EditorOperation::SetPanelVisibility {
                        panel: "terminal".into(),
                        visible: false,
                    },
                    commands::PendingEditor::Effect,
                ) {
                    self.error = Some(error);
                }
            }
        }
    }
    fn reply(&self) -> api::Output {
        api::Output {
            views: vec![api::View {
                panel: "terminal".into(),
                document: self.document(),
            }],
            ..Default::default()
        }
    }
    /// Bound styled history by the host quota in addition to configured history rows.
    fn snapshot(&self) -> Snapshot {
        let budget = (8 * 1024 * 1024 / self.tabs.len().max(1)).max(1024);
        let tabs = self
            .tabs
            .iter()
            .map(|t| {
                let (output, display) = scene::history(t, budget);
                SavedTab {
                    exited: t.exited,
                    id: t.id,
                    name: t.name.clone(),
                    profile: t.profile.clone(),
                    cwd: t.cwd.clone(),
                    output,
                    display: Some(display),
                }
            })
            .collect();
        Snapshot {
            schema: 2,
            data: serde_json::to_string(&Saved {
                tabs,
                active: self.active,
                next_id: self.next_id,
                counts: BTreeMap::new(),
                settings: self.settings.clone(),
                tab_width: self.tab_width,
                recovery_version: 1,
            })
            .unwrap(),
        }
    }
}
