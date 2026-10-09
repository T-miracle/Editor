//! Upgrade enters through the same offline importer as application startup, with isolated real files.
use super::*;
use crate::terminal::{
    Settings,
    engine::{Engine, GridSize},
};
use plugin_runtime::{
    Installed, Manager,
    plugin_protocol::{self as protocol},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// The last retired release is data-only here: migration never needs its WASM executable.
fn legacy(runtime: &Path, workspace: &Path) -> std::path::PathBuf {
    let manifest:protocol::Manifest=serde_json::from_value(json!({"id":"terminal","name":"Terminal","version":"0.12.2","protocol":7,
        "api":{"base":"^1"},"storage_limit":8388608,"component":"terminal.wasm","permissions":[],"panels":[{"id":"terminal","title":"Terminal","position":"bottom"}]})).unwrap();
    let entry = Installed {
        manifest: manifest.clone(),
        digest: "old-release".into(),
        grants: BTreeSet::new(),
        enabled: true,
        project_enabled: BTreeSet::from([workspace.display().to_string()]),
        retired_ui_contract: false,
        global_enabled: None,
        error: None,
    };
    std::fs::create_dir_all(runtime).unwrap();
    std::fs::write(
        runtime.join("registry.json"),
        serde_json::to_vec(&BTreeMap::from([("terminal", entry)])).unwrap(),
    )
    .unwrap();
    // Resolve through the actual runtime rather than repeating its canonical workspace hashing.
    let data =
        Manager::persisted_data_directory(runtime, &manifest, &workspace.display().to_string())
            .unwrap();
    std::fs::create_dir_all(&data).unwrap();
    data
}
/// Styled old output includes blank viewport rows; they must not turn into prompt gaps after reflow.
fn snapshot(data: &Path) -> Vec<u8> {
    let mut settings = Settings::default();
    settings.history = 10;
    settings.theme.foreground = Some("#112233".into());
    let saved = json!({"tabs":[{"id":7,"name":"my-shell","profile":settings.profiles[0],"cwd":"C:\\work","output":"\u{1b}[0mPS C:\\work> \r\n\u{1b}[0m\r\n\u{1b}[0m\r\n\u{1b}[0m","display":{"rows":4,"columns":80,"cursor":[0,12],"wrap_pending":false,"scrollback":0,"wrapped_lines":[],"soft_wraps":true}},
        {"id":8,"name":"old-run","profile":{"name":"Service execution","program":"do-not-replay.exe","args":[]},"cwd":"C:\\work","output":"OLD TASK OUTPUT","exited":true}],"active":0,"next_id":8,"settings":settings,"tab_width":222.,"recovery_version":1});
    let source = serde_json::to_vec(&protocol::Snapshot {
        schema: 2,
        data: serde_json::to_string(&saved).unwrap(),
    })
    .unwrap();
    std::fs::write(data.parent().unwrap().join("state.json"), &source).unwrap();
    source
}
/// Names, theme, cursor and stopped tasks commit together, and repeat startup never overwrites new state.
#[test]
fn workspace_upgrade_commits_once_and_keeps_other_scopes() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("project");
    std::fs::create_dir(&workspace).unwrap();
    let runtime = root.path().join("plugins");
    let native = root.path().join("native");
    let runs = root.path().join("runs");
    let session = root.path().join("layout.json");
    let data = legacy(&runtime, &workspace);
    let source = snapshot(&data);
    perform(&runtime, &workspace, &native, &runs, &session).unwrap();
    let state: Value =
        serde_json::from_slice(&std::fs::read(native.join("state.json")).unwrap()).unwrap();
    assert_eq!(state["sessions"][0]["name"], "my-shell");
    assert_eq!(state["settings"]["theme"]["foreground"], "#112233");
    assert_eq!(state["tab_width"], 222.);
    assert!(state["sessions"][1]["task"].is_object());
    assert_eq!(state["sessions"][1]["exited"], true);
    let grid = serde_json::from_value(state["sessions"][0]["grid"].clone()).unwrap();
    let mut engine = Engine::new(
        GridSize {
            columns: 80,
            rows: 4,
        },
        10,
    );
    engine.restore(grid).unwrap();
    assert_eq!(engine.snapshot().cursor, (0, 12));
    assert_eq!(
        engine
            .snapshot()
            .lines
            .iter()
            .filter(|line| line.iter().any(|cell| cell.c != ' '))
            .count(),
        1
    );
    assert_eq!(
        std::fs::read(data.parent().unwrap().join("state.json")).unwrap(),
        source
    );
    assert!(
        !Manager::read_registry(&runtime)
            .unwrap()
            .contains_key("terminal")
    );
    std::fs::write(native.join("state.json"), b"new user state").unwrap();
    perform(&runtime, &workspace, &native, &runs, &session).unwrap();
    assert_eq!(
        std::fs::read(native.join("state.json")).unwrap(),
        b"new user state"
    );
    let other = root.path().join("other");
    std::fs::create_dir(&other).unwrap();
    let other_native = root.path().join("other-native");
    perform(
        &runtime,
        &other,
        &other_native,
        &runs,
        &root.path().join("other-layout.json"),
    )
    .unwrap();
    assert!(
        !other_native.join("state.json").exists(),
        "A's data must never import into B"
    );
}
/// Damaged source is preserved and disabled before startup; repairing it permits a bounded retry.
#[test]
fn corrupt_upgrade_keeps_source_and_can_retry() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("project");
    std::fs::create_dir(&workspace).unwrap();
    let runtime = root.path().join("plugins");
    let native = root.path().join("native");
    let runs = root.path().join("runs");
    let session = root.path().join("layout.json");
    let data = legacy(&runtime, &workspace);
    let path = data.parent().unwrap().join("state.json");
    std::fs::write(&path, b"damaged").unwrap();
    assert!(perform(&runtime, &workspace, &native, &runs, &session).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"damaged");
    assert!(!native.join("state.json").exists());
    assert!(
        !Manager::read_registry(&runtime)
            .unwrap()
            .contains_key("terminal")
    );
    let archived: BTreeMap<String, Installed> = serde_json::from_slice(
        &std::fs::read(runtime.join("builtin-terminal-backup/installation.json")).unwrap(),
    )
    .unwrap();
    assert!(
        archived["terminal"].enabled,
        "original metadata stays recoverable"
    );
    snapshot(&data);
    perform(&runtime, &workspace, &native, &runs, &session).unwrap();
    assert!(native.join("state.json").exists());
}

