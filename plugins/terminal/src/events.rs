//! Internal terminal interactions are local Rust values; only the typed public SDK crosses WASM.
use super::*;

pub(super) enum Event {
    Ui(ui::UiEvent),
    Resize {
        width: f32,
        height: f32,
        cell_width: f32,
        cell_height: f32,
    },
    Theme(Environment),
    ProcessOutput {
        handle: api::ResourceHandle,
        bytes: Vec<u8>,
    },
    ProcessExit {
        handle: api::ResourceHandle,
    },
    Command {
        id: String,
        cwd: Option<String>,
        text: Option<String>,
        arguments: Option<serde_json::Value>,
    },
    Key {
        key: String,
        ctrl: bool,
        alt: bool,
        shift: bool,
    },
    Text(String),
    #[cfg(test)]
    Paste(String),
    Pointer {
        kind: String,
        x: f32,
        y: f32,
        button: u8,
        clicks: u8,
        shift: bool,
    },
    Wheel {
        delta: f32,
        shift: bool,
        x: f32,
        y: f32,
    },
    Scroll {
        offset: f32,
    },
    Focus(bool),
}

impl Terminal {
    /// Decode generic addressed canvas/process events; no terminal policy is added to the host.
    pub(super) fn notify(&mut self, notification: api::Notification) {
        let event = match notification {
            api::Notification::Process {
                handle,
                update: process::Update::Output { bytes, .. },
            } => Event::ProcessOutput { handle, bytes },
            api::Notification::Process { handle, .. } => Event::ProcessExit { handle },
            api::Notification::Theme(environment) => Event::Theme(environment),
            api::Notification::Tool(event)
                if event.revision == self.ui_revision
                    && event.target
                        == (ui::ToolTarget::Window {
                            panel: "terminal".into(),
                        })
                    && matches!(event.tool.as_str(), "terminal.new" | "terminal.menu") =>
            {
                Event::Command {
                    id: event.tool,
                    cwd: None,
                    text: None,
                    arguments: None,
                }
            }
            api::Notification::Command { id, arguments, .. } => Event::Command {
                id,
                cwd: None,
                text: None,
                arguments,
            },
            api::Notification::Ui(ui::UiEvent {
                action: ui::Action::Canvas(event),
                ..
            }) => match event {
                ui::CanvasEvent::Resize {
                    width,
                    height,
                    grid: Some(grid),
                } => Event::Resize {
                    width,
                    height,
                    cell_width: grid.cell_width,
                    cell_height: grid.cell_height,
                },
                ui::CanvasEvent::Key {
                    key,
                    ctrl,
                    alt,
                    shift,
                } => Event::Key {
                    key,
                    ctrl,
                    alt,
                    shift,
                },
                ui::CanvasEvent::Text { text } => Event::Text(text),
                ui::CanvasEvent::Focus { focused } => Event::Focus(focused),
                ui::CanvasEvent::Scroll { offset } => Event::Scroll { offset },
                ui::CanvasEvent::Wheel {
                    x,
                    y,
                    delta_y,
                    shift,
                    ..
                } => Event::Wheel {
                    x,
                    y,
                    delta: delta_y / self.ch,
                    shift,
                },
                ui::CanvasEvent::Pointer {
                    phase,
                    x,
                    y,
                    button,
                    clicks,
                    shift,
                } => Event::Pointer {
                    kind: match phase {
                        ui::PointerPhase::Down => "down",
                        ui::PointerPhase::Move => "move",
                        ui::PointerPhase::Up => "up",
                    }
                    .into(),
                    x,
                    y,
                    button,
                    clicks,
                    shift,
                },
                _ => return,
            },
            api::Notification::Ui(event) => Event::Ui(event),
            api::Notification::Request { handle, update } => {
                self.editor_completion(handle, update);
                return;
            }
            // Host focus/layout notifications do not manufacture sessions; panel.opened is an explicit command.
            _ => return,
        };
        self.event(event);
    }
}
