//! Public discovery executes through the independently compiled SDK guest and real Manager.
use plugin_runtime::{
    HostResources, InstallControl, Manager, Package,
    plugin_protocol::{Environment, api, settings, ui::Kind},
};
use serde_json::{Value, json};
use std::{
    io::{Cursor, Write},
    path::Path,
};

/// Repackage the published component using only public declarations and guest-owned assets.
fn package(edit: impl FnOnce(&mut Value), assets: &[(&str, &[u8])]) -> Package {
    let archive = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/plugin-api-test/capability-example.zip");
    let mut files = Package::read(&archive).unwrap().files;
    let mut manifest: Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["api"]["required"]["workspace.files"] = json!(">=1.1, <2");
    manifest["api"]["optional"]["host.sdk"] = json!("^1");
    edit(&mut manifest);
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    for (name, bytes) in assets {
        files.insert((*name).into(), bytes.to_vec());
    }
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        archive
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&archive.finish().unwrap().into_inner()).unwrap()
}

/// Declaring dependency preparation makes installation execute the same hook as active LSP discovery.
fn hook_package(version: u32, policy: &str) -> Package {
    package(
        |manifest| {
            manifest["version"] = json!(format!("{version}.0.0"));
            manifest["settings_hook"] = json!(false);
            manifest["settings"]["label"]["default"] = json!("sdk-discovery");
            manifest["data_format"] = json!({"version":version,"migration_hook":true});
            for capability in [
                "language.lsp",
                "process",
                "dependencies",
                "storage.migration",
            ] {
                manifest["api"]["required"][capability] = json!("^1");
            }
            manifest["permissions"].as_array_mut().unwrap().extend([
                json!("process.service.analysis"),
                json!("dependencies.prepare"),
            ]);
            // These tests resolve an existing executable but never start a native process.
            manifest["services"] = json!({"analysis":{"program":std::env::current_exe().unwrap()}});
            manifest["language_servers"] = json!([{
                "id":"analysis", "language":"unfamiliar-discovery-language",
                "service":"analysis", "hook":true
            }]);
        },
        &[("migration-policy.txt", policy.as_bytes())],
    )
}

