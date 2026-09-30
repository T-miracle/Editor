//! Terminal input, menus, project commands and tab state are implemented in the guest.
use super::*;
use crate::emulator::{MouseProtocolEncoding, MouseProtocolMode};
impl Terminal {
    /// Native input is forwarded as data and interpreted only by this plugin.
    pub(super) fn event(&mut self, event: Event) {
        self.ui_revision = self.ui_revision.wrapping_add(1);
        match event {
            Event::Ui(event) => self.ui_event(event),
            Event::Surface { event, .. } => self.event(*event),
            Event::Resize {
                width,
                height,
                cell_width,
                cell_height,
            } => {
                self.width = width;
                self.height = height;
                self.cw = cell_width.max(1.);
                self.ch = cell_height.max(1.);
                self.resize_grid();
            }
            Event::Theme(env) => self.env = env,
            Event::ProcessOutput { handle, bytes } => {
                let palette: Vec<_> = (0..269).map(|i| self.color(i)).collect();
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.handle == Some(handle)) {
                    tab.metadata_parser.advance(&mut tab.metadata, &bytes);
                    if let Some(cwd) = tab.metadata.cwd.take() {
                        tab.cwd = cwd;
                    }
                    tab.term.replies_mut().palette = palette;
                    tab.term.process(&bytes);
                    let bytes = std::mem::take(&mut tab.term.replies_mut().bytes);
                    if !bytes.is_empty() {
                        let _ = host(Request::Write { handle, bytes });
                    }
                }
            }
            Event::ProcessExit { handle } => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.handle == Some(handle)) {
                    tab.exited = true;
                    tab.handle = None;
                }
            }
            Event::Command {
                id,
                cwd,
                text,
                arguments,
            } => self.invoke_command(&id, cwd, text, arguments),
            Event::Key {
                key,
                ctrl,
                alt,
                shift,
            } => {
                // Clipboard shortcuts belong to the terminal UI, including their existing aliases.
                if ctrl && !alt && matches!(key.as_str(), "c" | "v") {
                    self.command(if key == "c" { "copy" } else { "paste" }, None, None);
                } else if ctrl && shift && !alt && matches!(key.as_str(), "t" | "w") {
                    match key.as_str() {
                        "t" => self.command("new", None, None),
                        "w" => self.close(self.active),
                        _ => {}
                    }
                } else if let Some(tab) = self.tabs.get(self.active) {
                    if let Some(bytes) = input::key(
                        &key,
                        ctrl,
                        alt,
                        shift,
                        tab.term.screen().application_cursor(),
                    ) {
                        self.send(bytes);
                    }
                }
            }
            Event::Text(text) => self.send(text.into_bytes()),
            Event::Paste(text) => {
                let bracketed = self
                    .tabs
                    .get(self.active)
                    .is_some_and(|t| t.term.screen().bracketed_paste());
                self.send(input::paste(&text, bracketed));
            }
            Event::Pointer {
                kind,
                x,
                y,
                button,
                clicks,
                shift,
            } => self.pointer(&kind, x, y, button, clicks, shift),
            Event::Wheel { delta, shift, x, y } => {
                if x >= self.tab_left() || self.menu.is_some() {
                    return;
                }
                if !shift
                    && self.tabs.get(self.active).is_some_and(|t| {
                        t.term.screen().mouse_protocol_mode() != MouseProtocolMode::None
                    })
                {
                    self.pointer("down", x, y, if delta > 0. { 64 } else { 65 }, 1, false);
                    return;
                }
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    if tab.term.screen().alternate_screen() && !shift {
                        let bytes = input::key(
                            if delta > 0. { "up" } else { "down" },
                            false,
                            false,
                            false,
                            tab.term.screen().application_cursor(),
                        )
                        .unwrap();
                        self.send(bytes.repeat(delta.abs().ceil().clamp(1., 20.) as usize));
                    } else {
                        tab.term.scroll(delta as i32);
                    }
                }
            }
            Event::Scroll { id: _, offset } => {
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    let target = (scene::visible_history(tab) as i32
                        - (offset / self.ch).round() as i32)
                        .max(0);
                    let delta = target - tab.term.screen().scrollback() as i32;
                    tab.term.scroll(delta);
                }
            }
            Event::Edit { id, text } => {
                if let Some(tab) = self
                    .tabs
                    .iter_mut()
                    .find(|t| format!("rename:{}", t.id) == id)
                {
                    if !text.trim().is_empty() {
                        tab.name = text.trim().chars().take(80).collect();
                    }
                }
                self.rename = None;
            }
            Event::Focus(focused) => {
                if self
                    .tabs
                    .get(self.active)
                    .is_some_and(|t| t.term.focus_mode())
                {
                    self.send(if focused { b"\x1b[I" } else { b"\x1b[O" }.to_vec());
                }
            }
        }
    }
    /// Synchronize the emulator and PTY only when the divider changes the cell grid.
    pub(super) fn resize_grid(&mut self) {
        let extent = self.extent();
        for tab in &mut self.tabs {
            tab.term.replies_mut().cell_size = (self.cw as u16, self.ch as u16);
            let dimensions = (extent.rows as u16, extent.columns as u16);
            // Pixel drag events need no ConPTY call until the cell count changes.
            if tab.term.screen().size() != dimensions {
                tab.term.resize(dimensions.0, dimensions.1);
                if let Some(handle) = tab.handle {
                    let _ = host(Request::Resize {
                        handle,
                        columns: dimensions.1,
                        rows: dimensions.0,
                    });
                }
            }
        }
    }
    /// Menu entries and declared host commands converge on these guest-owned actions.
    pub(super) fn command(&mut self, id: &str, cwd: Option<String>, text: Option<String>) {
        match id.trim_start_matches("terminal.") {
            "new" => self.add(
                self.settings.default_profile,
                cwd.unwrap_or(self.env.workspace.clone()),
            ),
            "menu" => {
                self.menu = if self.menu == Some(TerminalMenu::Commands) {
                    None
                } else {
                    Some(TerminalMenu::Commands)
                };
                self.menu_position = (0., 0.);
            }
            "close" => self.close(self.active),
            "next" => {
                if !self.tabs.is_empty() {
                    self.active = (self.active + 1) % self.tabs.len();
                }
            }
            "previous" => {
                if !self.tabs.is_empty() {
                    self.active = (self.active + self.tabs.len() - 1) % self.tabs.len();
                }
            }
            "interrupt" => self.send(vec![3]),
            "clear" => {
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.term.clear_history();
                }
            }
            "clear-buffer" => {
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.term.clear_buffer();
                }
                self.selecting = false;
            }
            "copy" => {
                // Never replace the clipboard when there is no nonempty selected text.
                if let Some(text) = self.selected_text() {
                    let _ = host(Request::ClipboardWrite(text));
                }
            }
            "paste" => {
                let _ = host(Request::ClipboardRead);
            }
            "selection" => {
                let _ = host(Request::Editor {
                    command: "selection".into(),
                });
            }
            "selection.result" => {
                if let Some(text) = text {
                    self.event(Event::Paste(text));
                }
            }
            "cwd" | "here" => {
                let _ = host(Request::Editor {
                    command: "active_directory".into(),
                });
            }
            "active_directory.result" => self.add(
                self.settings.default_profile,
                cwd.unwrap_or(self.env.workspace.clone()),
            ),
            "restart" => {
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    if let Some(handle) = tab.handle.take() {
                        let _ = host(Request::Close { handle });
                    }
                    self.spawn(self.active);
                }
            }
            "run" => self.invoke_command(id, cwd, text, None),
            "save.result" => {
                if let Some(options) = self.pending_runs.pop_front() {
                    self.run_project(options);
                }
            }
            "settings" => {
                let source = serde_json::to_string_pretty(&self.settings).unwrap();
                if host(Request::ReadData {
                    path: "settings.json".into(),
                })
                .is_err()
                {
                    let _ = host(Request::WriteData {
                        path: "settings.json".into(),
                        text: source,
                    });
                }
                let _ = host(Request::Editor {
                    command: "open_data:settings.json".into(),
                });
            }
            "reload" => {
                if let Ok(value) = host(Request::ReadData {
                    path: "settings.json".into(),
                }) {
                    match Settings::parse(value.as_str().unwrap_or("")) {
                        Ok(s) => {
                            self.settings = s;
                            for tab in &mut self.tabs {
                                tab.term.set_history_limit(self.settings.history);
                            }
                            self.error = None;
                        }
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
            }
            _ if id.starts_with("profile:") => {
                if let Ok(index) = id[8..].parse() {
                    self.add(index, self.env.workspace.clone());
                }
            }
            _ => {}
        }
    }
    /// Project detection reads only authorized workspace files and never auto-runs at launch.
    fn run_project(&mut self, options: commands::OpenOptions) {
        let command = options
            .command
            .or_else(|| self.settings.run_command.clone())
            .or_else(|| {
                if host(Request::ReadWorkspace {
                    path: "Cargo.toml".into(),
                })
                .is_ok()
                {
                    Some("cargo run".into())
                } else if let Ok(value) = host(Request::ReadWorkspace {
                    path: "package.json".into(),
                }) {
                    let package: serde_json::Value =
                        serde_json::from_str(value.as_str().unwrap_or("")).ok()?;
                    let scripts = package.get("scripts")?;
                    if scripts.get("dev").is_some() {
                        Some("npm run dev".into())
                    } else if scripts.get("start").is_some() {
                        Some("npm start".into())
                    } else {
                        None
                    }
                } else {
                    None
                }
            });
        if let Some(command) = command {
            // A rejected creation must not execute the task in a previously active terminal.
            if self
                .add_named(
                    options.profile.unwrap_or(self.settings.default_profile),
                    options.cwd.unwrap_or(self.env.workspace.clone()),
                    options.name,
                )
                .is_some()
            {
                self.send(format!("{}\r", command.trim_end_matches(['\r', '\n'])).into_bytes());
            }
        } else {
            self.error = Some("请在插件设置中指定 run_command".into());
        }
    }
    /// Hit testing, tab ordering and selection are terminal behavior.
    fn pointer(&mut self, kind: &str, x: f32, y: f32, button: u8, clicks: u8, shift: bool) {
        if x >= self.tab_left() || self.menu.is_some() || !x.is_finite() || !y.is_finite() {
            return;
        }
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let (rows, columns) = tab.term.screen().size();
        let col = ((x - 8.) / self.cw)
            .floor()
            .clamp(0., (columns as usize - 1) as f32) as usize;
        let row = ((y - 8.) / self.ch)
            .floor()
            .clamp(0., (rows as usize - 1) as f32) as i32;
        let mouse = tab.term.screen().mouse_protocol_mode();
        if !shift && mouse != MouseProtocolMode::None {
            if (kind == "move"
                && !matches!(
                    mouse,
                    MouseProtocolMode::ButtonMotion | MouseProtocolMode::AnyMotion
                ))
                || (kind == "up" && button >= 64)
            {
                return;
            }
            let code = if kind == "move" { 32 + button } else { button };
            if tab.term.screen().mouse_protocol_encoding() == MouseProtocolEncoding::Sgr {
                self.send(
                    format!(
                        "\x1b[<{code};{};{}{}",
                        col + 1,
                        row + 1,
                        if kind == "up" { 'm' } else { 'M' }
                    )
                    .into_bytes(),
                );
            } else if col < 223 && row < 223 {
                self.send(vec![
                    27,
                    b'[',
                    b'M',
                    32 + if kind == "up" { 3 } else { code },
                    33 + col as u8,
                    33 + row as u8,
                ]);
            }
            return;
        }
        if button == 2 && kind == "down" {
            // Opening a context menu preserves the current selection for an explicit copy action.
            self.selecting = false;
            self.menu = Some(TerminalMenu::Output);
            self.menu_position = (x.clamp(0., 10000.), y.clamp(0., 10000.));
            return;
        }
        if kind == "down" && button == 0 {
            // The unused grid and panel padding must not begin a local text selection.
            let in_grid = x >= 8.
                && y >= 8.
                && x < 8. + columns as f32 * self.cw
                && y < 8. + rows as f32 * self.ch;
            if in_grid && col < tab.term.screen().content_end(row as u16) as usize {
                tab.term.select(row as u16, col as u16, clicks);
                self.selecting = true;
            } else {
                tab.term.clear_selection();
                self.selecting = false;
            }
        } else if kind == "move" && self.selecting {
            tab.term.extend_selection(row as u16, col as u16);
        } else if kind == "up" {
            self.selecting = false;
        }
    }
    /// Shared availability check for keyboard copy and both native menus.
    pub(super) fn selected_text(&self) -> Option<String> {
        self.tabs
            .get(self.active)
            .and_then(|tab| tab.term.selection_text())
            .filter(|text| !text.is_empty())
    }
    /// The output context menu deliberately contains only the requested three commands.
    pub(super) fn output_menu_actions(&self) -> Vec<(String, String)> {
        [
            ("copy", "复制"),
            ("paste", "粘贴"),
            ("clear-buffer", "清空缓冲区"),
        ]
        .map(|(id, label)| (id.into(), label.into()))
        .into()
    }
    pub(super) fn menu_actions(&self) -> Vec<(String, String)> {
        let mut items: Vec<_> = self
            .settings
            .profiles
            .iter()
            .enumerate()
            .map(|(i, p)| (format!("profile:{i}"), p.name.clone()))
            .collect();
        items.extend(
            [
                ("run", "运行项目"),
                ("interrupt", "中断"),
                ("cwd", "在当前文件目录打开"),
                ("selection", "发送选中内容"),
                ("copy", "复制"),
                ("paste", "粘贴"),
                ("clear", "清除历史"),
                ("restart", "重启 Shell"),
                ("settings", "主题 / 配置"),
                ("reload", "重新加载配置"),
            ]
            .map(|(a, b)| (a.into(), b.into())),
        );
        items
    }
}
