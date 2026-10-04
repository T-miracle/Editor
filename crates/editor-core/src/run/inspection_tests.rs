//! Late inspection answers, frame selection, and what a new pause does to the old one.
use super::*;

fn frames() -> Vec<StackFrame> {
    vec![
        StackFrame {
            id: 0,
            name: "probe_debug::add".into(),
            source: "src/main.rs".into(),
            line: 2,
        },
        StackFrame {
            id: 1,
            name: "probe_debug::main".into(),
            source: "src/main.rs".into(),
            line: 7,
        },
    ]
}

fn variables() -> Vec<DebugVariable> {
    vec![
        DebugVariable {
            name: "left".into(),
            value: "2".into(),
        },
        DebugVariable {
            name: "right".into(),
            value: "3".into(),
        },
    ]
}

/// A pause describes exactly one stopped moment, and the moment after it refuses the first's data.
#[test]
fn an_answer_about_an_earlier_pause_is_refused() {
    let mut data = PauseData::default();
    assert_eq!(data.scope(), None);
    let first = data.begin();
    data.set_frames(first, frames()).expect("the first pause");
    data.set_variables(first, 0, variables())
        .expect("the first pause's variables");
    assert_eq!(data.frames().len(), 2);
    assert_eq!(data.variables_of(0).len(), 2);

    // A step ends the pause; an answer still on its way about the old one is refused, so it cannot
    // replace the view the user is now looking at.
    let second = data.begin();
    assert_ne!(first, second, "each pause is its own moment");
    assert!(
        data.frames().is_empty(),
        "the previous pause's frames are gone"
    );
    assert!(data.variables_of(0).is_empty());
    assert_eq!(
        data.set_frames(first, frames()),
        Err(InspectionError::StalePause)
    );
    assert_eq!(
        data.set_variables(first, 0, variables()),
        Err(InspectionError::StalePause)
    );
    assert!(
        data.accepts(second).is_ok(),
        "the current pause is the one answers are applied to"
    );

    // The current pause accepts its own answers.
    data.set_frames(second, frames()).expect("the second pause");
    assert!(data.accepts(second).is_ok());
    assert!(data.set_variables(first, 0, variables()).is_err());
    data.set_variables(second, 0, variables())
        .expect("the second pause's variables");
    assert_eq!(data.variables_of(0).len(), 2);
}

/// Resuming or restarting ends the pause, and every answer about it is refused afterwards.
#[test]
fn clearing_ends_the_pause_for_good() {
    let mut data = PauseData::default();
    let scope = data.begin();
    data.set_frames(scope, frames()).expect("the pause");
    data.clear();
    assert_eq!(data.scope(), None);
    assert!(data.frames().is_empty());
    assert_eq!(
        data.set_frames(scope, frames()),
        Err(InspectionError::StalePause)
    );
    // A later pause gets a scope of its own, never the one that was cleared.
    let next = data.begin();
    assert_ne!(next, scope);
}

/// Selecting a frame is what locates the source, and an unknown frame is refused by name.
#[test]
fn selecting_a_frame_locates_its_source() {
    let mut data = PauseData::default();
    let scope = data.begin();
    data.set_frames(scope, frames()).expect("the pause");
    // The first frame is where the target stopped, so it is what a user sees first.
    assert_eq!(data.selected_frame(), Some(0));
    assert_eq!(data.selected_location(), Some(("src/main.rs", 2)));

    let selected = data.select_frame(1).expect("the frame exists");
    assert_eq!(selected.name, "probe_debug::main");
    assert_eq!(
        data.selected_location(),
        Some(("src/main.rs", 7)),
        "switching frames moves the location"
    );
    assert_eq!(
        data.select_frame(9),
        Err(InspectionError::NoSuchFrame { frame: 9 })
    );
    assert_eq!(
        data.set_variables(scope, 9, variables()),
        Err(InspectionError::NoSuchFrame { frame: 9 })
    );
    // Variables read for one frame are kept while another is selected, so switching does not discard.
    data.set_variables(scope, 1, variables()).expect("frame 1");
    assert_eq!(data.variables_of(1).len(), 2);
    assert_eq!(data.variables_of(0).len(), 0, "frame 0 was never read");
}

/// A pause with no frames is not a pause whose frames can be added to.
#[test]
fn a_pause_without_frames_has_nothing_to_select() {
    let mut data = PauseData::default();
    let scope = data.begin();
    assert_eq!(data.select_frame(0), Err(InspectionError::NotPaused));
    assert_eq!(data.selected_location(), None);
    // A provider that reports no frames is believed: an empty list is empty, not a fabricated frame.
    data.set_frames(scope, Vec::new())
        .expect("an empty list is valid");
    assert!(data.frames().is_empty());
    assert_eq!(data.selected_frame(), None);
    assert_eq!(data.selected_location(), None);
}
