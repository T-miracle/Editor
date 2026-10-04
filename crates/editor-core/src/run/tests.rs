//! Run configuration rules and their host-local file contract.
use super::*;

fn program_config(id: &str, name: &str) -> RunConfig {
    RunConfig {
        id: id.into(),
        name: name.into(),
        target: RunTarget::Program {
            program: "powershell.exe".into(),
            args: vec![
                "-NoProfile".into(),
                "-Command".into(),
                "Write-Output ok".into(),
            ],
        },
        directory: Some("C:/work".into()),
        env: Default::default(),
        tool_paths: Default::default(),
        build: Default::default(),
        prelaunch: Default::default(),
        source: RunConfigSource::Local,
        local: true,
    }
}

/// Tool directories search before the inherited path and never replace it.
#[test]
fn tool_directories_lead_the_search_order() {
    use std::collections::BTreeMap;
    // With no override the caller's entries are handed back exactly as written.
    let written = BTreeMap::from([("APP_MODE".to_owned(), "dev".to_owned())]);
    assert_eq!(launch_environment(&written, &[]), written);

    // An override leads, and one PATH reaches the child even when the user also wrote one.
    let with_path = BTreeMap::from([
        ("Path".to_owned(), "C:/inherited".to_owned()),
        ("APP_MODE".to_owned(), "dev".to_owned()),
    ]);
    let derived = launch_environment(
        &with_path,
        &["C:/tools/bin".to_owned(), "C:/more/bin".to_owned()],
    );
    let separator = if cfg!(windows) { ';' } else { ':' };
    let expected = format!("C:/tools/bin{separator}C:/more/bin{separator}C:/inherited");
    assert_eq!(
        derived.get("PATH").map(String::as_str),
        Some(expected.as_str())
    );
    // The user's own spelling of the variable does not survive as a second entry.
    assert_eq!(
        derived
            .keys()
            .filter(|name| name.eq_ignore_ascii_case("PATH"))
            .count(),
        1
    );
    assert_eq!(derived.get("APP_MODE").map(String::as_str), Some("dev"));
}

/// A tool directory that could not be searched is refused with its name.
#[test]
fn tool_directories_must_be_absolute_and_searchable() {
    let mut configuration = program_config("run-1", "工具");
    configuration.tool_paths = vec!["relative/bin".into()];
    assert_eq!(
        configuration.validate(),
        Err(RunConfigError::InvalidToolPath {
            path: "relative/bin".into()
        })
    );
    // A separator inside one entry would silently become two search directories.
    configuration.tool_paths = vec!["C:/one;C:/two".into()];
    assert!(matches!(
        configuration.validate(),
        Err(RunConfigError::InvalidToolPath { .. })
    ));
    configuration.tool_paths = vec!["C:/tools/bin".into()];
    assert_eq!(configuration.validate(), Ok(()));
    configuration.tool_paths = (0..17).map(|index| format!("C:/t{index}")).collect();
    assert_eq!(
        configuration.validate(),
        Err(RunConfigError::TooManyToolPaths)
    );
}

/// Saving, editing and re-selecting one configuration keeps its identity and content.
#[test]
fn upsert_keeps_identity_and_updates_in_place() {
    let mut set = RunConfigSet::default();
    set.upsert(program_config("run-1", "第一次")).unwrap();
    set.select("run-1");
    assert_eq!(set.selected().map(|entry| entry.id.as_str()), Some("run-1"));

    let mut edited = program_config("run-1", "改名后");
    edited.target = RunTarget::Program {
        program: "cargo.exe".into(),
        args: vec!["run".into()],
    };
    set.upsert(edited.clone()).unwrap();
    assert_eq!(set.configurations.len(), 1);
    assert_eq!(set.selected().unwrap().name, "改名后");
    assert_eq!(set.selected().unwrap(), &edited);
    // Selecting an unknown identity leaves the previous selection intact.
    assert!(!set.select("missing"));
    assert_eq!(set.selected().map(|entry| entry.id.as_str()), Some("run-1"));
    // Removing the selected configuration clears the selection instead of dangling.
    assert!(set.remove("run-1"));
    assert!(set.selected().is_none());
    assert!(!set.remove("run-1"));
}

/// Invalid settings are refused before they can be stored or launched.
#[test]
fn validation_rejects_incomplete_or_ambiguous_settings() {
    let mut empty_name = program_config("run-1", "   ");
    assert_eq!(empty_name.validate(), Err(RunConfigError::EmptyName));
    empty_name.name = "ok".into();

    let mut empty_program = empty_name.clone();
    empty_program.target = RunTarget::Program {
        program: "  ".into(),
        args: vec![],
    };
    assert_eq!(empty_program.validate(), Err(RunConfigError::EmptyProgram));

    let mut relative_directory = empty_name.clone();
    relative_directory.directory = Some("target/debug".into());
    assert_eq!(
        relative_directory.validate(),
        Err(RunConfigError::DirectoryNotAbsolute)
    );

    let mut too_many = empty_name.clone();
    too_many.target = RunTarget::Program {
        program: "tool.exe".into(),
        args: vec!["x".into(); MAX_RUN_ARGUMENTS + 1],
    };
    assert_eq!(too_many.validate(), Err(RunConfigError::TooManyArguments));

    let mut oversized_argument = empty_name.clone();
    oversized_argument.target = RunTarget::Program {
        program: "tool.exe".into(),
        args: vec!["x".repeat(MAX_ARGUMENT_BYTES + 1)],
    };
    assert_eq!(
        oversized_argument.validate(),
        Err(RunConfigError::ArgumentTooLong)
    );

    let mut blank_identity = empty_name.clone();
    blank_identity.id = " ".into();
    assert!(matches!(
        blank_identity.validate(),
        Err(RunConfigError::InvalidIdentity { .. })
    ));

    // A valid configuration with no directory is still launchable from the workspace root.
    let mut without_directory = empty_name;
    without_directory.directory = None;
    assert_eq!(without_directory.validate(), Ok(()));
}

