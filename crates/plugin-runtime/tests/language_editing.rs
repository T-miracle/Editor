//! Independent language editing declarations are accepted only with negotiated public capabilities.
use plugin_runtime::Package;
use serde_json::{Value, json};
use std::io::{Cursor, Write};

/// Inspect a genuinely independent package; it does not recognize or highlight a language itself.
fn package(capabilities: Value) -> anyhow::Result<Package> {
    let manifest = json!({
        "id":"independent-formatter", "name":"Independent formatter", "version":"1.0.0", "protocol":7,
        "api":{"base":"^1", "required":capabilities}, "storage_limit":1024, "contributions":"plugin.toml",
        "services":{"format":{"program":"independent-language-service", "args":["--stdio"]}},
        "permissions":["process.service.format"],
        "language_servers":[{"id":"format", "language":"javascript", "service":"format",
            "primary":false, "formatting":true}]
    });
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    archive.start_file("manifest.json", zip::write::SimpleFileOptions::default())?;
    archive.write_all(&serde_json::to_vec(&manifest)?)?;
    archive.start_file("plugin.toml", zip::write::SimpleFileOptions::default())?;
    archive.write_all(b"[plugin]\nid='independent-formatter'\nname='Independent formatter'\nversion='1.0.0'\nhost_version='^0.1'\n")?;
    Package::from_bytes(&archive.finish()?.into_inner())
}

/// A formatter contributes through the normal package boundary without taking over a main LSP.
#[test]
fn independent_formatter_negotiates_its_own_capability() {
    let capabilities = json!({"language.lsp":"^1", "process":"^1", "language.formatting":"^1"});
    assert!(package(capabilities).is_ok());
    assert!(package(json!({"language.lsp":"^1", "process":"^1"})).is_err());
}
