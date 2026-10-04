//! The host's debug requirement is its own declaration, so its meaning is checkable on its own.
//!
//! These are unit checks of the shapes a provider must declare and of how the host reads what one
//! reports. They name no debugger: a provider that declares this contract is interchangeable.
use super::*;
use crate::manager::debug_services::{DEBUG_OPTIONAL_METHODS, DEBUG_REQUIRED_METHODS};

/// The host asks for exactly the methods a debug session needs, and nothing language specific.
#[test]
fn the_debug_requirement_names_no_debugger() {
    let dependency = debug_dependency().expect("the host's own requirement is valid");
    let methods = dependency.methods.keys().cloned().collect::<Vec<_>>();
    let mut declared = DEBUG_REQUIRED_METHODS.to_vec();
    declared.extend(DEBUG_OPTIONAL_METHODS);
    declared.sort_unstable();
    assert_eq!(
        methods, declared,
        "the host declares everything it may call, required and optional alike"
    );
    // Only the methods a session cannot exist without are required of every provider.
    assert_eq!(
        dependency
            .methods
            .keys()
            .filter(|method| DEBUG_REQUIRED_METHODS.contains(&method.as_str()))
            .count(),
        DEBUG_REQUIRED_METHODS.len()
    );
    // Every method the host may call has a window; one it does not call is not silently callable.
    for method in &methods {
        assert!(
            debug_timeout_ms(method).is_some(),
            "{method} has no window, so it could be called without a bound"
        );
    }
    assert_eq!(debug_timeout_ms("evaluate"), None);
    // Starting includes building and attaching; the control exchanges are much shorter.
    assert!(debug_timeout_ms("start").unwrap() > debug_timeout_ms("resume").unwrap());
    // The declaration is the host's, so a debugger name or an adapter protocol cannot appear in it.
    let rendered = serde_json::to_string(&dependency)
        .unwrap()
        .to_ascii_lowercase();
    for forbidden in ["lldb", "gdb", "codelldb", "dap", "msvc", "rust"] {
        assert!(
            !rendered.contains(forbidden),
            "the host's requirement must not name {forbidden}"
        );
    }
}

/// A state word this build does not implement is refused instead of being shown as something else.
#[test]
fn an_unknown_debug_state_is_not_mapped_onto_a_known_one() {
    for (reported, expected) in [
        ("starting", DebugState::Starting),
        ("Running", DebugState::Running),
        ("paused", DebugState::Paused),
        ("breakpoint", DebugState::Paused),
        ("exited", DebugState::Exited),
    ] {
        assert_eq!(DebugState::from_str(reported), Some(expected), "{reported}");
    }
    // A newer provider's vocabulary is unreadable here, which is reported rather than guessed.
    assert_eq!(DebugState::from_str("restarting"), None);
    let error = DebugSession::from_value(&serde_json::json!({
        "session": "7",
        "state": "restarting"
    }))
    .expect_err("an unknown state is refused");
    assert!(error.message.contains("state"), "{error:?}");
    // A missing session is refused too: the host never invents an identity for a provider's session.
    assert!(
        DebugSession::from_value(&serde_json::json!({"state": "paused"})).is_err(),
        "a session identity is required"
    );
}

/// A stop location is read as the provider reported it, including an unverified breakpoint.
#[test]
fn a_paused_session_carries_where_it_stopped() {
    let session = DebugSession::from_value(&serde_json::json!({
        "session": "9",
        "state": "paused",
        "reason": "breakpoint",
        "source": "src/main.rs",
        "line": 4
    }))
    .expect("a paused session reads");
    assert_eq!(session.state, DebugState::Paused);
    assert_eq!(session.reason.as_deref(), Some("breakpoint"));
    assert_eq!(session.source.as_deref(), Some("src/main.rs"));
    assert_eq!(session.line, Some(4));

    // A running session reports no location, and none is invented for it.
    let running = DebugSession::from_value(&serde_json::json!({
        "session": "9",
        "state": "running"
    }))
    .expect("a running session reads");
    assert_eq!(running.source, None);
    assert_eq!(running.line, None);
}

/// A provider that cannot step is still a debug provider; the missing ability is reported, not fatal.
#[test]
fn a_missing_optional_ability_does_not_withdraw_the_provider() {
    let host: plugin_protocol::service::Contract =
        serde_json::from_value(debug_declaration()).expect("the host's declaration is valid");
    // Everything the host declares is offered.
    assert_eq!(
        debug_abilities(&host),
        DebugAbilities {
            breakpoints: true,
            resume_pause: true,
            step: true,
            inspect: true,
        }
    );

    // Dropping `step` leaves the rest offered and says so, rather than making the provider unusable.
    let mut without_step = host.clone();
    without_step.methods.remove("step");
    let abilities = debug_abilities(&without_step);
    assert!(!abilities.step);
    assert!(abilities.breakpoints && abilities.resume_pause);

    // A provider that declares a method with a different shape has not offered that ability: the same
    // exactness the base requirement uses applies per capability, so an incomplete offer is no offer.
    let mut reshaped = host.clone();
    if let Some(step) = reshaped.methods.get_mut("step") {
        step.parameters = serde_json::from_value(serde_json::json!({
            "type": "string",
            "max_bytes": 32
        }))
        .expect("a reshaped schema parses");
    }
    assert!(!debug_abilities(&reshaped).step);

    // Dropping resume and pause withdraws that ability together: a session you can pause but never
    // continue is worse than one that never offered either.
    let mut without_control = host.clone();
    without_control.methods.remove("resume");
    without_control.methods.remove("pause");
    let abilities = debug_abilities(&without_control);
    assert!(!abilities.resume_pause);
    assert!(abilities.step, "the abilities are independent");

    // A declaration that is not the host's at all offers nothing, without failing.
    let foreign: plugin_protocol::service::Contract =
        serde_json::from_value(serde_json::json!({"version": "1.0.0", "methods": {}}))
            .expect("an empty declaration parses");
    assert_eq!(debug_abilities(&foreign), DebugAbilities::default());
}

