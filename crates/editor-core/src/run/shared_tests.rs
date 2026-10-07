//! Sharing: what leaves this machine, what stays, and how a project entry is resolved.
use super::*;
use crate::run::{RunStep, StepTarget};
use std::path::Path;

/// Machine discovery links survive argument edits, but cannot bind a new on-disk target to an old source.
#[test]
fn shared_target_changes_drop_stale_discovery_provenance() {
    let workspace = Path::new("C:/work/project");
    for provided in [false, true] {
        let mut local = program("stable", "Target");
        local.from_target = Some("machine-target".into());
        local.env.insert("LOCAL_VALUE".into(), "preserved".into());
        if provided {
            local.target = RunTarget::Provided {
                provider: "first-source".into(),
                binding: "original".into(),
                label: "Target".into(),
                args: vec![],
            };
        }
        let mut shared = SharedConfig::from_config(&local, workspace);
        match &mut shared.target {
            RunTarget::Program { args, .. } | RunTarget::Provided { args, .. } => {
                args.push("new literal argument".into())
            }
            _ => unreachable!(),
        }
        assert_eq!(
            shared.clone().resolve(workspace, Some(&local)).from_target,
            local.from_target
        );
        shared.target = RunTarget::Program {
            program: "replacement.exe".into(),
            args: vec![],
        };
        let resolved = shared.resolve(workspace, Some(&local));
        assert!(
            resolved.from_target.is_none(),
            "a target replacement must not inherit an unrelated discovery identity"
        );
        assert_eq!(
            resolved.env, local.env,
            "machine overrides remain independent of the discovery link"
        );
    }
}

/// Editing a legacy v1 file upgrades only its format; identities, scripts and ordered steps survive.
#[test]
fn legacy_sharing_migrates_without_clearing_entries_and_preserves_portable_bindings() {
    let workspace = Path::new("C:/work/project");
    let mut original = program("stable", "legacy");
    original.target = RunTarget::Script {
        interpreter: "powershell.exe".into(),
        args: vec!["-Command".into()],
        script: "Write-Output 'literal | 中文'\nWrite-Output end".into(),
    };
    let legacy = serde_json::json!({"version":1,"configurations":[SharedConfig::from_config(&original,workspace)]});
    let mut migrated = SharedSet::from_json(&serde_json::to_vec(&legacy).unwrap()).unwrap();
    let mut bound = program("bound", "portable target");
    bound.target = RunTarget::Provided {
        provider: "target-provider".into(),
        binding: r#"{"manifest":"nested/Cargo.toml","bin":"native","profile":"dev"}"#.into(),
        label: "Native Debug".into(),
        args: vec!["literal space".into(), "".into()],
    };
    migrated.upsert(SharedConfig::from_config(&bound, workspace));
    let bytes = migrated.to_json().unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["version"], 2);
    let read = SharedSet::from_json(&bytes).unwrap();
    assert_eq!(read.configurations.len(), 2);
    assert_eq!(
        read.configurations[0]
            .clone()
            .resolve(workspace, None)
            .target,
        original.target
    );
    assert_eq!(
        read.configurations[1]
            .clone()
            .resolve(Path::new("D:/elsewhere"), None)
            .target,
        bound.target
    );
    for version in [0, SHARED_CONFIG_VERSION + 1] {
        assert!(
            SharedSet::from_json(
                &serde_json::to_vec(&serde_json::json!({"version":version,"configurations":[]}))
                    .unwrap()
            )
            .is_err()
        );
    }
}

/// A project file must refuse machine-specific executable and working-directory paths.
#[test]
fn shared_files_reject_private_paths_and_parent_directory_escape() {
    let workspace = Path::new("C:/work/project");
    let mut entry = SharedConfig::from_config(&program("run-1", "portable"), workspace);
    for path in ["D:/private/tools", "../outside", "${unknown}/tools"] {
        entry.directory = Some(path.into());
        let mut shared = SharedSet::default();
        shared.upsert(entry.clone());
        assert!(
            SharedSet::from_json(&shared.to_json().unwrap()).is_err(),
            "{path} must not be shared"
        );
    }
    entry.directory = None;
    entry.target = RunTarget::Program {
        program: "C:/Users/person/private/app.exe".into(),
        args: vec![],
    };
    let mut shared = SharedSet::default();
    shared.upsert(entry);
    assert!(SharedSet::from_json(&shared.to_json().unwrap()).is_err());
}

/// Project-relative directories resolve to the selected project before ordinary launch validation.
#[test]
fn relative_shared_directories_are_resolved_before_launch_validation() {
    let workspace = Path::new("C:/work/project");
    for path in ["tools", "${workspace}/tools"] {
        let mut entry = SharedConfig::from_config(&program("run-1", "portable"), workspace);
        entry.directory = Some(path.into());
        let resolved = entry.resolve(workspace, None);
        assert_eq!(
            resolved.directory.as_deref(),
            Some(workspace.join("tools").to_str().unwrap())
        );
        assert!(resolved.validate().is_ok());
    }
}

/// Source paths obey the same portability boundary as executable and working-directory paths.
#[test]
fn shared_breakpoints_convert_project_paths_and_refuse_private_sources() {
    let workspace = Path::new("C:/work/project");
    let mut config = program("run-1", "portable");
    config
        .breakpoints
        .insert("C:/work/project/src/main.rs", 10)
        .unwrap();
    let entry = SharedConfig::from_config(&config, workspace);
    assert_eq!(
        entry.breakpoints.entries()[0].source,
        "${workspace}/src/main.rs"
    );
    let elsewhere = entry.resolve(Path::new("D:/other/project"), None);
    assert_eq!(
        elsewhere.breakpoints.entries()[0].source,
        Path::new("D:/other/project")
            .join("src/main.rs")
            .display()
            .to_string()
    );
    config
        .breakpoints
        .insert("C:/Users/private/main.rs", 20)
        .unwrap();
    let mut file = SharedSet::default();
    file.upsert(SharedConfig::from_config(&config, workspace));
    assert!(SharedSet::from_json(&file.to_json().unwrap()).is_err());
}

/// Literal option arguments retain their boundaries while their path value becomes portable.
#[test]
fn shared_option_arguments_convert_project_paths_and_refuse_private_paths() {
    let workspace = Path::new("C:/work/project");
    let mut config = program("run-1", "portable");
    config.target = RunTarget::Program {
        program: "tool.exe".into(),
        args: vec!["--out=C:/work/project/data with spaces".into()],
    };
    let entry = SharedConfig::from_config(&config, workspace);
    assert_eq!(
        entry.target.arguments(),
        ["--out=${workspace}/data with spaces"]
    );
    let resolved = entry.resolve(Path::new("D:/other/project"), None);
    assert_eq!(
        resolved.target.arguments(),
        [format!(
            "--out={}",
            Path::new("D:/other/project")
                .join("data with spaces")
                .display()
        )]
    );
    for path in [
        "--out=C:/Users/private/data",
        "--out=../private",
        "--out=${unknown}/data",
    ] {
        let mut entry = SharedConfig::from_config(&config, workspace);
        entry.target = RunTarget::Program {
            program: "tool.exe".into(),
            args: vec![path.into()],
        };
        let mut file = SharedSet::default();
        file.upsert(entry);
        assert!(
            SharedSet::from_json(&file.to_json().unwrap()).is_err(),
            "{path}"
        );
    }
}

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
        breakpoints: Default::default(),
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

    // An external directory remains visible in the proposed entry, but the shared file validator
    // refuses it; it is never silently rewritten into a different working directory.
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
