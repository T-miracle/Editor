//! Generated text is an independent document consumer, built against the exported SDK.
use plugin_protocol::{
    api,
    bindings::{Guest, export},
    ui,
};
use std::cell::RefCell;

thread_local! {
    /// Only version references and transient UI tasks are retained; live text belongs to the host.
    static STATE: RefCell<Generator> = RefCell::new(Generator::default());
}

/// Completion stages are business intent, not a mutable editor model.
#[derive(Default)]
enum Stage {
    #[default]
    Idle,
    Select,
    Read(api::DocumentVersion),
    Preview(api::DocumentVersion),
    Locate {
        current: api::DocumentVersion,
        proposal: api::DocumentVersion,
    },
    Compare,
}

/// A generated proposal can be refreshed and navigated through the same public resource capability.
#[derive(Default)]
struct Generator {
    english: bool,
    text: String,
    task: Option<api::guest::EditorTask>,
    stage: Stage,
    proposal: Option<api::DocumentInfo>,
}
impl Generator {
    /// Replace older correlation so delayed callbacks cannot choose the new preview target.
    fn start(&mut self, operation: api::EditorOperation, stage: Stage) -> Result<(), api::Failure> {
        if let Some(task) = self.task.take() {
            let _ = task.cancel(api::CancelMode::StopWaiting);
        }
        self.task = Some(api::guest::EditorTask::start(operation, 30_000)?);
        self.stage = stage;
        Ok(())
    }

    /// This consumer derives its proposal from the current unsaved snapshot instead of fixed history.
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
                self.start(
                    api::EditorOperation::ReadDocument {
                        document: current.clone(),
                        range: None,
                    },
                    Stage::Read(current),
                )?;
            }
            (Stage::Read(current), api::EditorValue::DocumentSnapshot(snapshot)) => {
                // Transformation is plugin policy. The host provides only bounded readonly mechanisms.
                let text = format!(
                    "{}\n{}",
                    if self.english {
                        "Generated proposal"
                    } else {
                        "生成的建议内容"
                    },
                    snapshot.text.to_uppercase()
                );
                let operation = self
                    .proposal
                    .as_ref()
                    .map(|info| api::EditorOperation::RefreshVirtualDocument {
                        document: info.document.clone(),
                        text: text.clone(),
                    })
                    .unwrap_or(api::EditorOperation::OpenVirtualDocument {
                        title: if self.english {
                            "Generated preview"
                        } else {
                            "生成文本预览"
                        }
                        .into(),
                        language: Some(snapshot.info.language),
                        text,
                    });
                self.start(operation, Stage::Preview(current))?;
            }
            (Stage::Preview(current), api::EditorValue::DocumentOpened(info)) => {
                let proposal = info.document.clone();
                self.proposal = Some(info);
                self.start(
                    api::EditorOperation::LocateDocument {
                        document: proposal.clone(),
                        position: api::TextPosition {
                            line: 0,
                            character: 0,
                        },
                    },
                    Stage::Locate { current, proposal },
                )?;
            }
            (Stage::Locate { current, proposal }, api::EditorValue::DocumentLocated { .. }) => {
                self.start(
                    api::EditorOperation::CompareDocuments {
                        left: proposal,
                        right: current,
                    },
                    Stage::Compare,
                )?;
            }
            (Stage::Compare, api::EditorValue::DocumentsCompared { .. }) => {
                self.text = if self.english {
                    "Generated comparison is open. Run again to refresh."
                } else {
                    "生成内容比较已打开；再次运行可刷新只读预览。"
                }
                .into()
            }
            _ => {
                return Err(api::Failure::new(
                    api::ErrorCode::InvalidState,
                    "Unexpected generation task result",
                ));
            }
        }
        Ok(())
    }

    /// Commands and events use the standard manifest/SDK entry points, without host ID branches.
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
            } if id == "probe" => {
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
            api::Input::Event {
                event: api::Notification::Command { id, .. },
                ..
            } if id == "preview-generated" => {
                self.start(api::EditorOperation::ListDocuments, Stage::Select)?;
            }
            api::Input::Event { event, .. } => {
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
                        .proposal
                        .as_ref()
                        .is_some_and(|info| info.document.id == document.id)
                    {
                        self.proposal = None;
                    }
                }
                if let Some(update) = self.task.as_mut().and_then(|task| task.update(&event)) {
                    match update {
                        api::RequestUpdate::Completed { result } => {
                            self.task = None;
                            if let Err(error) = result.and_then(|value| self.advance(value)) {
                                self.proposal = None;
                                self.text = error.to_string();
                            }
                        }
                        api::RequestUpdate::Cancelled { reason, .. } => {
                            self.task = None;
                            self.text = format!("{reason:?}");
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
struct Generated;
impl Guest for Generated {
    /// A failed preview stays visible and retryable without tearing down unrelated plugin resources.
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
                        panel: "generated".into(),
                        document: ui::Document::new(ui::Node::text(
                            "generated-result",
                            state.text.clone(),
                        )),
                    }],
                    ..Default::default()
                })
            })
        })
    }
}
export!(Generated);
