//! Independent guest exercising only public capability contracts and native text UI.
use plugin_protocol::{
    api::{self, ErrorCode, Failure},
    bindings::{Guest, export},
    ui,
};
use std::cell::RefCell;
mod composition;
mod service_demo;

#[derive(Default)]
struct State {
    service_client: service_demo::Client,
    /// Bounded observable process history demonstrates stream ordering through the public SDK.
    process_events: Vec<plugin_protocol::process::Update>,
    /// Demonstrate cancellation from inside an output callback, including an already queued exit.
    close_on_output: bool,
    /// Resolved configuration arrives before activation and carries source metadata for each field.
    configuration: plugin_protocol::settings::Effective,
    text: String,
    optional_available: bool,
    /// Handles remain bound to this instance across workspace selection changes.
    workspace: Option<api::ResourceHandle>,
    data: Option<api::ResourceHandle>,
    /// Only the latest user intent may replace request output in this view.
    task: Option<api::guest::EditorTask>,
    document: Option<api::DocumentVersion>,
    subscription: Option<api::ResourceHandle>,
    /// Preview versions are echoed independently of command results or the panel's own UI revision.
    preview: Option<api::DocumentVersion>,
    /// The example's composition state is owned entirely by the guest.
    ui_demo: Option<composition::Demo>,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
struct Example;

impl Guest for Example {
    /// SDK dispatch handles IDs and encoding; the plugin sees typed lifecycle inputs only.
    fn dispatch(payload: String) -> Result<String, String> {
        api::guest::dispatch(&payload, |input| {
            STATE.with(|state| state.borrow_mut().handle(input))
        })
    }
}
export!(Example);

impl State {
    /// Preparation is side-effect free; missing optional functionality selects a visible fallback.
    fn handle(&mut self, input: api::Input) -> Result<api::Output, Failure> {
        match input {
            api::Input::Event {
                event:
                    api::Notification::Service(plugin_protocol::service::Notification::Invoke(call)),
                ..
            } => {
                return Ok(api::Output {
                    service_reply: Some(self.service_client.provide(call)),
                    ..Default::default()
                });
            }
            api::Input::Event {
                event: api::Notification::Service(event),
                ..
            } => {
                if let Some(text) = self.service_client.update(&event) {
                    self.text = text;
                }
            }
            api::Input::Event {
                event: api::Notification::Command { id, arguments },
                ..
            } if id.starts_with("service-") => {
                self.text = self
                    .service_client
                    .command(&id, arguments)
                    .unwrap_or_else(|error| format!("{error}"));
            }
            api::Input::Event {
                event: api::Notification::LanguageService(context),
                ..
            } => {
                // The SDK guest supplies opaque initialization data without host language branches.
                let label = context
                    .settings
                    .get("label")
                    .map(|value| value.value.clone())
                    .unwrap_or_default();
                if label == "emit-ui" {
                    // Negative fixture: the host must reject this before touching the current panel.
                    return Ok(api::Output {
                        views: vec![api::View {
                            panel: "welcome".into(),
                            document: ui::Document::new(ui::Node::text(
                                "forbidden",
                                "forbidden hook view",
                            )),
                        }],
                        language_service: Some(Default::default()),
                        ..Default::default()
                    });
                }
                let dynamic = label == "dynamic-start";
                // Resolve a data-only plan through the public asset API; the host owns all preparation work.
                let installation =
                    if label == "managed-dependency" || label == "managed-dependency-alternate" {
                        let path = if label == "managed-dependency-alternate" {
                            "dependency-plan-alternate.json"
                        } else {
                            "dependency-plan.json"
                        };
                        let api::Value::Asset { bytes } =
                            api::guest::request(api::Operation::ReadAsset { path: path.into() })?
                        else {
                            return Err(Failure::new(
                                ErrorCode::InvalidRequest,
                                "Expected dependency plan asset",
                            ));
                        };
                        Some(serde_json::from_slice(&bytes).map_err(|error| {
                            Failure::new(ErrorCode::InvalidRequest, error.to_string())
                        })?)
                    } else {
                        None
                    };
                let candidate = context.candidates.values().next().unwrap();
                let mut args = candidate.args.clone();
                args.push("--hook-selected".into());
                return Ok(api::Output {
                    language_service: Some(plugin_protocol::language::Proposal {
                        installation,
                        program: dynamic.then(|| candidate.program.clone()),
                        args: dynamic.then_some(args),
                        project_root: (label == "escaped-project").then(|| "../outside".into()),
                        initialization_options: Some(
                            serde_json::json!({"sdkHook":true,"label":label}),
                        ),
                        configuration: Some(std::collections::BTreeMap::from([(
                            "fixture.analysis".into(),
                            serde_json::json!({"enabled":true}),
                        )])),
                        ..Default::default()
                    }),
                    ..Default::default()
                });
            }
            api::Input::Prepare { api, .. } => {
                self.configuration.clear();
                self.service_client = Default::default();
                self.process_events.clear();
                self.close_on_output = false;
                self.workspace = None;
                self.data = None;
                self.task = None;
                self.document = None;
                self.subscription = None;
                self.preview = None;
                self.ui_demo = None;
                self.optional_available = api.capabilities.contains_key("example.future");
                return Ok(api::Output::default());
            }
            api::Input::Activate => {
                self.text = String::from_utf8(api::guest::read_asset("welcome.txt")?)
                    .map_err(|error| Failure::new(ErrorCode::OperationFailed, error.to_string()))?;
                if !self.optional_available {
                    self.text
                        .push_str("Optional feature unavailable; native fallback active.");
                }
                if let Some(enabled) = self.configuration.get("enabled") {
                    self.text.push_str(&format!(" enabled={}", enabled.value));
                }
            }
            api::Input::Snapshot => {
                return Ok(api::Output {
                    snapshot: Some(plugin_protocol::Snapshot::default()),
                    ..Default::default()
                });
            }
            api::Input::Event {
                event: api::Notification::Configuration { phase, values },
                ..
            } => {
                use plugin_protocol::settings::{Phase, Proposal, Source};
                match phase {
                    Phase::Validate => {
                        let mut proposal = Proposal::default();
                        if values.get("label").is_some_and(|value| {
                            matches!(value.source, Source::User | Source::Project)
                                && value.value == "invalid"
                        }) {
                            proposal.errors.insert(
                                "label".into(),
                                "This label is not accepted by the plugin".into(),
                            );
                        }
                        proposal
                            .discovered
                            .insert("label".into(), serde_json::json!("Discovered label"));
                        return Ok(api::Output {
                            configuration: Some(proposal),
                            ..Default::default()
                        });
                    }
                    Phase::Apply => self.configuration = values,
                }
            }
            api::Input::Event {
                event: api::Notification::Process { handle, update },
                ..
            } => {
                if self.close_on_output
                    && matches!(update, plugin_protocol::process::Update::Output { .. })
                {
                    api::guest::close_resource(handle)?;
                }
                if self.process_events.len() < 256 {
                    self.process_events.push(update);
                }
            }
            api::Input::Event {
                event: api::Notification::Command { id, .. },
                ..
            } if id == "close-on-output" => {
                self.close_on_output = true;
            }
            api::Input::Event {
                event: api::Notification::Command { id, arguments },
                ..
            } if id == "process-events" => {
                // Read one bounded event at a time; native text controls are not bulk byte storage.
                let index = arguments
                    .as_ref()
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0) as usize;
                self.text = serde_json::to_string(&self.process_events.get(index))
                    .map_err(|error| Failure::new(ErrorCode::OperationFailed, error.to_string()))?;
            }
            api::Input::Event {
                event: api::Notification::Command { id, .. },
                ..
            } if id == "check-errors" => {
                self.check_errors()?;
                self.text = "Typed errors and request IDs verified.".into();
            }
            api::Input::Event {
                event: api::Notification::Command { id, arguments },
                ..
            } if id == "scope-write" || id == "scope-read" => {
                if self.data.is_none() {
                    self.data = Some(api::guest::open_data()?);
                }
                if self.workspace.is_none() {
                    self.workspace = Some(api::guest::open_workspace()?);
                }
                let data = self.data.as_ref().unwrap();
                if id == "scope-write" {
                    let text = arguments
                        .as_ref()
                        .and_then(|args| args.get("text"))
                        .and_then(|text| text.as_str())
                        .unwrap_or_default();
                    api::guest::write_file(data, "value.txt", text.as_bytes().to_vec())?;
                }
                let workspace =
                    api::guest::read_file(self.workspace.as_ref().unwrap(), "source.txt")?;
                let private = api::guest::read_file(data, "value.txt")?;
                self.text = format!(
                    "{}|{}",
                    String::from_utf8_lossy(&workspace),
                    String::from_utf8_lossy(&private)
                );
            }
            api::Input::Event {
                event: api::Notification::Command { id, arguments },
                ..
            } if id == "ui-layout" => {
                if let Some(demo) = &mut self.ui_demo {
                    demo.layout(
                        arguments
                            .as_ref()
                            .and_then(|args| args.as_str())
                            .unwrap_or("combined"),
                    );
                }
            }
            api::Input::Event {
                event: api::Notification::Command { id, arguments },
                ..
            } if id == "scope-probe" => {
                // Return expected domain failures as data so the host can observe continued liveness.
                let operation = serde_json::from_value(arguments.unwrap_or_default())
                    .map_err(|error| Failure::new(ErrorCode::InvalidRequest, error.to_string()))?;
                if matches!(operation, api::Operation::Editor { .. }) {
                    self.task = None;
                }
                let result = api::guest::request(operation);
                if let Ok(api::Value::Accepted(handle)) = &result {
                    self.task = Some(api::guest::EditorTask::from_accepted(handle.clone()));
                }
                self.text = serde_json::to_string(&result)
                    .map_err(|error| Failure::new(ErrorCode::OperationFailed, error.to_string()))?;
            }
            api::Input::Event {
                event: api::Notification::Command { id, .. },
                ..
            } => {
                // Domain failures remain visible in the panel instead of trapping the WASM instance.
                if let Err(error) = self.editor_command(&id) {
                    self.text = format!("{error:?}");
                }
            }
            api::Input::Event {
                event: event @ api::Notification::Request { .. },
                ..
            } => {
                if let Some(text) = self.service_client.editor_update(&event) {
                    self.text = text;
                }
                if let Some(update) = self.task.as_mut().and_then(|task| task.update(&event)) {
                    if let api::RequestUpdate::Completed {
                        result: Ok(api::EditorValue::Selection { document, .. }),
                    } = &update
                    {
                        self.document = Some(document.clone());
                    }
                    self.text = format!("{update:?}");
                }
            }
            api::Input::Event {
                event:
                    event @ (api::Notification::Document { .. }
                    | api::Notification::SubscriptionFailed { .. }),
                ..
            } => {
                // Overflow destroys the host subscription; the next explicit subscribe must create a new one.
                if let api::Notification::SubscriptionFailed { subscription, .. } = &event {
                    if self.subscription.as_ref() == Some(subscription) {
                        self.subscription = None;
                    }
                }
                self.text = format!("{event:?}");
            }
            api::Input::Event {
                event: api::Notification::Theme(environment),
                ..
            } => {
                if let Some(demo) = &mut self.ui_demo {
                    demo.theme(&environment);
                }
            }
            api::Input::Event {
                event: api::Notification::Ui(event),
                ..
            } => {
                if let Some(demo) = &mut self.ui_demo {
                    demo.event(&event);
                }
            }
            api::Input::Event {
                event: api::Notification::Preview { document, text },
                ..
            } => {
                if let Some(demo) = &mut self.ui_demo {
                    demo.preview(document.clone(), &text);
                }
                self.preview = document;
                self.text = text;
            }
            api::Input::Event { .. } => {}
        }
        // The independently built fixture reads a portable tree asset; the host never interprets its label.
        let mut document = if self
            .configuration
            .get("label")
            .is_some_and(|value| value.value == "composable-ui")
        {
            if self.ui_demo.is_none() {
                self.ui_demo = Some(composition::Demo::load()?);
            }
            self.ui_demo.as_ref().unwrap().document()
        } else {
            ui::Document::new(ui::Node::text("welcome-text", self.text.clone()))
        };
        document.source = self.preview.clone();
        Ok(api::Output {
            views: vec![api::View {
                panel: "welcome".into(),
                document,
            }],
            ..Default::default()
        })
    }

