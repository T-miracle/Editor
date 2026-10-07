//! Independent Rust debugging policy over CodeLLDB/DAP; the host sees only debug.session messages.
mod session;
mod transport;
use plugin_protocol::{
    api::{self, ErrorCode, Failure},
    bindings::{Guest, export},
    service, ui,
};
use session::Session;
use std::{cell::RefCell, collections::BTreeMap};

#[derive(Default)]
struct Debugger {
    sessions: BTreeMap<String, Session>,
    next: u64,
    revision: u64,
    /// Selection belongs to the native output view; old histories cannot consume another target's budget.
    selected: Option<String>,
}
thread_local! { static STATE:RefCell<Debugger> = RefCell::new(Debugger::default()); }
struct Plugin;
impl Guest for Plugin {
    /// Host dispatch serializes callbacks; native updates enter with their original resource authority.
    fn dispatch(payload: String) -> Result<String, String> {
        api::guest::dispatch(&payload, |input| {
            STATE.with(|state| state.borrow_mut().dispatch(input))
        })
    }
}
export!(Plugin);

impl Debugger {
    /// Route only exact source-owned session IDs. Installation and preparation never launch adapters.
    fn dispatch(&mut self, input: api::Input) -> Result<api::Output, Failure> {
        let mut reply = None;
        match input {
            api::Input::Event {
                event: api::Notification::Service(service::Notification::Invoke(call)),
                ..
            } => {
                let result = if call.method == "start" {
                    self.start(call)
                } else {
                    let id = call.arguments["session"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned();
                    self.sessions
                        .get_mut(&id)
                        .filter(|session| session.owner == call.caller.instance)
                        .ok_or_else(|| {
                            Failure::new(
                                ErrorCode::InvalidHandle,
                                "Unknown or foreign debug session",
                            )
                        })
                        .and_then(|session| session.call(call))
                };
                reply = match result {
                    Ok(value) => value.map(Ok),
                    Err(error) => Some(Err(error)),
                };
            }
            api::Input::Event {
                event: api::Notification::Process { handle, update },
                ..
            } => {
                if let Some(session) = self
                    .sessions
                    .values_mut()
                    .find(|session| session.handle == handle)
                {
                    session.update(update);
                }
            }
            api::Input::Event {
                event:
                    api::Notification::Service(service::Notification::InvocationCancelled {
                        request,
                        ..
                    }),
                ..
            } => {
                // Cancelling a receipt stops that wait. A created target remains owned and stoppable.
                for session in self.sessions.values_mut() {
                    session.cancel_reply(&request);
                }
            }
            api::Input::Event {
                event: api::Notification::Ui(event),
                ..
            } if event.node == "debug-sessions" => {
                if let ui::Action::Select(id) = event.action
                    && self.sessions.contains_key(&id)
                {
                    self.selected = Some(id);
                }
            }
            _ => {}
        }
        self.revision = self.revision.saturating_add(1);
        // The provider's output is ordinary native text; inspection uses separate typed debug replies.
        let text = self
            .selected
            .as_ref()
            .and_then(|id| self.sessions.get(id))
            .map(Session::transcript)
            .unwrap_or_default();
        let choices = self
            .sessions
            .iter()
            .map(|(id, session)| ui::OptionItem::new(id, session.caption()))
            .collect();
        Ok(api::Output {
            service_reply: reply,
            views: vec![api::View {
                panel: "debug-output".into(),
                document: ui::Document::new(
                    ui::Node::column(
                        "debug-output-root",
                        vec![
                            ui::Node::new(
                                "debug-sessions",
                                ui::Kind::Choice {
                                    options: choices,
                                    selected: self.selected.clone(),
                                },
                            ),
                            ui::Node::new(
                                "debug-scroll",
                                ui::Kind::Scroll {
                                    content: Box::new(ui::Node::text("debug-text", text)),
                                },
                            )
                            .grow(),
                        ],
                    )
                    .gap(6.)
                    .grow(),
                )
                .revision(self.revision),
            }],
            // Logical snapshot excludes native handles and targets; recovery must never replay them.
            snapshot: Some(plugin_protocol::Snapshot {
                schema: 1,
                data: "{}".into(),
            }),
            ..Default::default()
        })
    }
    /// A target owns one bridge/adapter job. Finished histories may be evicted; active jobs may not.
    fn start(&mut self, call: service::Invocation) -> Result<Option<serde_json::Value>, Failure> {
        while self.sessions.len() >= 32 {
            let Some(id) = self
                .sessions
                .iter()
                .find(|(_, session)| session.ended())
                .map(|(id, _)| id.clone())
            else {
                break;
            };
            self.sessions.remove(&id);
        }
        if self.sessions.len() >= 32 {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Debug session quota exceeded",
            ));
        }
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| transport::failure("Debug identity exhausted"))?;
        let id = self.next.to_string();
        let session = Session::start(id.clone(), call)?;
        self.selected = Some(id.clone());
        self.sessions.insert(id, session);
        Ok(None)
    }
}
