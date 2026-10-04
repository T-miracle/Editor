//! Sharing: what leaves this machine, what stays, and how a project entry is resolved.
use super::*;
use crate::run::{RunStep, StepTarget};
use std::path::Path;

fn program(id: &str, name: &str) -> RunConfig {
    RunConfig {
        id: id.into(),
        name: name.into(),
        target: RunTarget::Program {
            program: "app.exe".into(),
            args: vec!["--flag".into(), "a b".into()],
        },
        directory: Some("C:/work/project".into()),
        env: Default::default(),
        tool_paths: Default::default(),
        build: Vec::new(),
        prelaunch: Vec::new(),
        source: RunConfigSource::Local,
        from_target: None,
        provider: None,
        local: true,
    }
}

/// A shared entry carries the portable half and none of this machine's own values.
#[test]
fn sharing_exports_only_portable_values() {
    let workspace = Path::new("C:/work/project");
    let mut config = program("run-1", "共享");
    config.directory = Some("C:/work/project/tools".into());
    config.env = [("SECRET".to_owned(), "s3cret".to_owned())].into();
    config.tool_paths = vec!["C:/Users/someone/.cargo/bin".into()];
    config.build = vec![RunStep {
        name: "构建".into(),
        target: StepTarget::Action {
            target: RunTarget::Program {
                program: "cargo.exe".into(),
                args: vec!["build".into()],
            },
        },
    }];
    config.prelaunch = vec![RunStep {
        name: "先建库".into(),
        target: StepTarget::Build {
            config: "库".into(),
        },
    }];

    let shared = SharedConfig::from_config(&config, workspace);
    // Portable values survive, including a literal argument containing a space.
    assert_eq!(shared.name, "共享");
    assert_eq!(shared.target, config.target);
    assert_eq!(shared.build, config.build);
    assert_eq!(shared.prelaunch, config.prelaunch);
    // A directory inside the project becomes relative; the project root becomes the token.
    assert_eq!(shared.directory.as_deref(), Some("tools"));
    let bytes = serde_json::to_string(&shared).unwrap();
    assert!(
        !bytes.contains("s3cret"),
        "a value never leaves the machine"
    );
    assert!(!bytes.contains("SECRET"));
    assert!(
        !bytes.contains("someone"),
        "a personal tool path is not shared"
    );

    // A directory outside the project has no portable form and is kept as written rather than
    // invented as a path inside the project.
    let mut outside = program("run-2", "外部");
    outside.directory = Some("D:/elsewhere".into());
    assert_eq!(
        SharedConfig::from_config(&outside, workspace)
            .directory
            .as_deref(),
        Some("D:/elsewhere")
    );
    // The project root itself is the token the host resolves per workspace.
    let mut root = program("run-3", "根");
    root.directory = Some("C:/work/project".into());
    assert_eq!(
        SharedConfig::from_config(&root, workspace)
            .directory
            .as_deref(),
        Some(WORKSPACE_TOKEN)
    );
}

/// A shared entry round-trips through the file and resolves per workspace.
#[test]
fn a_shared_file_round_trips_and_resolves_per_workspace() {
    let first = Path::new("C:/one/project");
    let second = Path::new("D:/two/project");
    let mut set = SharedSet::default();
    let mut inside = program("run-1", "共享");
    inside.directory = Some(first.display().to_string());
    set.upsert(SharedConfig::from_config(&inside, first));
    let bytes = set.to_json().unwrap();
    let document: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        document["configurations"].as_array().unwrap().len(),
        1,
        "one entry"
    );
    assert_eq!(
        document["configurations"][0]["directory"].as_str(),
        Some(WORKSPACE_TOKEN),
        "the project root is stored as a token"
    );
    let read = SharedSet::from_json(&bytes).expect("the file this build wrote is readable");
    assert_eq!(read, set);
    // The same entry resolves to each workspace's own project root.
    let here = read.configurations[0].clone().resolve(first, None);
    assert_eq!(here.directory.as_deref(), Some("C:/one/project"));
    let there = read.configurations[0].clone().resolve(second, None);
    assert_eq!(there.directory.as_deref(), Some("D:/two/project"));
    // A resolved project entry knows where it came from, so saving writes back to the file.
    assert_eq!(here.source, RunConfigSource::Project);
    assert!(!here.local);
    // With no overrides a shared entry carries no environment and no tool directories.
    assert!(here.env.is_empty() && here.tool_paths.is_empty());
}

