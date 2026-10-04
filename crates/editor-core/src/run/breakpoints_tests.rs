//! A configuration's stop locations: what may be stored, and what a portability rule protects.
use super::*;
use crate::run::shared::SharedConfig;

fn set(entries: &[(&str, u32)]) -> RunBreakpoints {
    let mut breakpoints = RunBreakpoints::default();
    for (source, line) in entries {
        breakpoints
            .insert(source, *line)
            .expect("the location is valid");
    }
    breakpoints
}

/// A location is a source and a one-based line, and an unusable one is refused rather than stored.
#[test]
fn only_real_locations_are_stored() {
    let mut breakpoints = RunBreakpoints::default();
    breakpoints
        .insert("src/main.rs", 4)
        .expect("a real location");
    assert_eq!(breakpoints.len(), 1);
    assert!(breakpoints.contains("src/main.rs", 4));

    // Line zero is a mistake, not a location no provider could bind.
    assert_eq!(
        breakpoints.insert("src/main.rs", 0),
        Err(BreakpointError::NoSuchLine)
    );
    assert_eq!(
        breakpoints.insert("   ", 3),
        Err(BreakpointError::EmptySource)
    );
    assert!(matches!(
        breakpoints.insert(&"a".repeat(MAX_BREAKPOINT_SOURCE_BYTES + 1), 3),
        Err(BreakpointError::SourceTooLong { .. })
    ));
    // Setting one line twice would be one breakpoint the provider sets twice.
    assert_eq!(
        breakpoints.insert("src/main.rs", 4),
        Err(BreakpointError::AlreadySet)
    );
    assert_eq!(breakpoints.len(), 1);

    // The same file reached through either separator is one file, not two.
    assert!(breakpoints.contains(r"src\main.rs", 4));
    assert!(breakpoints.remove(r"src\main.rs", 4));
    assert!(breakpoints.is_empty());
    assert!(!breakpoints.remove("src/main.rs", 4));
}

/// The list reads in project order, and removing a source takes all of its locations with it.
#[test]
fn the_list_is_ordered_and_a_source_can_be_cleared() {
    let mut breakpoints = set(&[("src/main.rs", 9), ("src/lib.rs", 2), ("src/main.rs", 4)]);
    let order = breakpoints
        .entries()
        .iter()
        .map(|entry| (entry.source.as_str(), entry.line))
        .collect::<Vec<_>>();
    assert_eq!(
        order,
        vec![("src/lib.rs", 2), ("src/main.rs", 4), ("src/main.rs", 9)],
        "the list reads like the project rather than like insert order"
    );
    assert_eq!(breakpoints.remove_source("src/main.rs"), 2);
    assert_eq!(breakpoints.len(), 1);
    assert_eq!(breakpoints.remove_source("src/main.rs"), 0);
}

/// A location that arrives from elsewhere is validated the same way, and a stale one is dropped.
#[test]
fn acknowledged_locations_are_the_only_ones_kept() {
    // A file that claims too many locations is refused rather than quietly trimmed.
    let entries = (1..=(MAX_RUN_BREAKPOINTS as u32 + 1))
        .map(|line| RunBreakpoint {
            source: "src/main.rs".into(),
            line,
        })
        .collect::<Vec<_>>();
    let document = serde_json::json!({"entries": entries});
    let oversized: RunBreakpoints = serde_json::from_value(document).expect("the list parses");
    assert!(matches!(
        oversized.validate(),
        Err(BreakpointError::TooMany { .. })
    ));

    // A provider that could not bind a location does not leave it looking set.
    let mut breakpoints = set(&[("src/main.rs", 4), ("src/main.rs", 9), ("src/lib.rs", 2)]);
    breakpoints.retain_acknowledged(&[("src/main.rs".into(), 9), ("src/lib.rs".into(), 2)]);
    assert_eq!(breakpoints.len(), 2);
    assert!(!breakpoints.contains("src/main.rs", 4));
    assert!(breakpoints.contains("src/main.rs", 9));
}

/// A stop location travels with the project because it means the same thing on another machine.
#[test]
fn breakpoints_travel_with_a_shared_entry() {
    let mut configuration = crate::run::tests::program_config("run-1", "第一个");
    configuration
        .breakpoints
        .insert("src/main.rs", 7)
        .expect("the location is valid");
    let workspace = std::path::Path::new("C:/work");
    let shared = SharedConfig::from_config(&configuration, workspace);
    assert_eq!(
        shared.breakpoints.len(),
        1,
        "a stop location is portable, so it is part of the shared entry"
    );
    // And it comes back on a machine whose own record has no such breakpoint.
    let resolved = shared.resolve(workspace, None);
    assert!(resolved.breakpoints.contains("src/main.rs", 7));
}
