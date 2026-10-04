//! The host's debug requirement is its own declaration, so its meaning is checkable on its own.
//!
//! These are unit checks of the shapes a provider must declare and of how the host reads what one
//! reports. They name no debugger: a provider that declares this contract is interchangeable.
use super::*;

/// The host asks for exactly the methods a debug session needs, and nothing language specific.
#[test]
fn the_debug_requirement_names_no_debugger() {
    let dependency = debug_dependency().expect("the host's own requirement is valid");
    let methods = dependency.methods.keys().cloned().collect::<Vec<_>>();
    assert_eq!(
        methods,
        vec![
            "pause".to_owned(),
            "resume".to_owned(),
            "set_breakpoints".to_owned(),
            "start".to_owned(),
            "status".to_owned(),
            "stop".to_owned()
        ],
        "the host requires exactly what it calls"
    );
    // Every method the host calls has a window; a method it does not call is not silently callable.
    for method in &methods {
        assert!(
            debug_timeout_ms(method).is_some(),
            "{method} has no window, so it could be called without a bound"
        );
    }
    assert_eq!(debug_timeout_ms("step"), None);
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