/// A crash after the first rename resumes remaining targets; an intervening user edit is a conflict.
#[test]
fn interrupted_upgrade_resumes_and_rejects_newer_data() {
    let root = tempfile::tempdir().unwrap();
    let native = root.path().join("native");
    let runtime = root.path().join("runtime");
    let configurations = root.path().join("run.json");
    let layout = root.path().join("layout.json");
    let locations = storage::Locations {
        native: &native,
        configurations: &configurations,
        layout: &layout,
        runtime: &runtime,
    };
    let mut plan = storage::Plan::default();
    plan.add(
        &locations,
        storage::Kind::State,
        b"converted cells".to_vec(),
    )
    .unwrap();
    plan.add(
        &locations,
        storage::Kind::Layout,
        b"converted layout".to_vec(),
    )
    .unwrap();
    plan.stage(&native).unwrap();
    // Durable state models the exact interruption point: the first target committed, the second failed.
    storage::atomic(&native.join("state.json"), b"converted cells").unwrap();
    std::fs::create_dir(&layout).unwrap();
    assert!(perform(&runtime, root.path(), &native, root.path(), &layout).is_err());
    assert!(native.join("upgrade-pending.json").exists());
    assert!(!native.join("upgrade-complete.json").exists());
    std::fs::remove_dir(&layout).unwrap();
    perform(&runtime, root.path(), &native, root.path(), &layout).unwrap();
    assert_eq!(std::fs::read(&layout).unwrap(), b"converted layout");
    assert!(!native.join("upgrade-pending.json").exists());
    // A conflict belongs to an unfinished migration, before any completion receipt was written.
    std::fs::remove_file(native.join("upgrade-complete.json")).unwrap();
    let mut plan = storage::Plan::default();
    plan.add(
        &locations,
        storage::Kind::Layout,
        b"second migration".to_vec(),
    )
    .unwrap();
    plan.stage(&native).unwrap();
    std::fs::write(&layout, b"newer user edit").unwrap();
    assert!(perform(&runtime, root.path(), &native, root.path(), &layout).is_err());
    assert_eq!(std::fs::read(&layout).unwrap(), b"newer user edit");
}

