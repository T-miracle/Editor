//! Task tab identity is independent from configuration and one round's revocable execution handles.

use super::*;
use crate::extensions::{HostWork, NativeExecutionMessage};

/// A persisted task identity reconnects presentation only; live handles belong to its current round.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Task {
    pub key: String,
    /// Persisted logical results never replay tasks after a window restart.
    #[serde(skip)]
    pub execution: Option<u64>,
    #[serde(skip)]
    pub request: u64,
    #[serde(skip)]
    pub close_waiting: bool,
    #[serde(skip)]
    pub process_alive: bool,
    #[serde(skip)]
    pub text_sources: BTreeMap<String, String>,
    /// Only preparations admitted in this round can append their retained output.
    #[serde(skip)]
    pub requests: std::collections::BTreeSet<u64>,
    /// Presentation is published by the parent rather than reading it from inside a child render.
    #[serde(skip)]
    pub controls: task_view::TaskControls,
}

impl TerminalPanel {
    /// Reveal an existing task without changing its launch round or retained output.
    pub(crate) fn locate_task(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let Some(id) = self
            .sessions
            .iter()
            .find(|session| session.task.as_ref().is_some_and(|task| task.key == key))
            .map(|session| session.id)
        else {
            return false;
        };
        self.active = Some(id);
        self.visible = true;
        self.measure_pending = true;
        self.focus_pending = true;
        cx.emit(PanelEvent::LayoutChanged);
        cx.notify();
        true
    }
    /// Snapshot cursors belong to this accepted round; a rerun clears them alongside its old grid.
    pub(crate) fn task_snapshot(
        &mut self,
        key: &str,
        source: &str,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(task) = self
            .sessions
            .iter_mut()
            .find(|session| session.task.as_ref().is_some_and(|task| task.key == key))
            .and_then(|session| session.task.as_mut())
        else {
            return;
        };
        if source.strip_prefix("preparation:").is_some_and(|request| {
            request
                .parse::<u64>()
                .is_ok_and(|request| !task.requests.contains(&request))
        }) {
            return;
        }
        let previous = task.text_sources.entry(source.into()).or_default();
        if previous == text {
            return;
        }
        // Bounded source buffers discard their oldest bytes. Keep the latest overlap so truncation
        // does not replay the whole retained tail into Alacritty on each subsequent publication.
        let added = snapshot_tail(previous, text).to_owned();
        *previous = text.into();
        self.task_text(key, added.as_bytes(), cx);
    }

    /// Publish stable task keys and optional execution receipts without exposing mutable tab state.
    pub(crate) fn task_keys(&self) -> Vec<(String, Option<u64>)> {
        self.sessions
            .iter()
            .filter_map(|session| {
                session
                    .task
                    .as_ref()
                    .map(|task| (task.key.clone(), task.execution))
            })
            .collect()
    }
    /// Called only after plan/save admission. A rejected plan leaves its prior visible output untouched.
    pub(crate) fn begin_task(
        &mut self,
        key: &str,
        name: &str,
        request: u64,
        cx: &mut Context<Self>,
    ) -> bool {
        let index = self
            .sessions
            .iter()
            .position(|session| session.task.as_ref().is_some_and(|task| task.key == key));
        let index = if let Some(index) = index {
            index
        } else {
            if self.sessions.len() >= 32 {
                self.error = Some(t!("terminal.session_limit").to_string());
                cx.notify();
                return false;
            }
            let Some(id) = self.next_id.checked_add(1) else {
                return false;
            };
            self.next_id = id;
            self.sessions.push(Session {
                id,
                name: name.into(),
                profile: self.settings.profiles[self.settings.default_profile].clone(),
                cwd: self.workspace.display().to_string(),
                engine: Engine::new(self.grid_size(), self.settings.history),
                launched: true,
                exited: false,
                restored: false,
                task: Some(Task {
                    key: key.into(),
                    execution: None,
                    request,
                    close_waiting: false,
                    process_alive: false,
                    text_sources: BTreeMap::new(),
                    requests: std::collections::BTreeSet::from([request]),
                    controls: Default::default(),
                }),
            });
            self.sessions.len() - 1
        };
        let size = self.grid_size();
        let session = &mut self.sessions[index];
        // A new process round starts with fresh VT modes; menu Clear retains modes in the same child.
        session.engine = Engine::new(size, self.settings.history);
        session.launched = true;
        session.exited = false;
        let task = session.task.as_mut().unwrap();
        task.execution = None;
        task.request = request;
        task.close_waiting = false;
        task.process_alive = false;
        task.text_sources.clear();
        task.requests = std::collections::BTreeSet::from([request]);
        self.active = Some(session.id);
        self.visible = true;
        self.measure_pending = true;
        self.focus_pending = true;
        self.dirty = true;
        cx.emit(PanelEvent::LayoutChanged);
        cx.notify();
        true
    }

