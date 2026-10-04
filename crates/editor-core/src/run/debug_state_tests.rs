//! Which debug control is offered when, and why the others are not.
use super::*;

fn paused() -> DebugSessionState {
    DebugSessionState::Paused {
        source: "src/main.rs".into(),
        line: 12,
        reason: Some("breakpoint".into()),
    }
}

/// Every unavailable control says why, so a disabled button is never a mystery.
#[test]
fn an_unavailable_control_always_carries_its_reason() {
    let states = [
        DebugSessionState::Disconnected,
        DebugSessionState::Starting,
        DebugSessionState::Running,
        paused(),
        DebugSessionState::Exited,
        DebugSessionState::Failed {
            reason: "适配器退出".into(),
        },
    ];
    for availability in [Ok("adapter"), Err("没有安装提供调试能力的插件")] {
        for state in &states {
            let controls = DebugControls::derive(availability, state);
            for (name, outcome) in [
                ("start", &controls.start),
                ("resume", &controls.resume),
                ("pause", &controls.pause),
                ("stop", &controls.stop),
            ] {
                if let Err(reason) = outcome {
                    assert!(
                        !reason.trim().is_empty(),
                        "{name} is unavailable in {state:?} without a reason"
                    );
                }
            }
        }
    }
}

/// A disconnected editor offers starting only when the host confirmed a provider.
#[test]
fn starting_requires_a_confirmed_provider() {
    let disconnected = DebugSessionState::Disconnected;
    let confirmed = DebugControls::derive(Ok("adapter"), &disconnected);
    assert!(confirmed.can_start());
    assert!(!confirmed.can_stop(), "there is no session to stop");
    assert!(confirmed.resume.is_err() && confirmed.pause.is_err());

    let missing = DebugControls::derive(Err("没有安装提供调试能力的插件"), &disconnected);
    assert!(!missing.can_start());
    assert!(missing.start.unwrap_err().contains("没有安装"));
}

/// Resume and pause follow the target's own state, and neither substitutes for the other.
#[test]
fn resume_and_pause_follow_the_target() {
    let running = DebugControls::derive(Ok("adapter"), &DebugSessionState::Running);
    assert!(
        running.pause.is_ok(),
        "a running target can be asked to stop"
    );
    assert!(running.resume.is_err(), "a running target is not resumed");
    assert!(running.can_stop());

    let stopped = DebugControls::derive(Ok("adapter"), &paused());
    assert!(stopped.resume.is_ok(), "a paused target is continued");
    assert!(
        stopped.pause.is_err(),
        "a paused target is not paused again"
    );
    assert!(stopped.can_stop());
    assert!(
        !stopped.can_start(),
        "a session already being served is stopped rather than started again"
    );

    // Connecting is a state a user needs a way out of, so stopping is offered there.
    let starting = DebugControls::derive(Ok("adapter"), &DebugSessionState::Starting);
    assert!(starting.can_stop());
    assert!(starting.resume.is_err() && starting.pause.is_err());
}

/// A session that is over offers nothing but starting again, and says what happened.
#[test]
fn a_finished_session_offers_starting_again() {
    for state in [
        DebugSessionState::Exited,
        DebugSessionState::Failed {
            reason: "适配器退出".into(),
        },
    ] {
        let controls = DebugControls::derive(Ok("adapter"), &state);
        assert!(
            controls.can_start(),
            "a new session is offered in {state:?}"
        );
        assert!(!controls.can_stop(), "nothing is being served in {state:?}");
        assert!(controls.resume.is_err() && controls.pause.is_err());
    }
    // A failed session reports the provider's own reason rather than a generic one.
    let failed = DebugControls::derive(
        Ok("adapter"),
        &DebugSessionState::Failed {
            reason: "适配器退出".into(),
        },
    );
    assert!(failed.stop.unwrap_err().contains("适配器退出"));
}

/// Where a session is stopped is carried, so the panel can show the location.
#[test]
fn a_paused_session_carries_its_location() {
    let state = paused();
    assert!(state.is_connected());
    assert_eq!(state.paused_at(), Some(("src/main.rs", 12)));
    assert_eq!(DebugSessionState::Running.paused_at(), None);
    assert!(!DebugSessionState::Exited.is_connected());
    // A session with no reason still reports where it stopped.
    let unnamed = DebugSessionState::Paused {
        source: "src/lib.rs".into(),
        line: 3,
        reason: None,
    };
    assert_eq!(unnamed.paused_at(), Some(("src/lib.rs", 3)));
}
