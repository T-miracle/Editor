//! Private-data migrations are verified through independently built packages and the public manager.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, ui::Kind},
};
use serde_json::json;
use std::io::{Cursor, Write};

/// Every version uses the same SDK fixture; only the package's declaration and migration policy differ.
fn package(version: u32, behavior: &str) -> Package {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/plugin-api-test/capability-example.zip");
    let mut files = Package::read(&source).unwrap().files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["version"] = json!(format!("{version}.0.0"));
    manifest["data_format"] = json!({"version":version,"migration_hook":true});
    manifest["api"]["required"]["storage.migration"] = json!("^1");
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    files.insert("migration-policy.txt".into(), behavior.as_bytes().to_vec());
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&zip.finish().unwrap().into_inner()).unwrap()
}

/// The hook converts opaque private content while retaining the workspace's original source file.
#[test]
#[ignore = "build the independent capability-example SDK guest first"]
fn upgrades_private_data_on_an_isolated_copy() {
    let root = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("source.txt"), "workspace").unwrap();
    // A metadata-only manager predates later commits; first ownership must refresh its cached registry.
    let mut waiting = Manager::open(
        root.path().into(),
        Environment {
            workspace: workspace.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let mut manager = Manager::open(
        root.path().into(),
        Environment {
            workspace: workspace.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let old = package(1, "ok");
    manager
        .install(&old, old.manifest.permissions.clone())
        .unwrap();
    manager
        .invoke_command(
            "capability-example",
            "scope-write",
            json!({"text":"latest"}),
        )
        .unwrap();
    let new = package(2, "ok");
    let control = plugin_runtime::InstallControl::default();
    let prepared = manager
        .prepare_installation(&new, new.manifest.permissions.clone(), &control)
        .unwrap();
    // Preparation does not retire the old guest; its later write must win over the initial copy.
    manager
        .invoke_command("capability-example", "scope-write", json!({"text":"later"}))
        .unwrap();
    assert!(
        Manager::read_registry(root.path()).is_err(),
        "Metadata readers cannot bypass an active data transaction"
    );
    manager.commit_installation(prepared, &control).unwrap();
    manager
        .invoke_command("capability-example", "scope-read", json!(null))
        .unwrap();
    let Kind::Text { text } = &manager.live["capability-example"]
        .views
        .values()
        .next()
        .unwrap()
        .as_ref()
        .root
        .kind
    else {
        panic!("text expected")
    };
    assert_eq!(text, "workspace|v2:later");
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("source.txt")).unwrap(),
        "workspace"
    );
    manager.disable("capability-example").unwrap();
    drop(manager);
    let latest = package(3, "ok");
    waiting
        .install(&latest, latest.manifest.permissions.clone())
        .unwrap();
    assert!(
        !waiting.installed["capability-example"].enabled,
        "Acquiring ownership must refresh the last committed enablement choice"
    );
    waiting.enable("capability-example").unwrap();
    assert_eq!(private_text(&mut waiting), "workspace|v3:v2:later");
}

/// Observe data through the guest's public command rather than depending on migration directory layout.
fn private_text(manager: &mut Manager) -> String {
    manager
        .invoke_command("capability-example", "scope-read", json!(null))
        .unwrap();
    let Kind::Text { text } = &manager.live["capability-example"]
        .views
        .values()
        .next()
        .unwrap()
        .as_ref()
        .root
        .kind
    else {
        panic!("text expected")
    };
    text.clone()
}

/// Hook, activation and registry failures all retain the old package and make its original data usable again.
#[test]
#[ignore = "build the independent capability-example SDK guest first"]
fn failed_migration_activation_and_commit_preserve_old_data() {
    let root = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("source.txt"), "workspace").unwrap();
    let mut manager = Manager::open(
        root.path().into(),
        Environment {
            workspace: workspace.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    // A first activation failure must not publish an installed record or live instance.
    let failed = package(1, "activate-fail");
    assert!(
        manager
            .install(&failed, failed.manifest.permissions.clone())
            .is_err()
    );
    assert!(manager.installed.is_empty() && manager.live.is_empty());
    let old = package(1, "ok");
    manager
        .install(&old, old.manifest.permissions.clone())
        .unwrap();
    manager
        .invoke_command(
            "capability-example",
            "scope-write",
            json!({"text":"original"}),
        )
        .unwrap();
    for policy in ["fail", "activate-fail"] {
        let failed = package(2, policy);
        assert!(
            manager
                .install(&failed, failed.manifest.permissions.clone())
                .is_err()
        );
        assert_eq!(
            manager.installed["capability-example"].manifest.version,
            "1.0.0"
        );
        assert_eq!(private_text(&mut manager), "workspace|original");
    }
    let update = package(2, "ok");
    // Cancellation at the durable commit boundary exercises a directory swap that must be rolled back.
    let control_slot = std::sync::Arc::new(std::sync::Mutex::new(
        None::<plugin_runtime::InstallControl>,
    ));
    let slot = control_slot.clone();
    let competing_root = root.path().to_owned();
    let competing_workspace = workspace.path().display().to_string();
    let control = plugin_runtime::InstallControl::new(move |stage| {
        if stage == plugin_runtime::InstallStage::Committing {
            assert!(
                Manager::open(
                    competing_root.clone(),
                    Environment {
                        workspace: competing_workspace.clone(),
                        ..Default::default()
                    }
                )
                .is_err(),
                "Another runtime must not activate an old package against newly swapped data"
            );
            slot.lock().unwrap().as_ref().unwrap().cancel();
        }
    });
    *control_slot.lock().unwrap() = Some(control.clone());
    assert!(
        manager
            .install_with_control(&update, update.manifest.permissions.clone(), &control)
            .is_err()
    );
    control_slot.lock().unwrap().take();
    assert_eq!(
        manager.installed["capability-example"].manifest.version,
        "1.0.0"
    );
    assert_eq!(private_text(&mut manager), "workspace|original");
    drop(manager);
    let mut reopened = Manager::open(
        root.path().into(),
        Environment {
            workspace: workspace.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(private_text(&mut reopened), "workspace|original");
}

/// A subprocess exits at a real public progress boundary, bypassing destructors like an interrupted host.
#[test]
#[ignore = "subprocess fixture invoked by interrupted_commits_recover"]
fn crash_child() {
    let Ok(root) = std::env::var("EDITOR_MIGRATION_TEST_ROOT") else {
        return;
    };
    let workspace = std::env::var("EDITOR_MIGRATION_TEST_WORKSPACE").unwrap();
    let phase = std::env::var("EDITOR_MIGRATION_TEST_PHASE").unwrap();
    let mut manager = Manager::open(
        root.into(),
        Environment {
            workspace,
            ..Default::default()
        },
    )
    .unwrap();
    let update = package(2, "ok");
    let control = plugin_runtime::InstallControl::new(move |stage| {
        if (phase == "pending" && stage == plugin_runtime::InstallStage::Committing)
            || (phase == "committed" && stage == plugin_runtime::InstallStage::Committed)
        {
            std::process::exit(73);
        }
    });
    manager
        .install_with_control(&update, update.manifest.permissions.clone(), &control)
        .unwrap();
    panic!("fixture did not interrupt the host");
}

/// Startup either rolls back an uncommitted scope or finishes committed cleanup; recovery failures retain evidence.
#[test]
#[ignore = "build the independent capability-example SDK guest first"]
fn interrupted_commits_recover_before_any_guest_is_activated() {
    for phase in ["pending", "committed"] {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join("source.txt"), "workspace").unwrap();
        let environment = Environment {
            workspace: workspace.path().display().to_string(),
            ..Default::default()
        };
        let mut manager = Manager::open(root.path().into(), environment.clone()).unwrap();
        let old = package(1, "ok");
        manager
            .install(&old, old.manifest.permissions.clone())
            .unwrap();
        manager
            .invoke_command(
                "capability-example",
                "scope-write",
                json!({"text":"durable"}),
            )
            .unwrap();
        let scope = manager
            .data_directory("capability-example")
            .parent()
            .unwrap()
            .to_owned();
        drop(manager);
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "crash_child", "--ignored"])
            .env("EDITOR_MIGRATION_TEST_ROOT", root.path())
            .env("EDITOR_MIGRATION_TEST_WORKSPACE", workspace.path())
            .env("EDITOR_MIGRATION_TEST_PHASE", phase)
            .output()
            .unwrap();
        assert_eq!(
            child.status.code(),
            Some(73),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        if phase == "pending" {
            // Real I/O failure during restoration must stop startup, retaining data for the next retry.
            std::fs::rename(
                root.path().join("registry.json"),
                root.path().join("saved-registry.json"),
            )
            .unwrap();
            std::fs::create_dir(root.path().join("registry.json")).unwrap();
            assert!(Manager::open(root.path().into(), environment.clone()).is_err());
            std::fs::remove_dir(root.path().join("registry.json")).unwrap();
            std::fs::rename(
                root.path().join("saved-registry.json"),
                root.path().join("registry.json"),
            )
            .unwrap();
        } else {
            // A committed marker cannot justify deleting the backup when the new data directory is unavailable.
            let retained = root.path().join("retained-scope");
            std::fs::rename(&scope, &retained).unwrap();
            assert!(Manager::open(root.path().into(), environment.clone()).is_err());
            std::fs::rename(&retained, &scope).unwrap();
        }
        let mut manager = Manager::open(root.path().into(), environment).unwrap();
        assert_eq!(
            manager.installed["capability-example"].manifest.version,
            if phase == "pending" { "1.0.0" } else { "2.0.0" }
        );
        assert_eq!(
            private_text(&mut manager),
            if phase == "pending" {
                "workspace|durable"
            } else {
                "workspace|v2:durable"
            }
        );
    }
}

/// A globally disabled plugin still migrates and runs when explicitly enabled in an older workspace scope.
#[test]
#[ignore = "build the independent capability-example SDK guest first"]
fn enabling_a_dormant_workspace_migrates_and_activates_its_own_data() {
    let root = tempfile::tempdir().unwrap();
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    for path in [a.path(), b.path()] {
        std::fs::write(path.join("source.txt"), "workspace").unwrap();
    }
    let environment = |path: &std::path::Path| Environment {
        workspace: path.display().to_string(),
        ..Default::default()
    };
    let mut manager = Manager::open(root.path().into(), environment(b.path())).unwrap();
    let old = package(1, "ok");
    manager
        .install(&old, old.manifest.permissions.clone())
        .unwrap();
    manager
        .invoke_command(
            "capability-example",
            "scope-write",
            json!({"text":"dormant"}),
        )
        .unwrap();
    manager
        .close_workspace(&b.path().display().to_string())
        .unwrap();
    manager
        .switch_workspace(environment(a.path()), true)
        .unwrap();
    manager
        .invoke_command(
            "capability-example",
            "scope-write",
            json!({"text":"active"}),
        )
        .unwrap();
    let new = package(2, "ok");
    manager
        .install(&new, new.manifest.permissions.clone())
        .unwrap();
    manager.disable("capability-example").unwrap();
    manager
        .switch_workspace(environment(b.path()), true)
        .unwrap();
    manager
        .set_project_enabled("capability-example", true)
        .unwrap();
    assert!(!manager.installed["capability-example"].enabled);
    assert_eq!(private_text(&mut manager), "workspace|v2:dormant");
}