    /// Every step changes the accepted request before dispatch, rejecting late events from prior steps.
    pub(crate) fn task_step(
        &mut self,
        key: &str,
        request: u64,
        label: &str,
        cx: &mut Context<Self>,
    ) {
        if let Some(session) = self
            .sessions
            .iter_mut()
            .find(|session| session.task.as_ref().is_some_and(|task| task.key == key))
        {
            let task = session.task.as_mut().unwrap();
            task.request = request;
            task.requests.insert(request);
            task.execution = None;
            task.process_alive = false;
            session.exited = false;
            session
                .engine
                .process(format!("\r\n[{}]\r\n", label).as_bytes());
            self.dirty = true;
            cx.notify();
        }
    }

    /// Provider identity and request join output to an existing tab; output alone never recreates a closed tab.
    pub(crate) fn observe_execution(
        &mut self,
        message: NativeExecutionMessage,
        cx: &mut Context<Self>,
    ) {
        let key = if message.config.is_empty() {
            format!("execution:{}", message.execution)
        } else {
            message.config.clone()
        };
        let index = self
            .sessions
            .iter()
            .position(|session| session.task.as_ref().is_some_and(|task| task.key == key));
        let index = match index {
            Some(index) => index,
            None if message.config.is_empty()
                && message.update.locate
                && !self.closed_tasks.contains(&key) =>
            {
                let name = message
                    .update
                    .request
                    .name
                    .as_deref()
                    .unwrap_or(&message.update.request.program);
                if !self.begin_task(&key, name, message.request_id, cx) {
                    // Public consumers can launch before the native view learns about admission.
                    // A full panel must stop that owned child rather than silently hide a live task.
                    self.io_host.read(cx).stage_host_run(HostWork::StopRun {
                        session: message.execution,
                        config: key,
                        mode: protocol::process::ExitMode::Force,
                        request_id: 0,
                    });
                    return;
                }
                self.active_index().unwrap()
            }
            _ => return,
        };
        let session = &mut self.sessions[index];
        let task = session.task.as_mut().unwrap();
        if task.request != message.request_id {
            return;
        }
        if message.update.locate {
            task.execution = Some(message.execution);
            // Locate also applies to ended histories; it must not manufacture a live input target.
            task.process_alive = !session.exited;
            session.profile.program = message.update.request.program.clone();
            if let Some(cwd) = &message.update.request.cwd {
                session.cwd = cwd.clone();
            }
            self.active = Some(session.id);
            self.visible = true;
            self.measure_pending = true;
            self.focus_pending = true;
            cx.emit(PanelEvent::LayoutChanged);
        }
        if task.execution != Some(message.execution) {
            return;
        }
        let id = session.id;
        for update in message.update.updates {
            self.apply_update(id, update, cx);
        }
        if let Some(failure) = message.update.failure {
            // A different active tab must not hide this owner's resize/cleanup failure. Retain a
            // deduplicated diagnostic in this round as well as the panel's visible error control.
            let text = format!(
                "\r\n{}\r\n",
                t!("terminal.error.process", details = failure.clone())
            );
            self.task_snapshot(&key, "native-error", &text, cx);
            self.report_error(FailureKind::Process, failure);
        }
        self.dirty = true;
        cx.notify();
    }

    /// Stdio/preparation snapshots are appended incrementally to the same task view, without VT replays.
    pub(crate) fn task_text(&mut self, key: &str, bytes: &[u8], cx: &mut Context<Self>) {
        if let Some(id) = self
            .sessions
            .iter()
            .find(|session| session.task.as_ref().is_some_and(|task| task.key == key))
            .map(|session| session.id)
        {
            // Stdio has no terminal newline conversion. Normalize LF once at this transport boundary.
            let mut text = Vec::with_capacity(bytes.len());
            for (index, byte) in bytes.iter().enumerate() {
                if *byte == b'\n' && (index == 0 || bytes[index - 1] != b'\r') {
                    text.push(b'\r');
                }
                text.push(*byte);
            }
            self.apply_update(
                id,
                Update::Output {
                    stream: protocol::process::Stream::Stdout,
                    bytes: text,
                },
                cx,
            );
        }
    }