/// This machine's own values are applied on top of a shared entry, keyed by identity.
#[test]
fn local_overrides_are_applied_by_identity() {
    let workspace = Path::new("C:/work/project");
    let mut local = RunConfigSet::default();
    let mut mine = program("run-1", "共享");
    mine.env = [("SECRET".to_owned(), "s3cret".to_owned())].into();
    mine.tool_paths = vec!["C:/tools/bin".into()];
    mine.directory = Some("C:/work/project".into());
    local.upsert(mine).unwrap();
    // Another configuration's values must not be borrowed by the shared entry.
    let mut other = program("run-2", "别的");
    other.env = [("OTHER".to_owned(), "value".to_owned())].into();
    local.upsert(other).unwrap();

    let mut shared = SharedSet::default();
    let mut entry = SharedConfig::from_config(&program("run-1", "共享"), workspace);
    entry.name = "项目里的名字".into();
    shared.upsert(entry);

    let merged = merge(workspace, &local, &shared);
    let resolved = merged.find("run-1").expect("the shared entry is present");
    assert_eq!(
        resolved.name, "项目里的名字",
        "the file owns the portable half"
    );
    assert_eq!(
        resolved.env.get("SECRET").map(String::as_str),
        Some("s3cret"),
        "this machine's own value is applied"
    );
    assert_eq!(resolved.tool_paths, vec!["C:/tools/bin".to_owned()]);
    assert_eq!(
        resolved.directory.as_deref(),
        Some("C:/work/project"),
        "the token resolves to this machine's own project"
    );
    // The machine's other configuration is untouched, and the merged set has both.
    assert_eq!(merged.configurations.len(), 2);
    assert_eq!(
        merged
            .find("run-2")
            .unwrap()
            .env
            .get("OTHER")
            .map(String::as_str),
        Some("value")
    );
    // Merging does not create a second entry for the same identity.
    assert_eq!(
        merged
            .configurations
            .iter()
            .filter(|config| config.id == "run-1")
            .count(),
        1
    );
}

/// A shared file is held to the same rules as the form.
#[test]
fn a_shared_file_is_validated_by_the_same_rules() {
    // A program that the form would refuse is refused when it arrives from a file.
    let broken = serde_json::json!({
        "version": SHARED_CONFIG_VERSION,
        "configurations": [{
            "id": "run-1",
            "name": "坏的",
            "target": {"mode": "program", "program": "", "args": []}
        }]
    });
    let error = match SharedSet::from_json(&serde_json::to_vec(&broken).unwrap()) {
        Ok(_) => panic!("an empty program is refused from a file too"),
        Err(error) => error,
    };
    assert!(matches!(error, SharedStoreError::Invalid(_)), "{error:?}");

    // A file written by a newer build is refused rather than silently rewritten.
    let newer = serde_json::json!({
        "version": SHARED_CONFIG_VERSION + 1,
        "configurations": []
    });
    let error = SharedSet::from_json(&serde_json::to_vec(&newer).unwrap())
        .expect_err("a newer shared file is refused");
    assert!(
        matches!(error, SharedStoreError::UnsupportedVersion { .. }),
        "{error}"
    );

    // An unknown field is refused so a hand edit cannot quietly mean something else.
    let unknown = serde_json::json!({
        "version": SHARED_CONFIG_VERSION,
        "configurations": [{
            "id": "run-1",
            "name": "x",
            "target": {"mode": "program", "program": "app.exe", "args": []},
            "env": {"SECRET": "value"}
        }]
    });
    assert!(
        SharedSet::from_json(&serde_json::to_vec(&unknown).unwrap()).is_err(),
        "a field this format does not define is not accepted"
    );
}
