//! Actual installed Rust policy and native analysis verify the public SDK discovery path.
use super::*;

/// Exercise the terminal import from the full editor workspace without a local SDK.
#[test]
#[ignore = "requires local Rust Analyzer and the editor workspace dependencies"]
fn host_sdk_completes_terminal_from_editor_workspace() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let (_storage, _manager, server) = installed_rust_server(&root);
    server.prepare_until_ready().unwrap();
    let path = root.join("plugins/terminal/src/controls.rs");
    let uri = file_uri(&path.canonicalize().unwrap()).unwrap();
    for name in [
        "Action",
        "Canvas",
        "MenuItem",
        "PopupMenu",
        "SideTab",
        "SideTabs",
        "UiEvent",
    ] {
        let prefix = &name[..3];
        let source =
            format!("// Host protocol completion probe.\nuse plugin_protocol::ui::{{{prefix}}};\n");
        let position = position_at_byte(&source, source.find(prefix).unwrap() + prefix.len());
        let response = server.completions(uri.clone(), source, position).unwrap();
        let items = match response {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        };
        assert!(
            items.iter().any(|item| item.label == name),
            "missing {name}: {items:?}"
        );
    }
    assert!(!root.join("plugins/terminal/sdk").exists());
}

/// Resolve completion and definition through the host cache for an excluded, SDK-free guest.
#[test]
#[ignore = "requires a local Rust Analyzer and Cargo toolchain"]
fn host_sdk_completes_nested_plugin_without_local_files() {
    let root = tempfile::Builder::new()
        .prefix("plugin SDK completion ")
        .tempdir()
        .unwrap();
    let plugin = root.path().join("plugins/demo");
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::write(root.path().join("src/lib.rs"), "// Host fixture.\n").unwrap();
    std::fs::write(
        root.path().join("Cargo.toml"),
        r#"[package]
name = "sdk-host-fixture"
version = "0.1.0"
edition = "2024"
[workspace]
exclude = ["plugins/demo"]
"#,
    )
    .unwrap();
    std::fs::create_dir_all(plugin.join("src")).unwrap();
    std::fs::write(plugin.join("manifest.json"), "{}").unwrap();
    let manifest_source = r#"[package]
name = "sdk-guest-fixture"
version = "0.1.0"
edition = "2024"
[dependencies]
plugin-protocol = { version = "=0.2.0", features = ["guest"] }
"#;
    std::fs::write(plugin.join("Cargo.toml"), manifest_source).unwrap();
    let path = plugin.join("src/lib.rs");
    let source = "// Host protocol import.\nuse plugin_protocol::ui::{Act};\n".to_owned();
    std::fs::write(&path, &source).unwrap();
    let (_storage, _manager, server) = installed_rust_server(root.path());
    server.prepare_until_ready().unwrap();
    // Include the server's loaded-workspace report when diagnosing integration failures.
    let analysis_status = server
        .connection
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .request("rust-analyzer/analyzerStatus", json!({}))
        .unwrap();
    let uri = file_uri(&path).unwrap();
    let position = position_at_byte(&source, source.find("Act").unwrap() + 3);
    let response = server
        .completions(uri.clone(), source.clone(), position)
        .unwrap();
    let items = match response {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    assert!(
        items.iter().any(|item| item.label == "Action"),
        "missing Action: {items:?}; server status: {analysis_status}"
    );
    let source = source.replace("{Act}", "{Action}");
    let hover = server.hover(uri.clone(), source.clone(), position).unwrap();
    assert!(
        hover.is_some(),
        "public SDK imports must retain hover documentation"
    );
    let definitions = server.definitions(uri, source, position).unwrap();
    assert!(
        definitions
            .iter()
            .any(|definition| definition.target_uri.as_str().contains("plugin-sdk/")),
        "definition must point to host SDK: {definitions:?}"
    );
    assert_eq!(
        std::fs::read_to_string(plugin.join("Cargo.toml")).unwrap(),
        manifest_source
    );
    assert!(!plugin.join("sdk").exists());
    assert!(!plugin.join(".cargo").exists());
}

/// Exercises the bundled plugin against the locally installed server and workspace.
#[test]
#[ignore = "run scripts/rust-readiness-smoke.ps1 with a local Rust language server"]
fn local_rust_server_readiness() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let (_storage, _manager, server) = installed_rust_server(&root);
    server.prepare_until_ready().unwrap();
}

/// Keep the installed owner alive so both guest hooks and native leases use production lifecycle rules.
pub(super) fn installed_rust_server(
    root: &Path,
) -> (tempfile::TempDir, plugin_runtime::Manager, LanguageServer) {
    let storage = tempfile::tempdir().unwrap();
    let package = plugin_runtime::Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/rust.zip"),
    )
    .expect("build the Rust package with scripts/build-plugins.ps1");
    let resources = plugin_runtime::HostResources {
        sdk: Some(crate::sdk_export::descriptor().map_err(|error| format!("{error:#}"))),
        ..Default::default()
    };
    let mut manager = plugin_runtime::Manager::open_with_resources(
        storage.path().join("plugins"),
        plugin_runtime::plugin_protocol::Environment {
            workspace: root.canonicalize().unwrap().display().to_string(),
            ..Default::default()
        },
        true,
        resources,
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let plan = manager.language_services()["rust/analysis"]
        .as_ref()
        .unwrap()
        .clone();
    let server = LanguageServer::from_service(plan).unwrap();
    (storage, manager, server)
}
