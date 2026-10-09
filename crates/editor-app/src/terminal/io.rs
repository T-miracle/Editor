//! Native output/replies and settings refresh, independent from dock and tab management.

use super::*;

impl TerminalPanel {
    /// Output parsing and reply dispatch stay on the same view thread as selection and painting.
    pub(super) fn poll(&mut self, cx: &mut Context<Self>) {
        let events = self.supervisor.poll();
        let mut changed = !events.is_empty();
        let colors = self.colors(cx);
        for event in events {
            match event {
                NativeProcessEvent::Started { .. } => {}
                NativeProcessEvent::Failed {
                    session,
                    message,
                    launch,
                } => {
                    self.report_error(FailureKind::Process, message);
                    if let Some(tab) = self.sessions.iter_mut().find(|tab| tab.id == session)
                        && launch
                    {
                        tab.exited = true;
                    }
                }
                NativeProcessEvent::Update { session, update } => {
                    let Some(tab) = self.sessions.iter_mut().find(|tab| tab.id == session) else {
                        continue;
                    };
                    match update {
                        Update::Output { bytes, .. } => {
                            for event in tab.engine.process(&bytes) {
                                match event {
                                    alacritty_terminal::event::Event::PtyWrite(text) => {
                                        let _ = self.supervisor.write(session, text.into_bytes());
                                    }
                                    alacritty_terminal::event::Event::ColorRequest(
                                        index,
                                        format,
                                    ) => {
                                        let color = tab.engine.color_index(index, &colors);
                                        let rgb = alacritty_terminal::vte::ansi::Rgb {
                                            r: (color >> 16) as u8,
                                            g: (color >> 8) as u8,
                                            b: color as u8,
                                        };
                                        let _ = self
                                            .supervisor
                                            .write(session, format(rgb).into_bytes());
                                    }
                                    alacritty_terminal::event::Event::TextAreaSizeRequest(
                                        format,
                                    ) => {
                                        let size = alacritty_terminal::event::WindowSize {
                                            num_lines: ((self.height - 16.) / self.cell_height)
                                                .floor()
                                                .clamp(1., 500.)
                                                as u16,
                                            num_cols: ((self.width - 16.) / self.cell_width)
                                                .floor()
                                                .clamp(2., 1000.)
                                                as u16,
                                            cell_width: self.cell_width as u16,
                                            cell_height: self.cell_height as u16,
                                        };
                                        let _ = self
                                            .supervisor
                                            .write(session, format(size).into_bytes());
                                    }
                                    // OSC clipboard reads are never silently granted to a program.
                                    alacritty_terminal::event::Event::ClipboardStore(_, text) => {
                                        cx.write_to_clipboard(ClipboardItem::new_string(text))
                                    }
                                    _ => {}
                                }
                            }
                            if let Some(cwd) = tab.engine.take_cwd() {
                                tab.cwd = cwd;
                            }
                        }
                        Update::Exited { .. } | Update::Terminated => tab.exited = true,
                    }
                    self.dirty = true;
                }
            }
        }
        if self.dirty && self.last_save.elapsed() >= Duration::from_secs(2) {
            if let Err(error) = self.save() {
                self.report_error(FailureKind::Save, error);
            }
            self.last_save = Instant::now();
        }
        if let Ok(metadata) = std::fs::metadata(self.storage.join("settings.json")) {
            let stamp = metadata.modified().ok();
            if stamp != self.settings_stamp {
                self.settings_stamp = stamp;
                let result = (|| {
                    anyhow::ensure!(metadata.len() <= 65536, t!("terminal.storage_quota"));
                    Settings::parse_for_os(
                        &std::fs::read_to_string(self.storage.join("settings.json"))?,
                        std::env::consts::OS,
                    )
                })();
                match result {
                    Ok(settings) => {
                        for session in &mut self.sessions {
                            session.engine.set_history(settings.history);
                        }
                        self.settings = settings;
                        self.measure_pending = true;
                        self.dirty = true;
                        self.error = None;
                    }
                    Err(error) => self.report_error(FailureKind::Settings, error),
                }
                changed = true;
            }
        }
        if changed {
            cx.notify();
        }
    }
}
