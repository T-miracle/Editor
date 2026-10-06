//! Historical installation dumps recover through public lifecycle APIs and an independently built SDK guest.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, Snapshot, settings::Source, ui::Kind},
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const OLD_ID: &str = "me.capability-example";
const ID: &str = "capability-example";

/// Fixture paths preserve the historical raw workspace spelling used by the previous snapshot writer.
struct LegacyInstallation {
    directory: tempfile::TempDir,
    root: PathBuf,
    first: PathBuf,
    second: PathBuf,
    registry: Vec<u8>,
    settings: Vec<u8>,
    snapshots: [(String, Vec<u8>); 2],
}

impl LegacyInstallation {
    /// Write only the old persisted format; a missing old component makes attempted execution observable.
    fn new(package: &Package) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("plugins");
        let first = directory.path().join("project-a");
        let second = directory.path().join("project-b");
        for workspace in [&first, &second] {
            std::fs::create_dir_all(workspace).unwrap();
            std::fs::write(workspace.join("source.txt"), "workspace").unwrap();
        }
        let data = root.join("data").join(OLD_ID);
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(root.join("settings")).unwrap();
        let mut manifest = serde_json::to_value(&package.manifest).unwrap();
        manifest["id"] = json!(OLD_ID);
        manifest["protocol"] = json!(6);
        manifest["api"] = json!(null);
        manifest["version"] = json!("0.0.6");
        manifest["component"] = json!("must-not-load-missing-legacy.wasm");
        // Historical serializers wrote these optional fields even when the command had no toolbar.
        // Keep the actual old wire shape so the earlier data importer also exercises UI-contract retirement.
        for command in manifest["commands"].as_array_mut().unwrap() {
            command["toolbar"] = serde_json::Value::Null;
            command["toolbar_icon"] = serde_json::Value::Null;
        }
        let registry = serde_json::to_vec_pretty(&json!({OLD_ID:{
            "manifest":manifest, "digest":"6666666666666666666666666666666666666666666666666666666666666666",
            "grants":package.manifest.permissions, "enabled":false,
            "project_enabled":[first.display().to_string(),second.display().to_string()]
        }})).unwrap();
        std::fs::write(root.join("registry.json"), &registry).unwrap();
        let project_key = format!(
            "{:x}",
            Sha256::digest(first.canonicalize().unwrap().to_string_lossy().as_bytes())
        );
        let settings = serde_json::to_vec_pretty(&json!({
            "user":{"enabled":false,"label":"historic user label"},
            "projects":{project_key:{"label":"project A label"}}
        }))
        .unwrap();
        std::fs::write(
            root.join("settings").join(format!("{OLD_ID}.json")),
            &settings,
        )
        .unwrap();
        std::fs::write(data.join("value.txt"), b"legacy private data").unwrap();
        std::fs::write(data.join("state-preferences.json"), b"business settings").unwrap();
        // A business file sharing the settings name is opaque and must never enter the host settings parser.
        std::fs::write(data.join("settings.json"), b"\0opaque plugin settings\xff").unwrap();
        let snapshots = [(&first, "opaque A: \u{2603}"), (&second, "opaque B: \0")].map(
            |(workspace, value)| {
                let hash = format!(
                    "{:x}",
                    Sha256::digest(workspace.display().to_string().as_bytes())
                );
                let name = format!("state-{}.json", &hash[..16]);
                let bytes = serde_json::to_vec_pretty(&Snapshot {
                    schema: 27,
                    data: value.into(),
                })
                .unwrap();
                std::fs::write(data.join(&name), &bytes).unwrap();
                (name, bytes)
            },
        );
        Self {
            directory,
            root,
            first,
            second,
            registry,
            settings,
            snapshots,
        }
    }

    /// Reopening crosses the public activation gate, never an internal migration helper.
    fn open(&self, workspace: &Path) -> Manager {
        Manager::open(self.root.clone(), environment(workspace)).unwrap()
    }

    /// Recovery evidence must retain the exact original bytes after successful imports and later writes.
    fn assert_backups(&self) {
        let backup = self.root.join("legacy-backup/v7");
        assert_eq!(
            std::fs::read(backup.join("registry.json")).unwrap(),
            self.registry
        );
        let plugin = backup.join("plugins").join(ID);
        assert_eq!(
            std::fs::read(plugin.join("settings.json")).unwrap(),
            self.settings
        );
        for data in [plugin.join("data"), self.root.join("data").join(OLD_ID)] {
            assert_eq!(
                std::fs::read(data.join("value.txt")).unwrap(),
                b"legacy private data"
            );
            assert_eq!(
                std::fs::read(data.join("settings.json")).unwrap(),
                b"\0opaque plugin settings\xff"
            );
            for (name, bytes) in &self.snapshots {
                assert_eq!(&std::fs::read(data.join(name)).unwrap(), bytes);
            }
        }
        // The input directory remains alive throughout all manager reopen operations.
        assert!(self.directory.path().exists());
    }
}

/// Use the exact host environment value that the historical writer hashed.
fn environment(workspace: &Path) -> Environment {
    Environment {
        workspace: workspace.display().to_string(),
        ..Default::default()
    }
}

