//! Language protocol declarations remain generic and bounded at the public package inspection seam.
use plugin_runtime::Package;
use serde_json::{Value, json};
use std::io::{Cursor, Write};

/// A declarative fixture requires no guest or installed tool to inspect its approved startup contract.
fn package(experimental: Value) -> anyhow::Result<Package> {
    package_with_service(experimental, json!({"program":"unfamiliar-language-tool"}))
}

/// Search policy is supplied by the package and never selected from a concrete language ID.
fn package_with_service(experimental: Value, service: Value) -> anyhow::Result<Package> {
    let manifest = json!({"id":"unfamiliar-language","name":"Unfamiliar language","version":"1.0.0",
        "protocol":7,"api":{"base":"^1","required":{"language.lsp":"^1","process":"^1"}},
        "contributions":"plugin.toml","storage_limit":1024,"permissions":["process.service.analysis"],
        "services":{"analysis":service},
        "language_servers":[{"id":"analysis","language":"unfamiliar","service":"analysis","client_experimental":experimental}]});
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("manifest.json", serde_json::to_vec(&manifest)?),
        (
            "plugin.toml",
            br#"[plugin]
id = "unfamiliar-language"
name = "Unfamiliar language"
version = "1.0.0"
host_version = ">=0.1.0"
"#
            .to_vec(),
        ),
    ] {
        archive.start_file(name, zip::write::SimpleFileOptions::default())?;
        archive.write_all(&bytes)?;
    }
    Package::from_bytes(&archive.finish()?.into_inner())
}

/// A broken higher-priority candidate cannot hide the next working approved native tool.
#[test]
fn declared_tool_search_checks_candidates_without_using_a_shell() {
    use plugin_runtime::{Manager, plugin_protocol::Environment};
    let root = tempfile::tempdir().unwrap();
    let tools = root.path().join("tools");
    for version in ["1", "9"] {
        std::fs::create_dir_all(tools.join(version)).unwrap();
    }
    let executable = format!("probe{}", std::env::consts::EXE_SUFFIX);
    std::fs::copy(
        std::env::current_exe().unwrap(),
        tools.join("1").join(&executable),
    )
    .unwrap();
    std::fs::write(tools.join("9").join(&executable), b"broken tool shim").unwrap();
    let search = format!("{}/*/{executable}", tools.display()).replace('\\', "/");
    let package = package_with_service(
        json!({}),
        json!({"program":"absent-fixture-tool",
        "args":["--list"], "search_paths":[search], "check_args":["--list"]}),
    )
    .expect("generic search declaration must inspect");
    let mut manager = Manager::open(
        root.path().join("installed"),
        Environment {
            workspace: root.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let services = manager.language_services();
    let plan = services["unfamiliar-language/analysis"]
        .as_ref()
        .expect("a working declared candidate should resolve");
    let (_process, input, mut output) = plan.spawn().unwrap();
    drop(input);
    let mut text = String::new();
    std::io::Read::read_to_string(&mut output, &mut text).unwrap();
    assert!(text.contains("accepts_generic_experimental_client_capabilities"));
    // An explicit broken executable must never be replaced with a convenient discovered candidate.
    let explicit = package_with_service(
        json!({}),
        json!({"program":tools.join("9").join(&executable),
        "search_paths":[format!("{}/*/{executable}", tools.display()).replace('\\', "/")],
        "check_args":["--list"]}),
    )
    .unwrap();
    manager
        .install(&explicit, explicit.manifest.permissions.clone())
        .unwrap();
    assert!(manager.language_services()["unfamiliar-language/analysis"].is_err());
}

/// Intermediate wildcard expansion consumes budgets even when the final filename is absent.
#[test]
fn empty_tool_search_still_accounts_for_visited_directories() {
    let root = tempfile::tempdir().unwrap();
    let tools = root.path().join("tools");
    for index in 0..300 {
        std::fs::create_dir_all(tools.join(index.to_string())).unwrap();
    }
    // A glob can yield no executable while its intermediate expansion consumes unbounded work.
    let package = package_with_service(
        json!({}),
        json!({"program":"missing-fixture",
        "search_paths":[format!("{}/*/missing.exe", tools.display()).replace('\\', "/")]}),
    )
    .unwrap();
    let mut manager = plugin_runtime::Manager::open(
        root.path().join("installed"),
        plugin_runtime::plugin_protocol::Environment {
            workspace: root.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let plans = manager.language_services();
    let error = plans["unfamiliar-language/analysis"]
        .as_ref()
        .err()
        .unwrap();
    assert!(
        error.contains("quota"),
        "intermediate search must be bounded: {error}"
    );
}

/// No server name is needed to preserve an arbitrary experimental capability object.
#[test]
fn accepts_generic_experimental_client_capabilities_and_rejects_unbounded_values() {
    let accepted = package(json!({"fixtureFeature":{"version":2,"readyNotification":true}}));
    assert!(
        accepted.is_ok(),
        "valid client capabilities rejected: {:?}",
        accepted.err()
    );
    assert!(package(json!({"x".repeat(257):true})).is_err());
    let mut nested = json!(true);
    for _ in 0..20 {
        nested = json!({"nested":nested});
    }
    assert!(package(json!({"tooDeep":nested})).is_err());
}