    /// These commands use only the published SDK; saving requires the version returned by selection.
    fn editor_command(&mut self, id: &str) -> Result<(), Failure> {
        use api::EditorOperation as Op;
        // Even a rejected new intent supersedes old output; its failure must not be replaced by late success.
        if matches!(
            id,
            "read-selection" | "active-directory" | "save-document" | "hide-panel" | "show-panel"
        ) {
            self.task = None;
        }
        let operation = match id {
            "read-selection" => Op::ReadSelection,
            "active-directory" => Op::ActiveDirectory,
            "save-document" => Op::SaveDocument {
                document: self.document.clone().ok_or_else(|| {
                    Failure::new(
                        ErrorCode::InvalidState,
                        "Read selection first to capture a document version",
                    )
                })?,
            },
            "hide-panel" | "show-panel" => Op::SetPanelVisibility {
                panel: "welcome".into(),
                visible: id == "show-panel",
            },
            "cancel-request" => {
                self.text = format!(
                    "{:?}",
                    self.task
                        .as_ref()
                        .ok_or_else(|| Failure::new(ErrorCode::NotFound, "No pending task"))?
                        .cancel(api::CancelMode::TryTerminate)?
                );
                return Ok(());
            }
            "subscribe-documents" => {
                if self.subscription.is_none() {
                    self.subscription = Some(api::guest::subscribe_documents()?);
                }
                self.text = "Document subscription active".into();
                return Ok(());
            }
            "unsubscribe-documents" => {
                if let Some(handle) = self.subscription.take() {
                    api::guest::close_resource(handle)?;
                }
                self.text = "Document subscription released".into();
                return Ok(());
            }
            _ => return Ok(()),
        };
        self.task = Some(api::guest::EditorTask::start(operation, 30_000)?);
        self.text = "Accepted: waiting for editor".into();
        Ok(())
    }