    /// The host's sequence state determines activity; an intermediate build child ending is not final exit.
    pub(crate) fn finish_task(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(session) = self
            .sessions
            .iter_mut()
            .find(|session| session.task.as_ref().is_some_and(|task| task.key == key))
        {
            // Failed/revoked host state is not native cleanup. Keep a closing tab until PTY EOF/exit.
            if session.task.as_ref().unwrap().process_alive {
                return;
            }
            if session.exited && !session.task.as_ref().unwrap().close_waiting {
                return;
            }
            session.exited = true;
            let remove = session.task.as_ref().unwrap().close_waiting;
            let id = session.id;
            if remove {
                self.remove_session(id, cx);
            } else {
                cx.notify();
            }
        }
    }

    /// A task's input uses its public host execution receipt; Shell tabs use their own native supervisor.
    pub(super) fn send_input(&mut self, id: u64, bytes: Vec<u8>, cx: &mut App) {
        let execution = self
            .sessions
            .iter()
            .find(|session| session.id == id)
            .and_then(|session| session.task.as_ref())
            .filter(|task| task.process_alive)
            .and_then(|task| task.execution);
        if let Some(execution) = execution {
            for bytes in bytes.chunks(1024) {
                self.io_host
                    .read(cx)
                    .stage_host_run(HostWork::ExecutionInput {
                        session: execution,
                        bytes: bytes.to_vec(),
                    });
            }
        } else if self
            .sessions
            .iter()
            .any(|session| session.id == id && session.task.is_none())
        {
            if let Err(error) = self.supervisor.write(id, bytes) {
                self.report_error(FailureKind::Process, error);
            }
        }
    }

    /// Geometry follows the selected resource; shared PTY coalescing still owns native resize policy.
    pub(super) fn resize_process(&self, id: u64, size: GridSize, cx: &App) {
        let task = self
            .sessions
            .iter()
            .find(|session| session.id == id)
            .and_then(|session| session.task.as_ref());
        if let Some(task) = task {
            if let Some(execution) = task.execution {
                self.io_host
                    .read(cx)
                    .stage_host_run(HostWork::ExecutionResize {
                        session: execution,
                        columns: size.columns as u16,
                        rows: size.rows as u16,
                    });
            }
        } else {
            let _ = self
                .supervisor
                .resize(id, size.columns as u16, size.rows as u16);
        }
    }

    /// Active task closure is a separate confirmation from hiding the panel; cancel makes no side effect.
    pub(super) fn close(&mut self, id: u64, cx: &mut Context<Self>) {
        if self
            .sessions
            .iter()
            .any(|session| session.id == id && session.task.is_some() && !session.exited)
        {
            self.pending_close = Some(id);
            self.visible = true;
            cx.notify();
        } else {
            self.remove_session(id, cx);
        }
    }

    pub(super) fn confirm_close(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.pending_close.take() else {
            return;
        };
        let Some(session) = self.sessions.iter_mut().find(|session| session.id == id) else {
            return;
        };
        if session.exited {
            self.remove_session(id, cx);
            return;
        }
        let Some(task) = &mut session.task else {
            return;
        };
        task.close_waiting = true;
        let key = task.key.clone();
        let execution = task.execution;
        if let Some(parent) = self.parent.upgrade() {
            cx.defer(move |cx| {
                parent.update(cx, |app, cx| app.stop_terminal_task(&key, execution, cx))
            });
        }
        cx.notify();
    }
}

/// Return only bytes beyond the last observed suffix, including UTF-8-safe bounded-buffer rollover.
fn snapshot_tail<'a>(previous: &str, current: &'a str) -> &'a str {
    if let Some(tail) = current.strip_prefix(previous) {
        return tail;
    }
    let mut start = previous.len().saturating_sub(4096);
    while !previous.is_char_boundary(start) {
        start += 1;
    }
    let suffix = &previous[start..];
    if !suffix.is_empty()
        && let Some(at) = current.rfind(suffix)
    {
        return &current[at + suffix.len()..];
    }
    current
}
