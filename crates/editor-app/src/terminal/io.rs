//! Native output/replies and settings refresh, independent from dock and tab management.

use super::*;

impl TerminalPanel {
    /// Alacritty parses once for both Shell and tasks, and replies follow the same resource that produced output.
    pub(super) fn apply_update(&mut self, id: u64, update: Update, cx: &mut Context<Self>) {
        let colors = self.colors(cx);
        let size = alacritty_terminal::event::WindowSize {
            num_lines: self.grid_size().rows as u16,
            num_cols: self.grid_size().columns as u16,
            cell_width: self.cell_width as u16,
            cell_height: self.cell_height as u16,
        };
        let Some(session) = self.sessions.iter_mut().find(|session| session.id == id) else {
            return;
        };
        let mut replies = vec![];
        match update {
            Update::Output { bytes, .. } => {
                for event in session.engine.process(&bytes) {
                    use alacritty_terminal::event::Event;
                    match event {
                        Event::PtyWrite(text) => replies.push(text.into_bytes()),
                        Event::ColorRequest(index, format) => {
                            let color = session.engine.color_index(index, &colors);
                            replies.push(
                                format(alacritty_terminal::vte::ansi::Rgb {
                                    r: (color >> 16) as u8,
                                    g: (color >> 8) as u8,
                                    b: color as u8,
                                })
                                .into_bytes(),
                            );
                        }
                        Event::TextAreaSizeRequest(format) => {
                            replies.push(format(size).into_bytes())
                        }
                        // Task output has process authority only. OSC52 must not bypass the
                        // clipboard capability; explicit user Copy remains available in the view.
                        Event::ClipboardStore(_, text) if session.task.is_none() => {
                            cx.write_to_clipboard(ClipboardItem::new_string(text))
                        }
                        _ => {}
                    }
                }
                if let Some(cwd) = session.engine.take_cwd() {
                    session.cwd = cwd;
                }
            }
            Update::Exited { .. } | Update::Terminated => {
                session.engine.end_process();
                if let Some(task) = &mut session.task {
                    task.process_alive = false;
                } else {
                    session.exited = true;
                }
            }
        }
        for bytes in replies {
            self.send_input(id, bytes, cx);
        }
        self.dirty = true;
        cx.notify();
    }
    /// Output parsing and reply dispatch stay on the same view thread as selection and painting.
    pub(super) fn poll(&mut self, cx: &mut Context<Self>) {
        let events = self.supervisor.poll();
        let mut changed = !events.is_empty();
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
                        tab.engine.end_process();
                    }
                }
                NativeProcessEvent::Update { session, update } => {
                    self.apply_update(session, update, cx)
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