/// Existing Shell form values, IDs and independent provider choices survive the finite cutover.
#[test]
fn upgrade_preserves_configurations_layout_and_unrelated_plugins() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("project");
    std::fs::create_dir(&workspace).unwrap();
    let runtime = root.path().join("plugins");
    let native = root.path().join("native");
    let runs = root.path().join("runs");
    let session = root.path().join("layout.json");
    let data = legacy(&runtime, &workspace);
    snapshot(&data);
    let mut registry = Manager::read_registry(&runtime).unwrap();
    let mut peer = registry["terminal"].clone();
    peer.manifest.id = "unrelated".into();
    peer.enabled = false;
    registry.insert("unrelated".into(), peer);
    std::fs::write(
        runtime.join("registry.json"),
        serde_json::to_vec(&registry).unwrap(),
    )
    .unwrap();
    let mut fields = protocol::configurations::command_form::Fields::new(
        "Restored script",
        vec!["-NoProfile".into(), "-Command".into()],
    );
    fields.script = Some("Write-Output migrated-command".into());
    let values = serde_json::to_string(&json!({"shell":"PowerShell","fields":fields})).unwrap();
    let set=editor_core::RunConfigSet::from_json(&serde_json::to_vec(&json!({"version":2,"configurations":[{"id":"shell-script","name":"Restored script","provider":"me.terminal","target":{"mode":"program","program":"powershell.exe"}}],"plugin_configurations":{"shell-script":{"provider":"terminal","template":"PowerShell","values":values,"name":"Restored script","program":"powershell.exe","revision":4}}})).unwrap()).unwrap();
    editor_core::save(&runs, &workspace.display().to_string(), &set).unwrap();
    let mut layout =
        serde_json::to_value(crate::app::session::SessionState::for_workspace(&workspace)).unwrap();
    layout["plugin_panel_visibility"] = json!({"terminal/terminal":true,"unrelated/panel":false});
    layout["plugin_dock_sizes"] = json!({"bottom":310});
    layout["dock_layout"] = json!({"center":{"panel_name":"Editor"},"bottom_dock":{"size":310,"panel":{"panel_name":"plugin:me.terminal/terminal"}}});
    std::fs::write(&session, serde_json::to_vec(&layout).unwrap()).unwrap();
    std::fs::write(runtime.join("service-providers.json"),serde_json::to_vec(&json!({"user":{"workspace/interactive.execute":"me.terminal","workspace/example.echo":"unrelated"},"projects":{}})).unwrap()).unwrap();
    perform(&runtime, &workspace, &native, &runs, &session).unwrap();
    let after = editor_core::load(&runs, &workspace.display().to_string()).unwrap();
    assert_eq!(
        after.plugin_configurations["shell-script"].provider,
        crate::terminal::configurations::PROVIDER
    );
    assert_eq!(after.plugin_configurations["shell-script"].values, values);
    assert_eq!(
        after.configurations[0].provider.as_deref(),
        Some("nanobug.execution")
    );
    assert_eq!(after.plugin_configurations["shell-script"].revision, 5);
    let layout: Value = serde_json::from_slice(&std::fs::read(session).unwrap()).unwrap();
    assert_eq!(layout["native_terminal_visible"], true);
    assert_eq!(layout["plugin_dock_sizes"]["bottom"], 310);
    assert_eq!(
        layout["dock_layout"]["bottom_dock"]["panel"]["panel_name"],
        "NativeTerminal"
    );
    assert_eq!(layout["plugin_panel_visibility"]["unrelated/panel"], false);
    assert!(
        Manager::read_registry(&runtime)
            .unwrap()
            .contains_key("unrelated")
    );
    let preferences: Value =
        serde_json::from_slice(&std::fs::read(runtime.join("service-providers.json")).unwrap())
            .unwrap();
    assert_eq!(
        preferences["user"]["workspace/interactive.execute"],
        "nanobug.execution"
    );
    assert_eq!(preferences["user"]["workspace/example.echo"], "unrelated");
}

