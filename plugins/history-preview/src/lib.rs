//! History comparison uses only public SDK documents, readonly resources and native diff.
use plugin_protocol::{
    api,
    bindings::{Guest, export},
    ui,
};
use std::cell::RefCell;

thread_local! {
    /// Domain intent retains immutable version references, never a second mutable document.
    static STATE: RefCell<HistoryState> = RefCell::new(HistoryState::default());
}

/// Each completion advances only the task that belongs to the latest command.
#[derive(Default)]
enum Stage {
    #[default]
    Idle,
    Select,
    Open {
        current: api::DocumentVersion,
    },
    Locate {
        current: api::DocumentVersion,
        previous: api::DocumentVersion,
    },
    Compare,
}

/// Provider-owned text is fixed fixture history; the host retains the live readonly value.
#[derive(Default)]
struct HistoryState {
    english: bool,
    text: String,
    task: Option<api::guest::EditorTask>,
    stage: Stage,
    previous: Option<api::DocumentInfo>,
    events: Vec<api::Notification>,
    refreshed: bool,
}

impl HistoryState {
    /// Correlation replacement stops waiting for older UI intent without claiming rollback.
    fn start(&mut self, operation: api::EditorOperation, stage: Stage) -> Result<(), api::Failure> {
        if let Some(task) = self.task.take() {
            let _ = task.cancel(api::CancelMode::StopWaiting);
        }
        self.task = Some(api::guest::EditorTask::start(operation, 30_000)?);
        self.stage = stage;
        Ok(())
    }

    /// The selected local session is captured before opening a virtual tab changes active focus.
    fn advance(&mut self, value: api::EditorValue) -> Result<(), api::Failure> {
        match (std::mem::replace(&mut self.stage, Stage::Idle), value) {
            (Stage::Select, api::EditorValue::Documents { documents, active }) => {
                let current = documents
                    .iter()
                    .find(|info| {
                        active.as_ref() == Some(&info.document)
                            && matches!(info.resource, api::ResourceIdentity::Local { .. })
                    })
                    .or_else(|| {
                        documents.iter().find(|info| {
                            matches!(info.resource, api::ResourceIdentity::Local { .. })
                        })
                    })
                    .ok_or_else(|| {
                        api::Failure::new(
                            api::ErrorCode::NotFound,
                            "Open a local text document first",
                        )
                    })?
                    .document
                    .clone();
                let text = if self.refreshed {
                    "历史版本已刷新😀\r\nprevious line\r\n"
                } else {
                    "历史版本😀\r\nprevious line\r\n"
                }
                .to_string();
                let operation = self
                    .previous
                    .as_ref()
                    .map(|info| api::EditorOperation::RefreshVirtualDocument {
                        document: info.document.clone(),
                        text: text.clone(),
                    })
                    .unwrap_or(api::EditorOperation::OpenVirtualDocument {
                        title: if self.english {
                            "Historical snapshot"
                        } else {
                            "历史快照"
                        }
                        .into(),
                        language: Some("text".into()),
                        text,
                    });
                self.start(operation, Stage::Open { current })?;
            }
            (Stage::Open { current }, api::EditorValue::DocumentOpened(info)) => {
                let previous = info.document.clone();
                self.previous = Some(info);
                self.start(
                    api::EditorOperation::LocateDocument {
                        document: previous.clone(),
                        position: api::TextPosition {
                            line: 0,
                            character: 0,
                        },
                    },
                    Stage::Locate { current, previous },
                )?;
            }
            (Stage::Locate { current, previous }, api::EditorValue::DocumentLocated { .. }) => {
                self.start(
                    api::EditorOperation::CompareDocuments {
                        left: previous,
                        right: current,
                    },
                    Stage::Compare,
                )?;
            }
            (Stage::Compare, api::EditorValue::DocumentsCompared { .. }) => {
                self.text = if self.english {
                    "History comparison is open. The historical pane is read-only."
                } else {
                    "历史比较已打开；历史内容为只读。可再次执行刷新历史命令。"
                }
                .into();
            }
            _ => {
                return Err(api::Failure::new(
                    api::ErrorCode::InvalidState,
                    "Unexpected document task result",
                ));
            }
        }
        Ok(())
    }

