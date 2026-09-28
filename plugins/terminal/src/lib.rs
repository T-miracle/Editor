//! Terminal application: VT parsing, layout, profiles and interaction live in this guest.
mod config;
mod emulator;
mod input;
mod interaction;
mod scene;
mod shell;
#[cfg(test)]
mod tests;
mod theme;
use config::{Profile, Settings};
use plugin_protocol::*;
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, collections::BTreeMap};
wit_bindgen::generate!({ path: "../../crates/plugin-protocol/wit", world: "plugin" });
struct TerminalPlugin;
thread_local! { static APP: RefCell<Option<Terminal>> = const { RefCell::new(None) }; }
// Keep the terminal usable while allowing a wider session list for long names.
const DEFAULT_TAB_WIDTH: f32 = 180.;
const MIN_TAB_WIDTH: f32 = 112.;
const MAX_TAB_WIDTH: f32 = 480.;
const MIN_CONTENT_WIDTH: f32 = 80.;
// The resize target belongs to the tab bar, immediately inside its left edge.
const TAB_RESIZE_HANDLE_WIDTH: f32 = 8.;

/// Older snapshots did not store the width of the terminal's tab list.
fn default_tab_width() -> f32 {
    DEFAULT_TAB_WIDTH
}
impl Guest for TerminalPlugin {
    /// A serial event loop keeps parsing and UI mutation in the same isolated instance.
    fn dispatch(payload: String) -> Result<String, String> {
        let message: Message = serde_json::from_str(&payload).map_err(|e| e.to_string())?;
        APP.with(|cell| {
            let mut app = cell.borrow_mut();
            if let Message::Prepare {
                environment,
                snapshot,
            } = &message
            {
                *app = Some(Terminal::prepare(environment.clone(), snapshot.clone())?);
            }
            let terminal = app.as_mut().ok_or("Plugin has not been prepared")?;
            let reply = match message {
                Message::Prepare { .. } => Reply::default(),
                Message::Activate => {
                    terminal.activate();
                    terminal.reply()
                }
                Message::Event(event) => {
                    terminal.event(event);
                    terminal.reply()
                }
                Message::Snapshot => Reply {
                    snapshot: Some(terminal.snapshot()),
                    ..Reply::default()
                },
            };
            serde_json::to_string(&reply).map_err(|e| e.to_string())
        })
    }
}
export!(TerminalPlugin);