/// Old physical wrapping becomes one logical line; changing width must recover the complete text.
#[test]
fn physical_wraps_reflow_without_inventing_line_breaks() {
    let settings = Settings::default();
    let old = protocol::Snapshot { schema: 2, data: json!({"tabs":[{"id":1,"name":"long line","profile":settings.profiles[0],"cwd":"C:\\work",
        "output":"abcdefgh\r\nijklm\r\n","display":{"rows":3,"columns":8,"cursor":[1,5],"wrap_pending":false,"scrollback":0,"wrapped_lines":[0],"soft_wraps":false}}],
        "active":0,"next_id":1,"settings":settings,"recovery_version":1}).to_string() };
    let bytes = legacy::convert(&serde_json::to_vec(&old).unwrap(), None).unwrap();
    let saved: crate::terminal::persistence::Saved = serde_json::from_slice(&bytes).unwrap();
    let mut engine = Engine::new(
        GridSize {
            columns: 8,
            rows: 3,
        },
        100,
    );
    engine
        .restore(saved.sessions.into_iter().next().unwrap().grid)
        .unwrap();
    engine.resize(GridSize {
        columns: 20,
        rows: 3,
    });
    let grid = engine.snapshot();
    let text: String = grid.lines[0].iter().map(|cell| cell.c).collect();
    assert_eq!(text.trim_end(), "abcdefghijklm");
    assert_eq!(grid.cursor, (0, 13));
}

/// Conversion enforces the same storage quota as live checkpoints, retaining the newest visible output.
#[test]
fn oversized_history_keeps_recent_output_and_metadata() {
    let mut settings = Settings::default();
    settings.history = 1000;
    let output = (0..1000)
        .map(|index| format!("ROW-{index:04} {}\r\n", "x".repeat(145)))
        .collect::<String>();
    let old = protocol::Snapshot { schema: 2, data: json!({"tabs":[{"id":9,"name":"history survives","profile":settings.profiles[0],"cwd":"C:\\work",
        "output":output,"display":{"rows":5,"columns":160,"cursor":[4,0],"wrap_pending":false,"scrollback":0,"wrapped_lines":[],"soft_wraps":true}}],
        "active":0,"next_id":9,"settings":settings,"recovery_version":1}).to_string() };
    let bytes = legacy::convert(&serde_json::to_vec(&old).unwrap(), None).unwrap();
    assert!(bytes.len() <= 8 * 1024 * 1024);
    let saved: crate::terminal::persistence::Saved = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(saved.active, Some(9));
    assert_eq!(saved.sessions[0].name, "history survives");
    let text: String = saved.sessions[0]
        .grid
        .lines
        .iter()
        .flatten()
        .map(|cell| cell.c)
        .collect();
    assert!(
        text.contains("ROW-0999"),
        "newest output cannot be discarded to satisfy the quota"
    );
    assert!(
        saved.sessions[0].grid.lines.len() < 1001,
        "only oldest scrollback may be reduced"
    );
}

/// Legacy JSON escaping can exceed the new file quota even when its output fits the retired quota.
#[test]
fn valid_legacy_source_above_new_quota_still_imports() {
    let root = tempfile::tempdir().unwrap();
    let runtime = root.path().join("runtime");
    let native = root.path().join("native");
    let data = legacy(&runtime, root.path());
    let settings = Settings::default();
    let old = protocol::Snapshot { schema: 2, data: json!({"tabs":[{"id":1,"name":"large old source","profile":settings.profiles[0],"cwd":root.path(),
        "output":"\u{1b}[0m\"".repeat(1250000),"display":{"rows":5,"columns":80,"cursor":[0,0],"wrap_pending":false,"scrollback":0,"wrapped_lines":[],"soft_wraps":true}}],
        "active":0,"next_id":1,"settings":settings,"recovery_version":1}).to_string() };
    let source = serde_json::to_vec(&old).unwrap();
    // The declared quota bounds decoded data; the outer envelope escapes that JSON a second time.
    assert!(old.data.len() <= 16 * 1024 * 1024);
    assert!(source.len() > 16 * 1024 * 1024 && source.len() <= 40 * 1024 * 1024);
    std::fs::write(data.parent().unwrap().join("state.json"), &source).unwrap();
    perform(
        &runtime,
        root.path(),
        &native,
        root.path(),
        &root.path().join("layout.json"),
    )
    .unwrap();
    assert!(native.join("state.json").exists());
    assert_eq!(
        std::fs::read(native.join("upgrade-backup/state.json")).unwrap(),
        source
    );
}