    /// Diagnostic probes enter the identical transport used by the real menu commands.
    fn input(&mut self, input: api::Input) -> Result<(), api::Failure> {
        match input {
            api::Input::Prepare { environment, .. } => {
                self.english = environment.locale.starts_with("en")
            }
            api::Input::Activate => {
                let _ = api::guest::subscribe_document_events()?;
            }
            api::Input::Event {
                event: api::Notification::Command { id, arguments },
                ..
            } => match id.as_str() {
                "probe" => {
                    let result = arguments
                        .ok_or_else(|| {
                            api::Failure::new(
                                api::ErrorCode::InvalidRequest,
                                "Probe requires an operation",
                            )
                        })
                        .and_then(|arguments| {
                            serde_json::from_value::<api::Operation>(arguments).map_err(|error| {
                                api::Failure::new(api::ErrorCode::InvalidRequest, error.to_string())
                            })
                        })
                        .and_then(api::guest::request);
                    self.text = serde_json::to_string(&result).unwrap();
                }
                "events" => self.text = serde_json::to_string(&self.events).unwrap(),
                "compare-history" | "refresh-history" => {
                    self.refreshed = id == "refresh-history";
                    self.start(api::EditorOperation::ListDocuments, Stage::Select)?;
                }
                _ => {}
            },
            api::Input::Event { event, .. } => {
                // A bounded diagnostic log makes observable events inspectable through the native panel.
                self.events.push(event.clone());
                if self.events.len() > 32 {
                    self.events.remove(0);
                }
                // Diagnostic UI has its own byte budget; text-bearing request completions
                // are consumed by tasks, never accumulated as a second document transcript.
                self.events.retain(|event| {
                    matches!(
                        event,
                        api::Notification::Document { .. }
                            | api::Notification::DocumentEvent { .. }
                            | api::Notification::SubscriptionFailed { .. }
                    )
                });
                while serde_json::to_vec(&self.events).unwrap().len() > 24 * 1024 {
                    self.events.remove(0);
                }
                if let api::Notification::DocumentEvent {
                    event:
                        api::DocumentEvent {
                            kind: api::DocumentEventKind::Closed(document),
                            ..
                        },
                    ..
                } = &event
                {
                    if self
                        .previous
                        .as_ref()
                        .is_some_and(|info| info.document.id == document.id)
                    {
                        self.previous = None;
                    }
                }
                match self.task.as_mut().and_then(|task| task.update(&event)) {
                    Some(api::RequestUpdate::Completed { result }) => {
                        self.task = None;
                        if let Err(error) = result.and_then(|value| self.advance(value)) {
                            self.previous = None;
                            self.text = error.to_string();
                        }
                    }
                    Some(api::RequestUpdate::Cancelled { reason, .. }) => {
                        self.task = None;
                        self.text = format!("{reason:?}");
                    }
                    _ if self.task.is_none() => {
                        let diagnostic = serde_json::to_string(&event).unwrap();
                        self.text = if diagnostic.len() <= 24 * 1024 {
                            diagnostic
                        } else {
                            "Completed result exceeds the diagnostic display budget".into()
                        };
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }
}

struct History;
impl Guest for History {
    /// Failed user commands remain visible without retiring the instance and its valid resources.
    fn dispatch(payload: String) -> Result<String, String> {
        api::guest::dispatch(&payload, |input| {
            STATE.with(|state| {
                let mut state = state.borrow_mut();
                if let Err(error) = state.input(input) {
                    state.text = error.to_string();
                }
                Ok(api::Output {
                    snapshot: Some(Default::default()),
                    views: vec![api::View {
                        panel: "history".into(),
                        document: ui::Document::new(ui::Node::text(
                            "history-result",
                            state.text.clone(),
                        )),
                    }],
                    ..Default::default()
                })
            })
        })
    }
}
export!(History);
