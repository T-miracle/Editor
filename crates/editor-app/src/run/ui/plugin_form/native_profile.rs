//! Explicit preparation for manual Windows acceptance, through public package and storage APIs.
//! This ignored fixture creates only its dedicated workspace and never starts a configured command.
use super::*;
use plugin_runtime::{Manager, Package, plugin_protocol as protocol};

/// Seed a persistent, isolated runtime plus a long tree/form for real IME, theme and scroll checks.
#[test]
#[ignore = "build actual rust/terminal packages first; prepares a dedicated native acceptance profile"]
fn prepare_native_configuration_profile() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let root = repo.join("target/run-config-plugin-tree/native-04");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"native-configuration\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/main.rs"),
        "// Native acceptance fixture; no command is launched during preparation.\nfn main() {}\n",
    )
    .unwrap();
    let workspace = editor_core::Workspace::open(&root).unwrap();
    let key = workspace.root().display().to_string();
    let mut manager = Manager::open(
        root.join("runtime"),
        protocol::Environment {
            workspace: key.clone(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    for name in ["rust", "terminal"] {
        let package = Package::read(&repo.join(format!("dist/plugins/{name}.zip"))).unwrap();
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
    }
    manager.shutdown();
    let mut set = RunConfigSet::default();
    for index in 0..80 {
        let id = format!("native-{index:02}");
        let name = format!("配置 {index:02} / Cargo run");
        let args = if index == 0 {
            (0..80)
                .map(|index| {
                    if index == 0 {
                        "run".into()
                    } else {
                        format!("argument {index:02}")
                    }
                })
                .collect()
        } else {
            vec!["run".into(), "--release".into()]
        };
        let fields = contract::command_form::Fields::new(&name, args.clone());
        let configuration = serde_json::from_value(serde_json::json!({"id":id,"name":name,"target":{"mode":"program","program":"cargo","args":args}})).unwrap();
        set.upsert(configuration).unwrap();
        set.plugin_configurations.insert(
            id,
            PluginConfiguration {
                provider: "rust".into(),
                template: "run".into(),
                values: serde_json::to_string(&fields).unwrap(),
                pending_events: vec![],
                name,
                program: "cargo".into(),
                revision: 0,
                validation: ConfigurationValidation::Unchecked,
            },
        );
    }
    // No external choice: opening the dialog must show the empty right pane despite a populated tree.
    set.selected = None;
    editor_core::save(&editor_core::default_root().unwrap(), &key, &set).unwrap();
    std::fs::write(root.join("acceptance-profile.json"), serde_json::to_vec_pretty(&serde_json::json!({"workspace":key,"runtime":root.join("runtime"),"configuration":editor_core::storage_path(&key)})).unwrap()).unwrap();
}