/// A damaged journal is rejected as a whole, before even its earlier valid target is committed.
#[test]
fn damaged_journal_never_partially_commits() {
    let root = tempfile::tempdir().unwrap();
    let native = root.path().join("native");
    let runtime = root.path().join("runtime");
    let run = root.path().join("run.json");
    let layout = root.path().join("layout.json");
    let locations = storage::Locations {
        native: &native,
        configurations: &run,
        layout: &layout,
        runtime: &runtime,
    };
    let mut plan = storage::Plan::default();
    plan.add(
        &locations,
        storage::Kind::State,
        b"converted state".to_vec(),
    )
    .unwrap();
    plan.add(
        &locations,
        storage::Kind::Layout,
        b"converted layout".to_vec(),
    )
    .unwrap();
    plan.stage(&native).unwrap();
    let pending = native.join("upgrade-pending.json");
    let mut value: Value = serde_json::from_slice(&std::fs::read(&pending).unwrap()).unwrap();
    value["targets"][1]["after"] = json!("damaged checksum");
    std::fs::write(&pending, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(perform(&runtime, root.path(), &native, root.path(), &layout).is_err());
    assert!(!native.join("state.json").exists());
    assert!(!layout.exists());
    assert!(pending.exists());
}

/// A current same-ID package retains its contributions, preferences and configuration ownership.
#[test]
fn current_package_identity_is_not_a_legacy_reference() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let runtime = root.path().join("runtime");
    let native = root.path().join("native");
    let data = legacy(&runtime, &workspace);
    let mut registry = Manager::read_registry(&runtime).unwrap();
    // Another workspace already retired the old version before a current same-ID package arrived.
    std::fs::create_dir_all(runtime.join("builtin-terminal-backup")).unwrap();
    std::fs::write(
        runtime.join("builtin-terminal-backup/installation.json"),
        serde_json::to_vec(&registry).unwrap(),
    )
    .unwrap();
    registry.get_mut("terminal").unwrap().manifest.version = "1.0.0".into();
    std::fs::write(
        runtime.join("registry.json"),
        serde_json::to_vec(&registry).unwrap(),
    )
    .unwrap();
    snapshot(&data);
    let runs = root.path().join("runs");
    let set = editor_core::RunConfigSet::from_json(br#"{"version":2,"configurations":[{"id":"current","name":"current","provider":"terminal","target":{"mode":"program","program":"tool.exe"}}],"plugin_configurations":{"current":{"provider":"terminal","template":"custom","values":"{}","name":"current","program":"tool.exe","revision":0}}}"#).unwrap();
    editor_core::save(&runs, &workspace.display().to_string(), &set).unwrap();
    let session = root.path().join("layout.json");
    let mut layout =
        serde_json::to_value(crate::app::session::SessionState::for_workspace(&workspace)).unwrap();
    layout["plugin_panel_visibility"] = json!({"terminal/terminal":true});
    layout["dock_layout"] = json!({"panel_name":"plugin:terminal/terminal"});
    let source = serde_json::to_vec(&layout).unwrap();
    std::fs::write(&session, &source).unwrap();
    let prefs = br#"{"user":{"workspace/interactive.execute":"terminal"},"projects":{}}"#;
    std::fs::write(runtime.join("service-providers.json"), prefs).unwrap();
    perform(&runtime, &workspace, &native, &runs, &session).unwrap();
    let after = editor_core::load(&runs, &workspace.display().to_string()).unwrap();
    assert_eq!(
        after.configurations[0].provider.as_deref(),
        Some("terminal")
    );
    assert_eq!(after.plugin_configurations["current"].provider, "terminal");
    assert_eq!(std::fs::read(session).unwrap(), source);
    assert_eq!(
        std::fs::read(runtime.join("service-providers.json")).unwrap(),
        prefs
    );
    assert!(
        Manager::read_registry(&runtime)
            .unwrap()
            .contains_key("terminal")
    );
    assert!(!native.join("state.json").exists());
}
