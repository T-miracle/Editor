//! From a discovered target to a configuration, and what a later discovery may do to it.
use super::*;
use crate::run::{RunConfigSet, configuration_for, reconcile, repair};
use std::collections::BTreeMap;

fn target(id: &str, label: &str) -> DiscoveredTarget {
    DiscoveredTarget {
        id: id.to_owned(),
        provider: "rust-binary".into(),
        target_type: "rust-binary".into(),
        program: label.to_owned(),
        label: label.to_owned(),
        fields: BTreeMap::new(),
        found_in: "Cargo.toml".into(),
    }
}

fn with_fields(id: &str, label: &str, fields: &[(&str, &str)]) -> DiscoveredTarget {
    let mut target = target(id, label);
    target.fields = fields
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    target
}

fn store(config: RunConfig) -> RunConfigSet {
    let mut set = RunConfigSet::default();
    set.upsert(config).unwrap();
    set
}

/// A confirmed candidate becomes an ordinary editable configuration.
#[test]
fn a_confirmed_target_becomes_an_editable_configuration() {
    let configuration = configuration_for(
        &with_fields(
            "rust-binary:my-app",
            "my-app",
            &[
                ("build_program", "cargo.exe"),
                ("build_args", "build\n--release"),
            ],
        ),
        "run-1".into(),
        "我的程序".into(),
    );
    assert_eq!(configuration.name, "我的程序");
    assert_eq!(configuration.target.executable(), "my-app");
    assert_eq!(
        configuration.from_target.as_deref(),
        Some("rust-binary:my-app")
    );
    assert!(
        configuration.local,
        "a discovered configuration starts host-local"
    );
    // The provider's own fields describe the build, so Build is available with a real command.
    assert_eq!(configuration.build.len(), 1);
    assert_eq!(
        configuration.build[0].target.executable(),
        Some("cargo.exe")
    );
    assert_eq!(
        configuration.build[0].target.arguments(),
        vec!["build", "--release"]
    );
    configuration
        .validate()
        .expect("the configuration is usable");

    // A provider that described no build leaves Build honestly unavailable rather than inventing one.
    let bare = configuration_for(&target("tools:fmt", "fmt"), "run-2".into(), "fmt".into());
    assert!(bare.build.is_empty());
    bare.validate()
        .expect("a configuration without a build is still usable");
}

/// A configuration's own choices survive a later discovery; only the target's program is corrected.
#[test]
fn a_user_configuration_survives_re_discovery() {
    let offered = target("rust-binary:my-app", "my-app");
    let mut mine = configuration_for(&offered, "run-1".into(), "我改过的名字".into());
    mine.directory = Some("C:/work/sub".into());
    mine.env = [("APP_MODE".to_owned(), "dev".to_owned())].into();
    mine.tool_paths = vec!["C:/tools".into()];
    mine.target = RunTarget::Program {
        program: "my-app".into(),
        args: vec!["--verbose".into()],
    };
    let stored = store(mine.clone());

    // A discovery that offers the same target changes nothing and offers nothing new.
    let outcome = reconcile(&stored, &[offered.clone()]);
    assert!(outcome.updated.is_empty() && outcome.offered.is_empty() && outcome.missing.is_empty());

    // The same target now names a different program: the program is corrected, nothing else is.
    let renamed = target("rust-binary:my-app", "renamed-app");
    let outcome = reconcile(&stored, &[renamed.clone()]);
    assert_eq!(outcome.updated, vec!["run-1".to_owned()]);
    let repaired = repair(&mine, &renamed);
    assert_eq!(repaired.target.executable(), "renamed-app");
    assert_eq!(
        repaired.literal_arguments(),
        vec!["--verbose"],
        "the user's arguments survive"
    );
    assert_eq!(repaired.name, "我改过的名字");
    assert_eq!(repaired.directory.as_deref(), Some("C:/work/sub"));
    assert_eq!(
        repaired.env.get("APP_MODE").map(String::as_str),
        Some("dev")
    );
    assert_eq!(repaired.tool_paths, vec!["C:/tools".to_owned()]);
    // Re-discovery of one target never adds a second configuration for it.
    assert_eq!(
        stored
            .configurations
            .iter()
            .filter(|config| config.from_target.as_deref() == Some("rust-binary:my-app"))
            .count(),
        1
    );
}

/// A target that is gone is reported, not deleted and not silently kept as if it still worked.
#[test]
fn a_missing_target_is_reported() {
    let offered = target("rust-binary:my-app", "my-app");
    let mine = configuration_for(&offered, "run-1".into(), "我的程序".into());
    let stored = store(mine);
    let outcome = reconcile(&stored, &[target("rust-binary:other", "other")]);
    assert_eq!(outcome.missing, vec!["run-1".to_owned()]);
    assert!(
        outcome.updated.is_empty(),
        "a missing target is not an update"
    );
    // The configuration is still there: what to do about a missing target is the user's decision.
    assert!(stored.find("run-1").is_some());
    // A configuration that came from no target is never reported as missing.
    let mut hand_written = configuration_for(&offered, "run-2".into(), "手写".into());
    hand_written.from_target = None;
    let stored = store(hand_written);
    assert!(reconcile(&stored, &[]).missing.is_empty());
}

/// Every unclaimed candidate is offered, and a claimed one is not offered again.
#[test]
fn candidates_are_offered_once_each() {
    let stored = store(configuration_for(
        &target("rust-binary:my-app", "my-app"),
        "run-1".into(),
        "我的程序".into(),
    ));
    let offered = vec![
        target("rust-binary:my-app", "my-app"),
        target("rust-binary:other", "other"),
        target("tools:fmt", "fmt"),
    ];
    let outcome = reconcile(&stored, &offered);
    assert_eq!(
        outcome.offered,
        vec!["rust-binary:other".to_owned(), "tools:fmt".to_owned()],
        "a candidate from any provider is offered the same way"
    );
}
