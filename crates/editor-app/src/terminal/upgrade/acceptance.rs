//! Prepare an explicit isolated old-user profile for physical Windows acceptance, without launching it.
use super::*;
use crate::terminal::Settings;
use plugin_runtime::{
    Package,
    plugin_protocol::{self as protocol, configurations::command_form::Fields},
};
use serde_json::json;
use std::collections::BTreeSet;
#[path = "../../../../plugin-runtime/tests/support/debugger_packages.rs"]
#[allow(dead_code)]
// This manual acceptance fixture uses only acquisition and the interactive target.
mod debugger_packages;

/// Profile roots are supplied by the test command and checked; no real user profile is modified.
#[test]
#[ignore = "set NANOBUG_UPGRADE_QA_ROOT and its ME_EDITOR_PROFILE_HOME; prepare current SDK packages and pinned CodeLLDB VSIX"]
fn prepare_builtin_upgrade_acceptance_profile() {
    let root = std::path::PathBuf::from(
        std::env::var_os("NANOBUG_UPGRADE_QA_ROOT").expect("explicit QA root required"),
    );
    assert_eq!(
        std::path::PathBuf::from(std::env::var_os("ME_EDITOR_PROFILE_HOME").unwrap()),
        root.join("profile")
    );
    let workspace = root.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let key = workspace.display().to_string();
    let (_, binary) = debugger_packages::interactive_program(&workspace);
    let runtime = root.join("runtime");
    let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut manager = Manager::open(
        runtime.clone(),
        protocol::Environment {
            workspace: key.clone(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    for package in [
        Package::read(&repo.join("target/plugin-api-test/configuration-example.zip")).unwrap(),
        debugger_packages::debugger("qa-debugger"),
    ] {
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
    }
    manager.shutdown();
    let settings = Settings::default();
    let prompt = crate::terminal::shell::default_prompt(&settings.profiles[0], &key).unwrap();
    let manifest:protocol::Manifest=serde_json::from_value(json!({"id":"terminal","name":"Terminal","version":"0.12.2","protocol":7,
        "api":{"base":"^1"},"storage_limit":8388608,"component":"terminal.wasm","permissions":[],"panels":[{"id":"terminal","title":"Terminal","position":"bottom"}]})).unwrap();
    let mut registry = Manager::read_registry(&runtime).unwrap();
    registry.insert(
        "terminal".into(),
        Installed {
            manifest: manifest.clone(),
            digest: "retired-user-fixture".into(),
            grants: BTreeSet::new(),
            enabled: true,
            project_enabled: BTreeSet::from([key.clone()]),
            retired_ui_contract: false,
            global_enabled: None,
            error: None,
        },
    );
    std::fs::write(
        runtime.join("registry.json"),
        serde_json::to_vec(&registry).unwrap(),
    )
    .unwrap();
    let data = Manager::persisted_data_directory(&runtime, &manifest, &key).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    let old=protocol::Snapshot{schema:2,data:json!({"tabs":[{"id":7,"name":"Restored Shell","profile":settings.profiles[0],"cwd":key,
        "output":format!("{prompt}\r\nOLD_USER_HISTORY\r\n{prompt}{}","\r\n".repeat(9)),
        "display":{"rows":12,"columns":160,"cursor":[2,prompt.chars().count()],"wrap_pending":false,"scrollback":0,"wrapped_lines":[],"soft_wraps":true}},
        {"id":8,"name":"Old finished task","profile":{"name":"Service execution","program":"do-not-replay.exe","args":[]},"cwd":key,"output":"OLD_TASK_RESULT","exited":true}],
        "active":0,"next_id":8,"settings":settings,"tab_width":190.,"recovery_version":1}).to_string()};
    std::fs::write(
        data.parent().unwrap().join("state.json"),
        serde_json::to_vec(&old).unwrap(),
    )
    .unwrap();
    let mut session = crate::app::session::SessionState::for_workspace(&workspace);
    session
        .plugin_panel_visibility
        .insert("terminal/terminal".into(), true);
    session.save();
    let mut set = editor_core::RunConfigSet::default();
    let mut fields = Fields::new(
        "Migrated Shell task",
        vec!["-NoProfile".into(), "-Command".into()],
    );
    fields.script = Some("Write-Output 'UPGRADED_RUN_OK'".into());
    for (id, name, program, provider, template, values) in [
        (
            "old-script",
            "Migrated Shell task",
            "powershell.exe",
            "terminal",
            "PowerShell",
            json!({"shell":"PowerShell","fields":fields}).to_string(),
        ),
        (
            "debug-stdin",
            "Interactive Debug",
            binary.to_str().unwrap(),
            "configuration-example",
            "program",
            json!({"name":"Interactive Debug","program":binary,"arguments":[],"horizontal":false})
                .to_string(),
        ),
    ] {
        let config = serde_json::from_value(
            json!({"id":id,"name":name,"target":{"mode":"program","program":program,"args":[]}}),
        )
        .unwrap();
        set.upsert(config).unwrap();
        set.plugin_configurations.insert(
            id.into(),
            editor_core::PluginConfiguration {
                provider: provider.into(),
                template: template.into(),
                values,
                pending_events: vec![],
                name: name.into(),
                program: program.into(),
                revision: 0,
                validation: editor_core::ConfigurationValidation::Unchecked,
            },
        );
    }
    set.select("old-script");
    editor_core::save(&editor_core::default_root().unwrap(), &key, &set).unwrap();
    std::fs::write(root.join("acceptance-profile.json"),serde_json::to_vec_pretty(&json!({"workspace":workspace,"runtime":runtime,"profile":root.join("profile"),"configuration":editor_core::storage_path(&key)})).unwrap()).unwrap();
}