/// Distinguish root matches, nested matches, ignore files and explicitly excluded subtrees.
fn workspace() -> tempfile::TempDir {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir(workspace.path().join(".git")).unwrap();
    for (path, contents) in [
        (".gitignore", "hidden/\n"),
        (".ignore", "ignored.marker\n"),
        ("root.marker", "root"),
        ("nested/item.marker", "nested"),
        ("nested/.ignore", "*.marker\n!item.marker\n"),
        ("nested/blocked.marker", "nested ignored"),
        ("hidden/secret.marker", "git ignored"),
        ("ignored.marker", "ignored"),
        ("generated/skip.marker", "excluded"),
    ] {
        let path = workspace.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    workspace
}

/// Ordinary native paths expose whether returning SDK metadata accidentally expands file authority.
fn resources(sdk_root: &Path) -> HostResources {
    let config = sdk_root.join("cargo-config.toml");
    std::fs::write(&config, "sdk fixture configuration").unwrap();
    HostResources {
        sdk: Some(Ok(api::SdkDescriptor {
            digest: "fixture-sdk-contract-a".into(),
            root: sdk_root.display().to_string(),
            cargo_config: config.display().to_string(),
        })),
        ..Default::default()
    }
}

/// All guest incarnations belong to one Manager with one immutable resource snapshot.
fn manager(store: &Path, workspace: &Path, resources: HostResources) -> Manager {
    Manager::open_with_resources(
        store.into(),
        Environment {
            workspace: workspace.display().to_string(),
            ..Default::default()
        },
        true,
        resources,
    )
    .unwrap()
}

/// Read typed operation results from the guest's public native diagnostic view.
fn probe_for(
    manager: &mut Manager,
    id: &str,
    operation: api::Operation,
) -> Result<api::Value, api::Failure> {
    manager
        .invoke_command(id, "scope-probe", serde_json::to_value(operation).unwrap())
        .unwrap();
    let Kind::Text { text } = &manager.live[id].views["welcome"].as_ref().root.kind else {
        panic!("expected the guest's diagnostic view");
    };
    serde_json::from_str(text).unwrap()
}

/// Most scenarios use the original guest identity rather than a specialized host test path.
fn probe(manager: &mut Manager, operation: api::Operation) -> Result<api::Value, api::Failure> {
    probe_for(manager, "capability-example", operation)
}

/// Obtain actual host-issued handles instead of synthesizing supposedly valid resource identities.
fn open_workspace(manager: &mut Manager) -> api::ResourceHandle {
    let api::Value::Resource(handle) = probe(manager, api::Operation::OpenWorkspace).unwrap()
    else {
        panic!("workspace handle expected");
    };
    handle
}

/// Overlapping patterns deliberately exercise stable sorting and deduplication.
fn query() -> api::FileQuery {
    api::FileQuery {
        include: vec!["**/*.marker".into(), "nested/*.marker".into()],
        exclude: vec!["**/generated/**".into()],
        max_results: 32,
    }
}

/// Observe hook output through the prepared language service and prove temporary handles were retired.
fn assert_hook(manager: &mut Manager, sdk: &api::SdkDescriptor, paths: &[&str]) {
    let services = manager.language_services();
    let service = services["capability-example/analysis"]
        .as_ref()
        .unwrap_or_else(|error| panic!("discovery hook failed: {error}"));
    let result = &service.provider.initialization_options;
    assert_eq!(result["sdk"], serde_json::to_value(sdk).unwrap());
    assert_eq!(result["files"]["paths"], json!(paths));
    assert_eq!(result["files"]["skipped"], json!([]));
    let handle = serde_json::from_value(result["temporary_workspace"].clone()).unwrap();
    assert_eq!(
        probe(
            manager,
            api::Operation::FindFiles {
                handle,
                query: query()
            }
        )
        .unwrap_err()
        .code,
        api::ErrorCode::InvalidHandle,
        "proposal data cannot extend a hook handle's lifetime"
    );
}

/// Guest-selected globs stay within the workspace and honor ignore files and explicit exclusions.
#[test]
#[ignore = "build host and run scripts/verify-plugin-sdk.ps1 first"]
fn workspace_discovery_uses_only_allowed_files() {
    let store = tempfile::tempdir().unwrap();
    let workspace = workspace();
    // The excluded subtree exceeds the depth limit and must be pruned at its entrance.
    let mut excluded = workspace.path().join("generated");
    for _ in 0..66 {
        excluded.push("d");
    }
    std::fs::create_dir_all(&excluded).unwrap();
    std::fs::write(excluded.join("deep.marker"), "excluded before traversal").unwrap();
    let external = tempfile::tempdir().unwrap();
    std::fs::write(external.path().join("outside.marker"), "outside workspace").unwrap();
    #[cfg(windows)]
    let link = std::os::windows::fs::symlink_dir(external.path(), workspace.path().join("linked"));
    #[cfg(unix)]
    let link = std::os::unix::fs::symlink(external.path(), workspace.path().join("linked"));
    #[cfg(any(windows, unix))]
    if let Err(error) = link {
        // Windows may prohibit symlink creation; all other boundary assertions still run.
        eprintln!("Skipping external-link discovery assertion: cannot create symlink: {error}");
    }
    let package = package(|_| {}, &[]);
    let mut manager = manager(store.path(), workspace.path(), HostResources::default());
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let handle = open_workspace(&mut manager);
    let api::Value::Files(files) = probe(
        &mut manager,
        api::Operation::FindFiles {
            handle,
            query: query(),
        },
    )
    .expect("file discovery must be a supported public operation") else {
        panic!("file matches expected");
    };
    assert_eq!(files.paths, ["nested/item.marker", "root.marker"]);
    assert!(files.skipped.is_empty());
}

/// Invalid input and exceeded budgets are typed failures, never successful partial scans.
#[test]
#[ignore = "build host and run scripts/verify-plugin-sdk.ps1 first"]
fn invalid_globs_and_result_limits_leave_discovery_usable() {
    let store = tempfile::tempdir().unwrap();
    let workspace = workspace();
    let package = package(|_| {}, &[]);
    let mut manager = manager(store.path(), workspace.path(), HostResources::default());
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let handle = open_workspace(&mut manager);
    for pattern in [
        "",
        "../*.marker",
        "/root.marker",
        "C:/*.marker",
        "nested\\*.marker",
        "[",
    ] {
        let mut invalid = query();
        invalid.include = vec![pattern.into()];
        assert_eq!(
            probe(
                &mut manager,
                api::Operation::FindFiles {
                    handle: handle.clone(),
                    query: invalid,
                }
            )
            .unwrap_err()
            .code,
            api::ErrorCode::InvalidPath,
            "invalid pattern {pattern:?}"
        );
    }
    for (include, max_results, expected) in [
        (Vec::new(), 32, api::ErrorCode::InvalidRequest),
        (vec!["**/*.marker".into()], 0, api::ErrorCode::LimitExceeded),
        (
            vec!["**/*.marker".into()],
            4097,
            api::ErrorCode::LimitExceeded,
        ),
        (vec!["**/*.marker".into()], 1, api::ErrorCode::LimitExceeded),
        (
            vec!["**/*.marker".into(); 33],
            32,
            api::ErrorCode::LimitExceeded,
        ),
        (vec!["x".repeat(1025)], 32, api::ErrorCode::LimitExceeded),
    ] {
        let mut limited = query();
        limited.include = include;
        limited.max_results = max_results;
        assert_eq!(
            probe(
                &mut manager,
                api::Operation::FindFiles {
                    handle: handle.clone(),
                    query: limited,
                }
            )
            .unwrap_err()
            .code,
            expected
        );
    }
    let api::Value::Files(files) = probe(
        &mut manager,
        api::Operation::FindFiles {
            handle,
            query: query(),
        },
    )
    .unwrap() else {
        panic!("file matches expected");
    };
    assert_eq!(files.paths, ["nested/item.marker", "root.marker"]);
}

/// Discovery cannot borrow authority from private storage, another guest or a closed workspace root.
#[test]
#[ignore = "build host and run scripts/verify-plugin-sdk.ps1 first"]
fn discovery_rejects_private_foreign_released_and_retired_handles() {
    let store = tempfile::tempdir().unwrap();
    let workspace = workspace();
    let original = package(|_| {}, &[]);
    let peer = package(|manifest| manifest["id"] = json!("discovery-peer"), &[]);
    let mut manager = manager(store.path(), workspace.path(), HostResources::default());
    manager
        .install(&original, original.manifest.permissions.clone())
        .unwrap();
    manager
        .install(&peer, peer.manifest.permissions.clone())
        .unwrap();
    let api::Value::Resource(private) = probe(&mut manager, api::Operation::OpenData).unwrap()
    else {
        panic!("private-data handle expected");
    };
    let api::Value::Resource(foreign) = probe_for(
        &mut manager,
        "discovery-peer",
        api::Operation::OpenWorkspace,
    )
    .unwrap() else {
        panic!("peer workspace handle expected");
    };
    let released = open_workspace(&mut manager);
    probe(
        &mut manager,
        api::Operation::CloseResource {
            handle: released.clone(),
        },
    )
    .unwrap();
    for handle in [private, foreign, released] {
        assert_eq!(
            probe(
                &mut manager,
                api::Operation::FindFiles {
                    handle,
                    query: query()
                }
            )
            .unwrap_err()
            .code,
            api::ErrorCode::InvalidHandle
        );
    }
    let retired = open_workspace(&mut manager);
    manager.restart_plugin("capability-example").unwrap();
    assert_eq!(
        probe(
            &mut manager,
            api::Operation::FindFiles {
                handle: retired,
                query: query()
            }
        )
        .unwrap_err()
        .code,
        api::ErrorCode::InvalidHandle
    );
    assert!(matches!(
        probe(&mut manager, api::Operation::OpenWorkspace),
        Ok(api::Value::Resource(_))
    ));
}

/// Negotiation, workspace permission and SDK availability remain independent failure causes.
#[test]
#[ignore = "build host and run scripts/verify-plugin-sdk.ps1 first"]
fn discovery_requires_negotiation_and_preserves_sdk_failure_categories() {
    let workspace = workspace();
    let sdk_root = tempfile::tempdir().unwrap();
    let host = resources(sdk_root.path());
    let unavailable = package(
        |manifest| {
            manifest["api"]["required"]
                .as_object_mut()
                .unwrap()
                .remove("workspace.files");
            manifest["api"]["optional"]
                .as_object_mut()
                .unwrap()
                .remove("host.sdk");
        },
        &[],
    );
    let store = tempfile::tempdir().unwrap();
    let mut missing = manager(store.path(), workspace.path(), host.clone());
    missing
        .install(&unavailable, unavailable.manifest.permissions.clone())
        .unwrap();
    assert_eq!(
        probe(&mut missing, api::Operation::OpenWorkspace)
            .unwrap_err()
            .code,
        api::ErrorCode::CapabilityUnavailable
    );
    assert_eq!(
        probe(&mut missing, api::Operation::DescribeSdk)
            .unwrap_err()
            .code,
        api::ErrorCode::CapabilityUnavailable
    );
    let no_read = package(
        |manifest| {
            manifest["permissions"]
                .as_array_mut()
                .unwrap()
                .retain(|permission| permission != "workspace.read");
        },
        &[],
    );
    let store = tempfile::tempdir().unwrap();
    let mut denied = manager(store.path(), workspace.path(), host);
    denied
        .install(&no_read, no_read.manifest.permissions.clone())
        .unwrap();
    assert_eq!(
        probe(&mut denied, api::Operation::OpenWorkspace)
            .unwrap_err()
            .code,
        api::ErrorCode::PermissionDenied
    );
    assert!(
        matches!(
            probe(&mut denied, api::Operation::DescribeSdk),
            Ok(api::Value::Sdk(_))
        ),
        "SDK metadata requires no workspace permission"
    );
    let package = package(|_| {}, &[]);
    for (host, expected) in [
        (HostResources::default(), api::ErrorCode::NotFound),
        (
            HostResources {
                sdk: Some(Err("fixture export failed".into())),
                ..Default::default()
            },
            api::ErrorCode::OperationFailed,
        ),
    ] {
        let store = tempfile::tempdir().unwrap();
        let mut manager = manager(store.path(), workspace.path(), host);
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
        assert_eq!(
            probe(&mut manager, api::Operation::DescribeSdk)
                .unwrap_err()
                .code,
            expected
        );
        assert!(
            matches!(
                probe(&mut manager, api::Operation::OpenWorkspace),
                Ok(api::Value::Resource(_))
            ),
            "an unavailable SDK must not break unrelated workspace capability"
        );
    }
}

/// Host metadata survives normal owner replacement without expanding guest filesystem authority.
#[test]
#[ignore = "build host and run scripts/verify-plugin-sdk.ps1 first"]
fn sdk_resources_survive_preparation_settings_restart_workspace_and_reopen() {
    let store = tempfile::tempdir().unwrap();
    let workspace = workspace();
    let sdk_root = tempfile::tempdir().unwrap();
    let host = resources(sdk_root.path());
    let sdk = host.sdk.clone().unwrap().unwrap();
    let package = hook_package(1, "ok");
    let mut manager = manager(store.path(), workspace.path(), host.clone());
    let control = InstallControl::default();
    // Detached installation must run discovery before the candidate is activated or published.
    let prepared = manager
        .begin_installation(&package, package.manifest.permissions.clone(), &control)
        .unwrap()
        .run(&control)
        .expect("installation discovery must receive the host SDK");
    manager.commit_installation(prepared, &control).unwrap();
    assert_hook(&mut manager, &sdk, &["nested/item.marker", "root.marker"]);
    let handle = open_workspace(&mut manager);
    let relative_escape = format!(
        "../{}/cargo-config.toml",
        sdk_root.path().file_name().unwrap().to_string_lossy()
    );
    for path in [&sdk.cargo_config, &relative_escape] {
        assert_eq!(
            probe(
                &mut manager,
                api::Operation::ReadFile {
                    handle: handle.clone(),
                    path: path.clone()
                }
            )
            .unwrap_err()
            .code,
            api::ErrorCode::InvalidPath
        );
    }
    let api::Value::Bytes(bytes) = probe(
        &mut manager,
        api::Operation::ReadFile {
            handle,
            path: "root.marker".into(),
        },
    )
    .unwrap() else {
        panic!("workspace file bytes expected");
    };
    assert_eq!(bytes, b"root");
    manager
        .update_setting(
            "capability-example",
            settings::Scope::Project,
            "enabled",
            Some(json!(false)),
        )
        .unwrap();
    let api::Value::Sdk(actual) = probe(&mut manager, api::Operation::DescribeSdk).unwrap() else {
        panic!("SDK descriptor expected");
    };
    assert_eq!(actual, sdk);
    assert_hook(&mut manager, &sdk, &["nested/item.marker", "root.marker"]);
    manager.restart_plugin("capability-example").unwrap();
    assert_hook(&mut manager, &sdk, &["nested/item.marker", "root.marker"]);
    let other = tempfile::tempdir().unwrap();
    std::fs::write(other.path().join("other.marker"), "other").unwrap();
    let previous_workspace = open_workspace(&mut manager);
    manager
        .switch_workspace(
            Environment {
                workspace: other.path().display().to_string(),
                ..Default::default()
            },
            true,
        )
        .unwrap();
    assert_eq!(
        probe(
            &mut manager,
            api::Operation::FindFiles {
                handle: previous_workspace,
                query: query(),
            },
        )
        .unwrap_err()
        .code,
        api::ErrorCode::InvalidHandle,
    );
    assert_hook(&mut manager, &sdk, &["other.marker"]);
    manager
        .switch_workspace(
            Environment {
                workspace: workspace.path().display().to_string(),
                ..Default::default()
            },
            true,
        )
        .unwrap();
    assert_hook(&mut manager, &sdk, &["nested/item.marker", "root.marker"]);
    assert!(!workspace.path().join("hook-must-not-write.marker").exists());
    assert!(!other.path().join("hook-must-not-write.marker").exists());
    drop(manager);
    let mut reopened = self::manager(store.path(), workspace.path(), host);
    assert_hook(&mut reopened, &sdk, &["nested/item.marker", "root.marker"]);
}

/// Failed activation after migration restores a fresh SDK-aware owner and leaves committed data intact.
#[test]
#[ignore = "build host and run scripts/verify-plugin-sdk.ps1 first"]
fn sdk_resources_survive_migration_rollback_without_committing_candidate_data() {
    let store = tempfile::tempdir().unwrap();
    let workspace = workspace();
    let sdk_root = tempfile::tempdir().unwrap();
    let host = resources(sdk_root.path());
    let sdk = host.sdk.clone().unwrap().unwrap();
    let old = hook_package(1, "ok");
    let mut manager = manager(store.path(), workspace.path(), host);
    manager
        .install(&old, old.manifest.permissions.clone())
        .unwrap();
    let api::Value::Resource(data) = probe(&mut manager, api::Operation::OpenData).unwrap() else {
        panic!("private-data handle expected");
    };
    probe(
        &mut manager,
        api::Operation::WriteFile {
            handle: data,
            path: "value.txt".into(),
            bytes: b"original".to_vec(),
        },
    )
    .unwrap();
    let previous = manager
        .instance_id("capability-example")
        .unwrap()
        .to_owned();
    let candidate = hook_package(2, "activate-fail");
    let control = InstallControl::default();
    let prepared = manager
        .prepare_installation(&candidate, candidate.manifest.permissions.clone(), &control)
        .unwrap();
    let error = manager.commit_installation(prepared, &control).unwrap_err();
    // Instance diagnostics add context; assert the typed guest failure beneath that context.
    let failure = error
        .downcast_ref::<api::Failure>()
        .unwrap_or_else(|| panic!("expected a typed activation failure: {error:#}"));
    assert_eq!(
        (failure.code, failure.message.as_str()),
        (api::ErrorCode::OperationFailed, "Fixture activation failed"),
        "unexpected migration rollback error: {error:#}"
    );
    assert_eq!(manager.installed["capability-example"].digest, old.digest);
    assert_ne!(
        manager.instance_id("capability-example"),
        Some(previous.as_str())
    );
    assert_hook(&mut manager, &sdk, &["nested/item.marker", "root.marker"]);
    let api::Value::Resource(data) = probe(&mut manager, api::Operation::OpenData).unwrap() else {
        panic!("private-data handle expected");
    };
    let api::Value::Bytes(bytes) = probe(
        &mut manager,
        api::Operation::ReadFile {
            handle: data,
            path: "value.txt".into(),
        },
    )
    .unwrap() else {
        panic!("private-data bytes expected");
    };
    assert_eq!(bytes, b"original");
}
