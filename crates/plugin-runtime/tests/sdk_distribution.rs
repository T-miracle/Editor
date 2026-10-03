//! Install a package built outside the repository using only the distributed editor SDK.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, ui::Kind},
};
use std::path::Path;

/// Observe the public native view produced by the real component, not its internal guest state.
fn panel_text<'a>(manager: &'a Manager, plugin_id: &str) -> &'a str {
    let document = manager.live[plugin_id].views["welcome"].as_ref();
    let Kind::Text { text } = &document.root.kind else {
        panic!("SDK guest must publish its diagnostic text");
    };
    text
}

/// Package inspection, consent and typed calls must work with the independently built artifact.
#[test]
#[ignore = "build the host, then run scripts/verify-plugin-sdk.ps1 first"]
fn externally_built_sdk_package_includes_readme_and_runs_through_public_manager() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/plugin-sdk-test/capability-example.zip");
    let package = Package::read(&path).expect("read the package built outside the repository");
    assert_eq!(package.manifest.protocol, 7);
    assert_eq!(
        package.files.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "README.md",
            "capability-example.wasm",
            "composed-ui.json",
            "manifest.json",
            "welcome.txt",
        ],
        "the distributable contains its declared resources, without SDK or build files"
    );
    let readme = std::str::from_utf8(&package.files["README.md"])
        .expect("the installed README remains readable UTF-8");
    assert!(readme.starts_with("# Capability Example"));
    assert!(readme.contains("--plugin-cargo"));

    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(root.path().into(), Environment::default()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .expect("install a current SDK component with its approved permissions");
    let id = &package.manifest.id;
    assert!(panel_text(&manager, id).contains("Hello from a versioned package asset."));
    assert!(
        panel_text(&manager, id).contains("Optional feature unavailable; native fallback active.")
    );
    manager
        .invoke_command(id, "check-errors", serde_json::Value::Null)
        .expect("the exported contract preserves typed errors and request correlation");
    assert_eq!(
        panel_text(&manager, id),
        "Typed errors and request IDs verified."
    );
}
