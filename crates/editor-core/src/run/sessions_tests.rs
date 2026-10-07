//! Two sessions at once, and why one never speaks for the other.
use super::*;
use crate::run::{DebugVariable, InspectionError, StackFrame};

fn paused_at(line: u32) -> DebugSessionState {
    DebugSessionState::Paused {
        source: "src/main.rs".into(),
        line,
        reason: Some("breakpoint".into()),
    }
}

fn session(state: DebugSessionState) -> DebugSession {
    let mut session = DebugSession::default();
    session.note_state(state);
    session
}

/// Two configurations can be debugged together, and pausing one does not pause the other.
#[test]
fn a_session_keeps_its_own_state() {
    let mut sessions = DebugSessions::default();
    sessions.insert("run-1", session(DebugSessionState::Running));
    sessions.insert("run-2", session(DebugSessionState::Running));
    assert_eq!(sessions.len(), 2);

    // Pausing the first leaves the second running.
    sessions
        .session_mut("run-1")
        .expect("the first session")
        .note_state(paused_at(12));
    assert!(matches!(
        sessions.session("run-1").unwrap().state(),
        DebugSessionState::Paused { line: 12, .. }
    ));
    assert_eq!(
        sessions.session("run-2").unwrap().state(),
        &DebugSessionState::Running
    );
    // The panel knows another session is stopped, which is what keeps it from moving the view.
    assert!(sessions.another_is_paused("run-2"));
    assert!(!sessions.another_is_paused("run-1"));

    // Resuming the first leaves the second alone as well.
    sessions
        .session_mut("run-1")
        .expect("the first session")
        .note_state(DebugSessionState::Running);
    assert!(!sessions.another_is_paused("run-2"));
    assert_eq!(sessions.len(), 2, "neither session was removed");
}

/// The panel acts on the selected session only, and the selection follows what is present.
#[test]
fn actions_belong_to_the_selected_session() {
    let mut sessions = DebugSessions::default();
    sessions.insert("run-1", session(DebugSessionState::Running));
    // The first session inserted is the one the panel acts on, so it is never acting on nothing.
    assert_eq!(sessions.selected(), Some("run-1"));
    assert_eq!(sessions.current().map(|(config, _)| config), Some("run-1"));

    sessions.insert("run-2", session(DebugSessionState::Running));
    assert_eq!(
        sessions.selected(),
        Some("run-1"),
        "adding a session does not steal the selection"
    );
    assert!(sessions.select("run-2"), "a selection is explicit");
    assert_eq!(sessions.selected(), Some("run-2"));
    assert!(
        !sessions.select("nobody"),
        "a configuration with no session cannot be selected"
    );
    assert_eq!(
        sessions.selected(),
        Some("run-2"),
        "the selection is unchanged"
    );

    // Removing the selected session moves the panel to one that is still there.
    let removed = sessions.remove("run-2").expect("the session existed");
    assert_eq!(*removed.state(), DebugSessionState::Running);
    assert_eq!(sessions.selected(), Some("run-1"));
    sessions.remove("run-1");
    assert_eq!(sessions.selected(), None);
    assert!(sessions.is_empty());
    assert!(sessions.current().is_none());
}

/// A state that is no longer a pause ends the pause data, however the change arrived.
#[test]
fn leaving_a_pause_invalidates_its_data() {
    let mut session = DebugSession::default();
    session.note_state(paused_at(4));
    let scope = session.begin_pause();
    session
        .set_frames(
            scope,
            vec![StackFrame {
                id: 0,
                name: "f".into(),
                source: "src/main.rs".into(),
                line: 4,
            }],
        )
        .expect("the pause is described");
    assert_eq!(session.pause().frames().len(), 1);

    // Running again ends it: the frames describe a moment the target has left.
    session.note_state(DebugSessionState::Running);
    assert!(session.pause().frames().is_empty());
    assert_eq!(session.pause().scope(), None);
    assert_eq!(
        session.pause().accepts(scope),
        Err(InspectionError::StalePause),
        "an answer about the ended pause is refused"
    );

    // A later pause is a different moment, so nothing from the first is reused.
    session.note_state(paused_at(9));
    let next = session.begin_pause();
    assert_ne!(next, scope);
    // The session listing shows every session, in a stable order.
    let mut sessions = DebugSessions::default();
    sessions.insert("run-2", session.clone());
    sessions.insert("run-1", session);
    assert_eq!(
        sessions
            .entries()
            .map(|(config, _)| config)
            .collect::<Vec<_>>(),
        vec!["run-1", "run-2"]
    );
}