/// The public guest command proves private files are available to the new capability API.
fn private_text(manager: &mut Manager) -> String {
    manager
        .invoke_command(ID, "scope-read", json!(null))
        .unwrap();
    let Kind::Text { text } = &manager.live[ID]
        .views
        .values()
        .next()
        .unwrap()
        .as_ref()
        .root
        .kind
    else {
        panic!("expected the SDK guest's native text publication");
    };
    text.clone()
}

/// Opaque snapshots round-trip through guest Prepare/Snapshot, without inspecting host-private candidate paths.
fn assert_snapshot(manager: &mut Manager, expected: &str) {
    let snapshot = manager.live.get_mut(ID).unwrap().snapshot().unwrap();
    assert_eq!(snapshot.schema, 27);
    assert_eq!(snapshot.data, expected);
}

/// A compatible reinstall restores two independent scopes once while retaining preferences and recovery evidence.
#[test]
#[ignore = "build the current independent capability-example SDK guest first"]
fn compatible_reinstall_imports_legacy_scopes_once_and_keeps_recoverable_bytes() {
    let package = Package::read(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
    )
    .unwrap();
    let legacy = LegacyInstallation::new(&package);
    let mut manager = legacy.open(&legacy.first);
    let entry = &manager.installed[ID];
    let reason = entry
        .compatibility_error()
        .expect("legacy API needs an explicit update explanation");
    assert_eq!(
        entry.error.as_deref(),
        Some(reason.as_str()),
        "reject before attempting to load the missing component"
    );
    assert!(!entry.enabled);
    assert!(entry.project_enabled_in(&legacy.first.display().to_string()));
    assert!(entry.project_enabled_in(&legacy.second.display().to_string()));
    assert_eq!(entry.grants, package.manifest.permissions);
    assert!(manager.live.is_empty());
    assert_eq!(manager.resource_count(), 0);
    legacy.assert_backups();
    drop(manager);
    manager = legacy.open(&legacy.first);
    assert!(
        manager.live.is_empty(),
        "reopening cannot bypass the compatibility gate"
    );
    legacy.assert_backups();

    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    assert!(
        !manager.installed[ID].enabled,
        "reinstall preserves the global disabled preference"
    );
    assert!(manager.installed[ID].compatibility_error().is_none());
    assert_eq!(private_text(&mut manager), "workspace|legacy private data");
    assert_eq!(
        std::fs::read(manager.data_directory(ID).join("state-preferences.json")).unwrap(),
        b"business settings"
    );
    assert_snapshot(&mut manager, "opaque A: \u{2603}");
    let settings = manager.effective_settings(ID).unwrap();
    assert_eq!(settings["label"].value, "project A label");
    assert_eq!(settings["label"].source, Source::Project);
    assert_eq!(settings["enabled"].value, false);
    assert_eq!(
        std::fs::read(manager.data_directory(ID).join("settings.json")).unwrap(),
        b"\0opaque plugin settings\xff"
    );
    manager
        .invoke_command(ID, "scope-write", json!({"text":"new A data"}))
        .unwrap();

    // The globally replaced package still imports each previously enabled workspace on its first activation.
    // Parking A must not prevent the already installed version from initializing an independent B scope.
    manager
        .switch_workspace(environment(&legacy.second), true)
        .unwrap();
    assert_eq!(private_text(&mut manager), "workspace|legacy private data");
    assert_snapshot(&mut manager, "opaque B: \0");
    let settings = manager.effective_settings(ID).unwrap();
    assert_eq!(settings["label"].value, "historic user label");
    assert_eq!(settings["label"].source, Source::User);
    manager
        .invoke_command(ID, "scope-write", json!({"text":"new B data"}))
        .unwrap();
    manager
        .close_workspace(&legacy.second.display().to_string())
        .unwrap();
    manager
        .close_workspace(&legacy.first.display().to_string())
        .unwrap();
    manager
        .switch_workspace(environment(&legacy.first), true)
        .unwrap();
    assert_eq!(private_text(&mut manager), "workspace|new A data");
    assert_snapshot(&mut manager, "opaque A: \u{2603}");
    drop(manager);

    // Neither a host restart nor a later scope activation may replay the original flat private files.
    let mut manager = legacy.open(&legacy.second);
    assert!(!manager.installed[ID].enabled);
    assert_eq!(private_text(&mut manager), "workspace|new B data");
    assert_snapshot(&mut manager, "opaque B: \0");
    legacy.assert_backups();

    // Explicit data deletion also retires historical recovery data, so reinstall cannot resurrect it.
    manager.uninstall(ID, true).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    assert!(!manager.data_directory(ID).join("value.txt").exists());
    assert!(
        manager
            .live
            .get_mut(ID)
            .unwrap()
            .snapshot()
            .unwrap()
            .data
            .is_empty()
    );
    assert!(!legacy.root.join("data").join(OLD_ID).exists());
    assert!(
        !legacy
            .root
            .join("settings")
            .join(format!("{OLD_ID}.json"))
            .exists()
    );
}
