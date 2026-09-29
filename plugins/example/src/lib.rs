//! Two independent dock panels prove that the host does not special-case terminals.
use plugin_protocol::*;
mod views;
use std::cell::RefCell;
use plugin_protocol::bindings::{Guest, editor, export};
struct Example;
#[derive(Default)]
struct State {
    count: u64,
    note: String,
    editing: bool,
    revision: u64,
    checked: bool,
    selected: Option<String>,
    tab: String,
    env: Environment,
}
thread_local! {static STATE:RefCell<State>=RefCell::new(State::default());}
impl Guest for Example {
    /// This package has no native permissions; it uses only UI events and opaque snapshots.
    fn dispatch(payload: String) -> Result<String, String> {
        let message: Message = serde_json::from_str(&payload).map_err(|e| e.to_string())?;
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            let mut reply = Reply::default();
            match message {
                Message::Prepare {
                    environment,
                    snapshot,
                } => {
                    state.env = environment;
                    if let Some(snapshot) = snapshot {
                        if snapshot.schema != 7 {
                            return Err("Example requires snapshot schema 7".into());
                        }
                        let (count, note) =
                            serde_json::from_str(&snapshot.data).map_err(|e| e.to_string())?;
                        state.count = count;
                        state.note = note;
                    }
                }
                Message::Snapshot => {
                    reply.snapshot = Some(Snapshot {
                        schema: 7,
                        data: serde_json::to_string(&(state.count, &state.note)).unwrap(),
                    })
                }
                Message::Event(event) => state.event(event),
                Message::Activate => {
                    // Optional fixture asset exercises host rollback after successful preparation.
                    let request = serde_json::to_string(&Request::ReadAsset {
                        path: "activation-policy.txt".into(),
                    })
                    .unwrap();
                    if let Ok(value) = editor::plugin::host::request(&request) {
                        if let Ok(bytes) = serde_json::from_str::<Vec<u8>>(&value) {
                            if bytes == b"reject" {
                                return Err("Example activation rejected by package policy".into());
                            }
                        }
                    }
                }
            }
            reply.scenes = vec![state.scene("counter"), state.scene("notes")];
            serde_json::to_string(&reply).map_err(|e| e.to_string())
        })
    }
}
export!(Example);
impl State {
    fn event(&mut self, event: Event) {
        self.revision += 1;
        match event {
            Event::Surface { event, .. } => self.event(*event),
            Event::Theme(env) => self.env = env,
            Event::Command { id, .. } if id == "increment" => self.count += 1,
            Event::Command { id, .. } if id == "edit" => self.editing = true,
            Event::Ui(event) => match (event.node.as_str(), event.action) {
                ("increment", ui::Action::Click) => self.count += 1,
                ("note", ui::Action::Change(value) | ui::Action::Submit(value)) => {
                    self.note = value
                }
                ("enabled", ui::Action::Toggle(value)) => self.checked = value,
                ("mode", ui::Action::Select(value)) => self.selected = Some(value),
                ("pages", ui::Action::Select(value)) => self.tab = value,
                ("open-dialog", ui::Action::Click) => self.editing = true,
                ("sample-dialog", ui::Action::Dismiss) | ("close-dialog", ui::Action::Click) => {
                    self.editing = false
                }
                _ => {}
            },
            Event::Edit { text, .. } => {
                self.note = text;
                self.editing = false;
            }
            _ => {}
        }
    }
}