/// Native operations cross the permission-checked component import.
fn host(request: Request) -> Result<serde_json::Value, String> {
    #[cfg(test)]
    {
        return tests::host(request);
    }
    #[cfg(not(test))]
    {
        let payload = serde_json::to_string(&request).map_err(|e| e.to_string())?;
        let result = editor::plugin::host::request(&payload)?;
        serde_json::from_str(&result).map_err(|e| e.to_string())
    }
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
    handle: Option<u64>,
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
    counts: BTreeMap<String, usize>,
    settings: Settings,
    #[serde(default = "default_tab_width")]
    tab_width: f32,
}
#[derive(Serialize, Deserialize)]
struct SavedTab {
    id: u64,
    name: String,
    profile: Profile,
    cwd: String,
    output: String,
}
struct Terminal {
    env: Environment,
    settings: Settings,
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    counts: BTreeMap<String, usize>,
    width: f32,
    height: f32,
    tab_width: f32,
    cw: f32,
    ch: f32,
    menu: bool,
    tab_scroll: usize,
    menu_scroll: usize,
    rename: Option<u64>,
    selecting: bool,
    error: Option<String>,
    drag: Option<usize>,
    resizing_tab_bar: bool,
}
impl Terminal {
    /// Validate and migrate saved data without acquiring any OS resources.
    fn prepare(env: Environment, snapshot: Option<Snapshot>) -> Result<Self, String> {
        let mut settings = Settings::default();
        if env.os == "windows" {
            settings.profiles = vec![
                Profile {
                    name: "PowerShell".into(),
                    program: "powershell.exe".into(),
                    args: vec!["-NoLogo".into()],
                },
                Profile {
                    name: "Command Prompt".into(),
                    program: "cmd.exe".into(),
                    args: vec![],
                },
                Profile {
                    name: "PowerShell 7".into(),
                    program: "pwsh.exe".into(),
                    args: vec!["-NoLogo".into()],
                },
                Profile {
                    name: "WSL".into(),
                    program: "wsl.exe".into(),
                    args: vec![],
                },
            ];
        }
        let mut app = Self {
            env,
            settings,
            tabs: vec![],
            active: 0,
            next_id: 1,
            counts: BTreeMap::new(),
            width: 800.,
            height: 240.,
            tab_width: DEFAULT_TAB_WIDTH,
            cw: 8.4,
            ch: 21.,
            menu: false,
            tab_scroll: 0,
            menu_scroll: 0,
            rename: None,
            selecting: false,
            error: None,
            drag: None,
            resizing_tab_bar: false,
        };
        if let Some(snapshot) = snapshot {
            if snapshot.schema != 1 {
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
            app.counts = saved.counts;
            // Snapshot data is user-controlled; reject non-finite layout values.
            if saved.tab_width.is_finite() {
                app.tab_width = saved.tab_width.clamp(MIN_TAB_WIDTH, MAX_TAB_WIDTH);
            }
            for saved in saved.tabs {
                app.restore_tab(saved);
            }
            app.active = saved.active.min(app.tabs.len().saturating_sub(1));
        }
        Ok(app)
    }
    /// Read private settings and launch fresh shells only after host commit starts.
    fn activate(&mut self) {
        if let Ok(value) = host(Request::ReadData {
            path: "settings.json".into(),
        }) {
            if let Some(text) = value.as_str() {
                match Settings::parse(text) {
                    Ok(s) => self.settings = s,
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
        }
        if self.tabs.is_empty() {
            self.add(self.settings.default_profile, self.env.workspace.clone());
        } else {
            for index in 0..self.tabs.len() {
                self.spawn(index);
            }
        }
    }
    /// Restore historical bytes to the emulator, never to a shell input pipe.
    fn restore_tab(&mut self, saved: SavedTab) {
        let extent = self.extent();
        let mut term = emulator::Emulator::new(
            extent.rows as u16,
            extent.columns as u16,
            self.settings.history,
        );
        term.process(saved.output.as_bytes());
        // Reinstalling a package must not append another identical history separator.
        if !saved.output.is_empty() && !saved.output.contains("--- restored session; new shell ---")
        {
            term.process(b"\r\n\x1b[0m--- restored session; new shell ---\r\n");
        }
        // Never replay query responses when restoring a snapshot.
        term.replies_mut().bytes.clear();
        self.tabs.push(Tab {
            id: saved.id,
            name: saved.name,
            profile: saved.profile,
            cwd: saved.cwd,
            handle: None,
            term,
            exited: false,
            metadata_parser: vte::Parser::new(),
            metadata: shell::Metadata::default(),
        });
    }
    fn extent(&self) -> Extent {
        Extent {
            columns: ((self.tab_left() - 24.) / self.cw).floor().clamp(2., 1000.) as usize,
            rows: ((self.height - 16.) / self.ch).floor().clamp(1., 500.) as usize,
        }
    }
    /// Limit the divider to the available panel while preserving terminal space.
    fn effective_tab_width(&self) -> f32 {
        self.tab_width
            .clamp(MIN_TAB_WIDTH.min(self.width.max(0.)), MAX_TAB_WIDTH)
            .min((self.width - MIN_CONTENT_WIDTH).max(0.))
    }
    /// Share one divider coordinate across painting, hit testing and PTY sizing.
    fn tab_left(&self) -> f32 {
        (self.width - self.effective_tab_width()).max(0.)
    }
    /// Count each shell independently; process OSC titles cannot replace user names.
    fn add(&mut self, profile: usize, cwd: String) {
        if !self.settings.enabled {
            self.error = Some("终端配置 enabled=false，不能创建新会话".into());
            return;
        }
        if self.tabs.len() >= 32 {
            self.error = Some("最多可开启 32 个终端".into());
            return;
        }
        let Some(profile) = self.settings.profiles.get(profile).cloned() else {
            return;
        };
        let tool = profile
            .program
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("shell")
            .trim_end_matches(".exe")
            .to_lowercase();
        let count = self.counts.entry(tool.clone()).or_default();
        *count += 1;
        let name = if *count == 1 {
            tool
        } else {
            format!("{tool}{count}")
        };
        let saved = SavedTab {
            id: self.next_id,
            name,
            profile,
            cwd,
            output: String::new(),
        };
        self.next_id += 1;
        self.restore_tab(saved);
        self.active = self.tabs.len() - 1;
        self.tab_scroll = self
            .active
            .saturating_sub((self.height / 32.).floor().max(1.) as usize - 1);
        self.spawn(self.active);
        self.menu = false;
    }
    /// The host returns a resource handle owned only by this plugin instance.
    fn spawn(&mut self, index: usize) {
        let extent = self.extent();
        let tab = &mut self.tabs[index];
        match host(Request::Spawn {
            program: tab.profile.program.clone(),
            args: shell::arguments(&tab.profile),
            cwd: tab.cwd.clone(),
            columns: extent.columns as u16,
            rows: extent.rows as u16,
        }) {
            Ok(value) => {
                tab.handle = value.as_u64();
                tab.exited = false;
            }
            Err(error) => {
                self.error = Some(error);
                tab.exited = true;
            }
        }
    }
    fn send(&mut self, bytes: Vec<u8>) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.term.set_scrollback(0);
            if let Some(handle) = tab.handle {
                if let Err(e) = host(Request::Write { handle, bytes }) {
                    self.error = Some(e);
                }
            }
        }
    }
    fn close(&mut self, index: usize) {
        if index < self.tabs.len() {
            let tab = self.tabs.remove(index);
            if let Some(handle) = tab.handle {
                let _ = host(Request::Close { handle });
            }
            self.active = self.active.min(self.tabs.len().saturating_sub(1));
        }
    }
    fn reply(&self) -> Reply {
        Reply {
            scene: Some(self.scene()),
            error: self.error.clone(),
            ..Reply::default()
        }
    }
    /// Bound styled history by the host quota in addition to configured history rows.
    fn snapshot(&self) -> Snapshot {
        let budget = (8 * 1024 * 1024 / self.tabs.len().max(1)).max(1024);
        let tabs = self
            .tabs
            .iter()
            .map(|t| SavedTab {
                id: t.id,
                name: t.name.clone(),
                profile: t.profile.clone(),
                cwd: t.cwd.clone(),
                output: scene::history(t, budget),
            })
            .collect();
        Snapshot {
            schema: 1,
            data: serde_json::to_string(&Saved {
                tabs,
                active: self.active,
                next_id: self.next_id,
                counts: self.counts.clone(),
                settings: self.settings.clone(),
                tab_width: self.tab_width,
            })
            .unwrap(),
        }
    }
}