    /// An explicit diagnostic command demonstrates malformed/unknown requests without SDK internals.
    fn check_errors(&self) -> Result<(), Failure> {
        use plugin_protocol::bindings::editor::plugin::host;
        // Oversized calls must retain the quota category rather than fail response correlation.
        let oversized = api::guest::read_asset("x".repeat(2 * 1024 * 1024 + 1));
        if !matches!(oversized, Err(error) if error.code == ErrorCode::LimitExceeded) {
            return Err(Failure::new(
                ErrorCode::OperationFailed,
                "SDK lost the request quota failure",
            ));
        }
        for (id, operation, expected) in [
            (
                101,
                serde_json::json!({"method":"unknown_method"}),
                ErrorCode::UnsupportedOperation,
            ),
            (
                102,
                serde_json::json!({"method":"read_asset", "path":42}),
                ErrorCode::InvalidRequest,
            ),
            (
                103,
                serde_json::json!({"method":"read_asset", "path":"../outside.txt"}),
                ErrorCode::InvalidPath,
            ),
        ] {
            let payload = serde_json::json!({"id":id,"operation":operation}).to_string();
            let reply = host::request(&payload)
                .map_err(|error| Failure::new(ErrorCode::OperationFailed, error))?;
            let response: api::Response = serde_json::from_str(&reply)
                .map_err(|error| Failure::new(ErrorCode::InvalidRequest, error.to_string()))?;
            if response.id != id || !matches!(response.result, Err(error) if error.code == expected)
            {
                return Err(Failure::new(
                    ErrorCode::OperationFailed,
                    "Unexpected host error or request ID",
                ));
            }
        }
        Ok(())
    }
}
