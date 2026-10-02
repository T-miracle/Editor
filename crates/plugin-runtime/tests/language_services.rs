//! Language-service packages are inspected through the public package boundary, without known language IDs.
use plugin_runtime::Package;
use serde_json::json;
use std::io::{Cursor, Write};

/// A declaration-only package needs neither an empty lifecycle guest nor a host language branch.
fn package(provider: serde_json::Value) -> anyhow::Result<Package> {
    let manifest = json!({
        "id":"novel-analysis", "name":"Novel analysis", "version":"1.0.0", "protocol":7,
        "api":{"base":"^1", "required":{"language.lsp":"^1", "process":"^1"}},
        "contributions":"plugin.toml", "storage_limit":1024,
        "services":{"analysis":{"program":"novel-analysis-server", "args":["--stdio"]}},
        "permissions":["process.service.analysis"], "language_servers":[provider]
    });
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("manifest.json", serde_json::to_vec(&manifest)?),
        (
            "plugin.toml",
            br#"[plugin]
id = "novel-analysis"
name = "Novel analysis"
version = "1.0.0"
host_version = "^0.1"
"#
            .to_vec(),
        ),
    ] {
        archive.start_file(name, zip::write::SimpleFileOptions::default())?;
        archive.write_all(&bytes)?;
    }
    Package::from_bytes(&archive.finish()?.into_inner())
}

/// A language need not be registered by this package, but its native service authority must be declared.
#[test]
fn independent_declarative_lsp_provider_requires_a_declared_service() {
    let result =
        package(json!({"id":"analysis", "language":"unknown-language", "service":"analysis"}));
    assert!(
        result.is_ok(),
        "valid provider rejected: {:?}",
        result.err()
    );
    assert!(
        package(json!({"id":"analysis", "language":"unknown-language", "service":"undeclared"}))
            .is_err()
    );
    assert!(package(json!({"id":"analysis", "language":"unknown-language", "service":"analysis", "hook":true})).is_err());
}
