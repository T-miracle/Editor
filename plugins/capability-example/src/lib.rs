//! Independent guest exercising only public capability contracts and native text UI.
use plugin_protocol::{
    api::{self, ErrorCode, Failure},
    bindings::{Guest, export},
    ui,
};
use std::cell::RefCell;

#[derive(Default)]
struct State {
    text: String,
    optional_available: bool,
    /// Handles remain bound to this instance across workspace selection changes.
    workspace: Option<api::ResourceHandle>,
    data: Option<api::ResourceHandle>,
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
            api::Input::Prepare { api, .. } => {
                self.workspace = None;
                self.data = None;
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
            }
            api::Input::Snapshot => {
                return Ok(api::Output {
                    snapshot: Some(plugin_protocol::Snapshot::default()),
                    ..Default::default()
                });
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
            } if id == "scope-probe" => {
                // Return expected domain failures as data so the host can observe continued liveness.
                let operation = serde_json::from_value(arguments.unwrap_or_default())
                    .map_err(|error| Failure::new(ErrorCode::InvalidRequest, error.to_string()))?;
                self.text = serde_json::to_string(&api::guest::request(operation))
                    .map_err(|error| Failure::new(ErrorCode::OperationFailed, error.to_string()))?;
            }
            api::Input::Event { .. } => {}
        }
        Ok(api::Output {
            views: vec![api::View {
                panel: "welcome".into(),
                document: ui::Document::new(ui::Node::text("welcome-text", self.text.clone())),
            }],
            ..Default::default()
        })
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
