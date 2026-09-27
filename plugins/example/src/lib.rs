//! Two independent dock panels prove that the host does not special-case terminals.
use plugin_protocol::*;
use std::cell::RefCell;
wit_bindgen::generate!({path:"../../crates/plugin-protocol/wit",world:"plugin"});
struct Example;
#[derive(Default)]
struct State {
    count: u64,
    note: String,
    editing: bool,
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
        match event {
            Event::Surface { event, .. } => self.event(*event),
            Event::Theme(env) => self.env = env,
            Event::Command { id, .. } if id == "increment" => self.count += 1,
            Event::Command { id, .. } if id == "edit" => self.editing = true,
            Event::Edit { text, .. } => {
                self.note = text;
                self.editing = false;
            }
            _ => {}
        }
    }
    /// UI consists of ordinary native text/button/input widgets, with plugin-owned state.
    fn scene(&self, panel: &str) -> Scene {
        let counter = panel == "counter";
        Scene {
            panel: panel.into(),
            font: "Segoe UI".into(),
            font_size: 14.,
            paint: vec![Paint::Text {
                x: 12.,
                y: 12.,
                text: if counter {
                    format!("点击次数：{}", self.count)
                } else {
                    format!("笔记：{}", self.note)
                },
                color: self.env.foreground,
                size: 14.,
                bold: false,
            }],
            widgets: vec![Widget {
                id: if counter { "increment" } else { "edit" }.into(),
                rect: Rect {
                    x: 12.,
                    y: 48.,
                    w: 220.,
                    h: 30.,
                },
                label: if counter {
                    "增加一次".into()
                } else if self.editing {
                    self.note.clone()
                } else {
                    "编辑笔记".into()
                },
                edit: !counter && self.editing,
            }],
            ..Scene::default()
        }
    }
}