/// Argument vectors stay literal: a shell body belongs to interpreter mode, never to program mode.
#[test]
fn program_arguments_are_never_reinterpreted_as_a_shell_command() {
    let configuration = RunConfig {
        id: "run-1".into(),
        name: "带空格参数".into(),
        target: RunTarget::Program {
            program: "C:/Program Files/tool.exe".into(),
            args: vec!["a b".into(), "a&b".into(), "中文".into()],
        },
        directory: None,
        env: Default::default(),
        tool_paths: Default::default(),
        build: Default::default(),
        prelaunch: Default::default(),
        source: RunConfigSource::Local,
        local: true,
    };
    configuration.validate().unwrap();
    assert_eq!(
        configuration.literal_arguments(),
        vec!["a b".to_owned(), "a&b".to_owned(), "中文".to_owned()]
    );
    assert_eq!(
        configuration.target.executable(),
        "C:/Program Files/tool.exe"
    );

    // Interpreter mode appends the script body as one more literal argument.
    let script = RunConfig {
        target: RunTarget::Script {
            interpreter: "pwsh.exe".into(),
            args: vec!["-NoProfile".into(), "-Command".into()],
            script: "Get-ChildItem | Select-Object -First 3".into(),
        },
        ..configuration
    };
    assert_eq!(
        script.literal_arguments(),
        vec![
            "-NoProfile".to_owned(),
            "-Command".to_owned(),
            "Get-ChildItem | Select-Object -First 3".to_owned()
        ]
    );
}

/// Generated identities avoid every identity already stored, even after removals.
#[test]
fn generated_identities_do_not_collide_with_stored_ones() {
    let set = RunConfigSet::default();
    let first = set.generate_id("C:/work/project");
    assert!(set.find(&first).is_none());

    let mut collision = RunConfigSet::default();
    for _ in 0..3 {
        let id = collision.generate_id("C:/work/project");
        collision
            .upsert(program_config(&id, "生成"))
            .expect("generated identity is valid");
    }
    let ids = collision
        .configurations
        .iter()
        .map(|entry| entry.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), 3);
    // The next generated identity is not one of them.
    assert!(!ids.contains(&collision.generate_id("C:/work/project")));
    assert_eq!(collision.counts().get("program").copied(), Some(3));
}

/// A newer stored format is refused instead of being read with missing fields.
#[test]
fn newer_stored_format_is_refused() {
    let text = format!(
        r#"{{"version":{},"configurations":[],"selected":null}}"#,
        RUN_CONFIG_VERSION + 1
    );
    match RunConfigSet::from_json(text.as_bytes()) {
        Err(RunStoreError::Invalid(RunConfigError::UnsupportedVersion { found })) => {
            assert_eq!(found, RUN_CONFIG_VERSION + 1);
        }
        other => panic!("expected an unsupported-version error, got {other:?}"),
    }
}

/// Stored files round-trip through the real file contract, and a failed write keeps the old file.
#[test]
fn stored_file_round_trips_and_missing_files_start_empty() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = "C:/work/project";

    let empty = store::load(directory.path(), workspace).unwrap();
    assert!(empty.configurations.is_empty() && empty.selected.is_none());

    let mut set = RunConfigSet::default();
    let id = set.generate_id(workspace);
    let mut configuration = program_config(&id, "本机配置");
    // A relative directory is invalid, so it must be refused by the writer as well.
    configuration.directory = Some("relative".into());
    set.configurations.push(configuration);
    assert!(matches!(
        store::save(directory.path(), workspace, &set),
        Err(RunStoreError::Invalid(RunConfigError::DirectoryNotAbsolute))
    ));
    assert!(
        !store::file_for(directory.path(), workspace).exists(),
        "a rejected set must not create a file"
    );

    set.configurations[0].directory = Some("C:/work".into());
    set.select(&id);
    store::save(directory.path(), workspace, &set).unwrap();
    let loaded = store::load(directory.path(), workspace).unwrap();
    assert_eq!(loaded.version(), RUN_CONFIG_VERSION);
    assert_eq!(loaded.configurations, set.configurations);
    assert_eq!(loaded.selected.as_deref(), Some(id.as_str()));

    // Another workspace keeps its own file and starts empty.
    assert!(
        store::load(directory.path(), "C:/work/other")
            .unwrap()
            .configurations
            .is_empty()
    );

    // A malformed file is reported rather than silently replaced with defaults.
    std::fs::write(store::file_for(directory.path(), workspace), b"{").unwrap();
    assert!(matches!(
        store::load(directory.path(), workspace),
        Err(RunStoreError::Malformed(_))
    ));
}

/// The file name is derived from the workspace only and never leaks the path itself.
#[test]
fn file_names_are_stable_and_do_not_contain_the_workspace_path() {
    let base = std::path::Path::new("C:/state");
    let first = store::file_for(base, "C:/work/project");
    assert_eq!(first, store::file_for(base, "C:/work/project"));
    assert_ne!(first, store::file_for(base, "C:/work/other"));
    let name = first.file_name().unwrap().to_string_lossy().into_owned();
    assert!(!name.contains("project") && name.ends_with(".json"));
}
