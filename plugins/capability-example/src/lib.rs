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
