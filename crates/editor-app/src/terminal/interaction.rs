//! Native keyboard, pointer and menu policy; terminal bytes never pass through a WASM dispatcher.

use super::*;
use alacritty_terminal::term::TermMode;
use protocol::ui::{MenuItem, PointerPhase};

impl TerminalPanel {
    pub(super) fn canvas_event(&mut self, event: CanvasEvent, cx: &mut Context<Self>) {
        match event {
            CanvasEvent::Resize {
                width,
                height,
                grid,
            } => {
                self.width = width;
                self.height = height;
                if let Some(grid) = grid {
                    self.cell_width = grid.cell_width;
                    self.cell_height = grid.cell_height;
                }
                let size = self.grid_size();
                if let Some(index) = self.active_index() {
                    let tab = &mut self.sessions[index];
                    tab.engine.resize(size);
                    if tab.launched && !tab.exited {
                        let _ =
                            self.supervisor
                                .resize(tab.id, size.columns as u16, size.rows as u16);
                    }
                }
                self.launch_active();
            }
            CanvasEvent::Text { text } => self.write(text.into_bytes()),
            CanvasEvent::Key {
                key,
                ctrl,
                alt,
                shift,
            } => {
                if ctrl && shift && !alt && key == "t" {
                    self.new_shell(self.settings.default_profile, cx);
                } else if ctrl && shift && !alt && key == "w" {
                    if let Some(id) = self.active {
                        self.close(id, cx);
                    }
                } else if ctrl && !alt && key == "c" {
                    self.copy(cx);
                } else if ctrl && !alt && key == "v" {
                    self.paste(cx);
                } else if let Some(index) = self.active_index() {
                    let mode = self.sessions[index].engine.mode();
                    let page = self.grid_size().rows as i32;
                    if shift && key == "pageup" {
                        self.sessions[index].engine.scroll(page);
                    } else if shift && key == "pagedown" {
                        self.sessions[index].engine.scroll(-page);
                    } else if let Some(bytes) =
                        input::key(&key, ctrl, alt, shift, mode.contains(TermMode::APP_CURSOR))
                    {
                        self.write(bytes);
                    }
                }
            }
            CanvasEvent::Scroll { offset } => {
                if let Some(index) = self.active_index() {
                    let history = self.sessions[index].engine.history();
                    self.sessions[index].engine.set_offset(
                        history.saturating_sub((offset / self.cell_height).round() as usize),
                    );
                }
            }
            CanvasEvent::Wheel {
                delta_y,
                shift,
                x,
                y,
                ..
            } => {
                if let Some(index) = self.active_index() {
                    if !shift
                        && self.sessions[index]
                            .engine
                            .mode()
                            .intersects(TermMode::MOUSE_MODE)
                    {
                        self.report_mouse(if delta_y > 0. { 64 } else { 65 }, x, y, false);
                    } else {
                        self.sessions[index]
                            .engine
                            .scroll(if delta_y > 0. { 3 } else { -3 });
                    }
                }
            }
            CanvasEvent::Pointer {
                phase,
                x,
                y,
                button,
                clicks,
                shift,
            } => {
                if let Some(index) = self.active_index() {
                    let mouse = self.sessions[index]
                        .engine
                        .mode()
                        .intersects(TermMode::MOUSE_MODE);
                    if mouse && !shift {
                        let moving = phase == PointerPhase::Move;
                        if !moving
                            || self.sessions[index]
                                .engine
                                .mode()
                                .intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION)
                        {
                            self.report_mouse(
                                button as u8 + if moving { 32 } else { 0 },
                                x,
                                y,
                                phase == PointerPhase::Up,
                            );
                        }
                    } else if button == 2 && phase == PointerPhase::Down {
                        self.requested_menu = Some(true);
                    } else if button == 0 && x >= 8. && y >= 8. {
                        let row = ((y - 8.) / self.cell_height).floor() as usize;
                        let column = ((x - 8.) / self.cell_width).floor() as usize;
                        if row < self.grid_size().rows
                            && column < self.grid_size().columns
                            && phase != PointerPhase::Up
                        {
                            self.sessions[index].engine.select(
                                row,
                                column,
                                phase == PointerPhase::Down,
                                clicks,
                            );
                        }
                    }
                }
            }
            CanvasEvent::Focus { focused } => {
                if self.active_index().is_some_and(|index| {
                    self.sessions[index]
                        .engine
                        .mode()
                        .contains(TermMode::FOCUS_IN_OUT)
                }) {
                    self.write(if focused {
                        b"\x1b[I".to_vec()
                    } else {
                        b"\x1b[O".to_vec()
                    });
                }
            }
        }
        self.dirty = true;
        cx.notify();
    }

    /// SGR and legacy mouse bytes are addressed only to the selected terminal's owned process.
    fn report_mouse(&mut self, button: u8, x: f32, y: f32, release: bool) {
        let Some(index) = self.active_index() else {
            return;
        };
        let column = ((x - 8.) / self.cell_width).floor().max(0.) as usize + 1;
        let row = ((y - 8.) / self.cell_height).floor().max(0.) as usize + 1;
        let sgr = self.sessions[index]
            .engine
            .mode()
            .contains(TermMode::SGR_MOUSE);
        let bytes = if sgr {
            format!(
                "\x1b[<{button};{column};{row}{}",
                if release { 'm' } else { 'M' }
            )
            .into_bytes()
        } else if column <= 223 && row <= 223 {
            vec![
                27,
                b'[',
                b'M',
                32 + if release { 3 } else { button },
                32 + column as u8,
                32 + row as u8,
            ]
        } else {
            return;
        };
        self.write(bytes);
    }

    fn copy(&self, cx: &mut App) {
        if let Some(text) = self
            .active_index()
            .and_then(|index| self.sessions[index].engine.selected_text())
        {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }
    fn paste(&mut self, cx: &mut App) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let bracketed = self.active_index().is_some_and(|index| {
                self.sessions[index]
                    .engine
                    .mode()
                    .contains(TermMode::BRACKETED_PASTE)
            });
            self.write(input::paste(&text, bracketed));
        }
    }

    pub(super) fn menu_items(&self, output: bool) -> Vec<MenuItem> {
        let selection = self
            .active_index()
            .and_then(|index| self.sessions[index].engine.selected_text())
            .is_some();
        let mut items = vec![
            MenuItem {
                id: "copy".into(),
                label: t!("editor.copy").to_string(),
                disabled: !selection,
                separator_before: false,
            },
            MenuItem {
                id: "paste".into(),
                label: t!("editor.paste").to_string(),
                disabled: self.active.is_none(),
                separator_before: false,
            },
            MenuItem {
                id: "clear".into(),
                label: t!("terminal.clear").to_string(),
                disabled: self.active.is_none(),
                separator_before: false,
            },
        ];
        if !output {
            items.push(MenuItem {
                id: "interrupt".into(),
                label: t!("terminal.interrupt").to_string(),
                disabled: self.active.is_none(),
                separator_before: true,
            });
            items.push(MenuItem {
                id: "settings".into(),
                label: t!("terminal.settings").to_string(),
                disabled: false,
                separator_before: false,
            });
            for (index, profile) in self.settings.profiles.iter().enumerate() {
                items.push(MenuItem {
                    id: format!("new:{index}"),
                    label: profile.name.clone(),
                    disabled: false,
                    separator_before: index == 0,
                });
            }
        }
        items
    }

    pub(super) fn menu_action(
        &mut self,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.popup = None;
        if let Action::Select(id) = action {
            match id.as_str() {
                "copy" => self.copy(cx),
                "paste" => self.paste(cx),
                "clear" => {
                    if let Some(index) = self.active_index() {
                        self.sessions[index].engine.clear();
                        self.dirty = true;
                    }
                }
                "interrupt" => self.write(vec![3]),
                "settings" => {
                    let path = self.storage.join("settings.json");
                    // Existing user edits remain intact when reopening settings.
                    let result = std::fs::create_dir_all(&self.storage).and_then(|_| {
                        if path.exists() {
                            Ok(())
                        } else {
                            std::fs::write(
                                &path,
                                serde_json::to_vec_pretty(&self.settings).unwrap_or_default(),
                            )
                        }
                    });
                    if let Err(error) = result {
                        self.report_error(FailureKind::Settings, error);
                    } else {
                        let _ = self
                            .parent
                            .update(cx, |app, cx| app.open_file(path, window, cx));
                    }
                }
                _ => {
                    if let Some(index) = id.strip_prefix("new:").and_then(|id| id.parse().ok()) {
                        self.new_shell(index, cx);
                    }
                }
            }
        }
        cx.notify();
    }
}