/// A frame report is believed exactly as given, and one that is not a location is refused.
#[test]
fn a_frame_report_is_read_or_refused() {
    let frames = frames_from_value(&serde_json::json!({
        "frames": [
            {"id": 0, "name": "probe::add", "source": "src/main.rs", "line": 2},
            {"id": 1, "name": "probe::main", "source": "src/main.rs", "line": 7}
        ]
    }))
    .expect("a real frame list reads");
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].id, 0);
    assert_eq!(frames[0].name, "probe::add");
    assert_eq!(
        (frames[0].source.as_str(), frames[0].line),
        ("src/main.rs", 2)
    );

    // A frame without a location is not a location: each missing piece is refused by name rather than
    // producing a frame the panel would have to display with a blank.
    for (report, expected) in [
        (
            serde_json::json!({"frames": [{"id": 0, "name": "f", "line": 2}]}),
            "source",
        ),
        (
            serde_json::json!({"frames": [{"id": 0, "name": "f", "source": "s"}]}),
            "line",
        ),
        (
            serde_json::json!({"frames": [{"id": 0, "source": "s", "line": 2}]}),
            "name",
        ),
        (
            serde_json::json!({"frames": [{"name": "f", "source": "s", "line": 2}]}),
            "identity",
        ),
        // Line numbers are one-based, so a reported zero is a mistake rather than a location.
        (
            serde_json::json!({"frames": [{"id": 0, "name": "f", "source": "s", "line": 0}]}),
            "line",
        ),
        // A report with no list at all is not an empty stack.
        (serde_json::json!({}), "frame list"),
    ] {
        let error = frames_from_value(&report).expect_err("the report is refused");
        assert!(
            error.message.contains(expected),
            "{expected} was not named: {error:?}"
        );
    }

    // A list over the declared bound is refused rather than shown in part.
    let oversized = serde_json::json!({
        "frames": (0..=MAX_DEBUG_FRAMES)
            .map(|id| serde_json::json!({
                "id": id as u32, "name": "f", "source": "s", "line": 1
            }))
            .collect::<Vec<_>>()
    });
    assert!(frames_from_value(&oversized).is_err());
    // An empty list is a real answer: a stopped target with no frames to report.
    assert!(
        frames_from_value(&serde_json::json!({"frames": []}))
            .expect("an empty stack is valid")
            .is_empty()
    );
}

/// A variable is the provider's own rendering, shown as given and never reinterpreted.
#[test]
fn a_variable_report_is_shown_as_given() {
    let variables = variables_from_value(&serde_json::json!({
        "variables": [
            {"name": "left", "value": "2"},
            {"name": "text", "value": "\"a b\""},
            // An empty rendering is a value, not a missing one.
            {"name": "nothing", "value": ""}
        ]
    }))
    .expect("a real variable list reads");
    assert_eq!(variables.len(), 3);
    assert_eq!(variables[1].value, "\"a b\"", "quoting is the provider's");
    assert_eq!(variables[2].value, "");
    // A variable without a name cannot be shown, and no name is invented for it.
    let error = variables_from_value(&serde_json::json!({"variables": [{"value": "1"}]}))
        .expect_err("a nameless variable is refused");
    assert!(error.message.contains("name"), "{error:?}");
    assert!(variables_from_value(&serde_json::json!({})).is_err());
    assert!(variables_from_value(&serde_json::json!({"variables": []})).is_ok());
}

/// Breakpoints are grouped by source, because one request per source is what a provider is asked for.
#[test]
fn breakpoints_are_grouped_by_their_source() {
    let requests = [
        DebugBreakpointRequest {
            source: "src/main.rs".into(),
            line: 4,
        },
        DebugBreakpointRequest {
            source: "src/lib.rs".into(),
            line: 12,
        },
        DebugBreakpointRequest {
            source: "src/main.rs".into(),
            line: 9,
        },
    ];
    let grouped = breakpoints_by_source(&requests);
    assert_eq!(grouped.len(), 2);
    assert_eq!(grouped["src/main.rs"].len(), 2);
    assert_eq!(grouped["src/lib.rs"].len(), 1);
    assert_eq!(
        grouped["src/main.rs"][0],
        serde_json::json!({"source": "src/main.rs", "line": 4})
    );

    // An unverified breakpoint is reported as unverified rather than counted as set.
    let acknowledged = DebugBreakpoint::list_from_value(&serde_json::json!({
        "breakpoints": [
            {"source": "src/main.rs", "line": 4, "verified": true},
            {"source": "src/main.rs", "line": 9}
        ]
    }))
    .expect("the list reads");
    assert!(acknowledged[0].verified);
    assert!(
        !acknowledged[1].verified,
        "a breakpoint without verification is not claimed to be set"
    );
}
