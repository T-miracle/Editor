//! Independent native panels and an attributed guided check use only public SDK capabilities.
use plugin_protocol::bindings::{Guest, export};
use plugin_protocol::{Environment, Snapshot, api, ui};
use std::cell::RefCell;
mod interaction_demo;
mod views;
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
    /// Guided interaction is ephemeral; persisted notes never serialize active host request handles.
    interaction: interaction_demo::Demo,
}
thread_local! {static STATE:RefCell<State>=RefCell::new(State::default());}
impl Guest for Example {
    /// SDK lifecycle and opaque snapshots need no filesystem or process authority.
    fn dispatch(payload: String) -> Result<String, String> {
        api::guest::dispatch(&payload, |message| {
            STATE.with(|cell| {
                let mut state = cell.borrow_mut();
                match message {
                    api::Input::Prepare {
                        environment,
                        snapshot,
                        ..
                    } => {
                        *state = State {
                            env: environment,
                            ..Default::default()
                        };
                        if let Some(snapshot) = snapshot {
                            if snapshot.schema != 7 {
                                return Err(api::Failure::new(
                                    api::ErrorCode::InvalidRequest,
                                    "Example requires snapshot schema 7",
                                ));
                            }
                            let (count, note) =
                                serde_json::from_str(&snapshot.data).map_err(|error| {
                                    api::Failure::new(
                                        api::ErrorCode::InvalidRequest,
                                        error.to_string(),
                                    )
                                })?;
                            state.count = count;
                            state.note = note;
                        }
                    }
                    api::Input::Snapshot => {
                        return Ok(api::Output {
                            snapshot: Some(Snapshot {
                                schema: 7,
                                data: serde_json::to_string(&(state.count, &state.note)).unwrap(),
                            }),
                            ..Default::default()
                        });
                    }
                    api::Input::Event { panel, event } => state.event(panel.as_deref(), event),
                    api::Input::Activate => {}
                }
                Ok(api::Output {
                    views: vec![state.view("counter"), state.view("notes")],
                    ..Default::default()
                })
            })
        })
    }
}
export!(Example);
impl State {
    /// Value updates preserve input identity; only a changed target tree invalidates queued UI events.
    fn event(&mut self, panel: Option<&str>, event: api::Notification) {
        let identity = (self.editing, self.tab.clone());
        match event {
            event if self.interaction.handles(&event) => self.note = self.interaction.handle(event),
            api::Notification::Theme(env) => self.env = env,
            api::Notification::Command { id, .. } if id == "increment" => self.count += 1,
            api::Notification::Ui(event) => match (panel, event.node.as_str(), event.action) {
                (Some("counter"), "increment", ui::Action::Click) => self.count += 1,
                (Some("notes"), "note", ui::Action::Change(value) | ui::Action::Submit(value)) => {
                    self.note = value
                }
                (Some("counter"), "enabled", ui::Action::Toggle(value)) => self.checked = value,
                (Some("counter"), "mode", ui::Action::Select(value)) => self.selected = Some(value),
                (Some("counter"), "pages", ui::Action::Select(value)) => self.tab = value,
                (Some("counter"), "open-dialog", ui::Action::Click) => self.editing = true,
                (Some("counter"), "sample-dialog", ui::Action::Dismiss)
                | (Some("counter"), "close-dialog", ui::Action::Click) => self.editing = false,
                _ => {}
            },
            _ => {}
        }
        if identity != (self.editing, self.tab.clone()) {
            self.revision = self.revision.saturating_add(1);
        }
    }
}
