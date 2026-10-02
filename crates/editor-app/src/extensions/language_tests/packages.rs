//! Independent ZIP fixtures enter through public package inspection and installation.
use super::*;
use std::io::{Cursor, Write};

/// The language identity is unfamiliar to the host; only the grammar export name is reused.
pub(in crate::extensions) fn language_package(id: &str) -> Package {
    let source = format!(
        r#"[plugin]
id = "{id}"
name = "Novel language"
version = "1.0.0"
host_version = ">=0.1.0"
[[language_definitions]]
id = "novel"
name = "Novel"
extensions = ["novel"]
[[highlighters]]
id = "syntax"
language = "novel"
grammar_name = "toml"
grammar = "grammar.wasm"
highlights = "highlights.scm"
tree_sitter_abi = 15
"#
    );
    let manifest = serde_json::json!({"id":id,"name":"Novel language","version":"1.0.0",
        "protocol":7,"api":{"base":"^1"},"contributions":"plugin.toml","storage_limit":1024});
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/toml");
    let legacy = plugin_schema::PluginManifest::parse(
        &std::fs::read_to_string(root.join("plugin.toml")).unwrap(),
    )
    .unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
        ("plugin.toml", source.into_bytes()),
        (
            "grammar.wasm",
            std::fs::read(root.join(&legacy.languages[0].grammar)).unwrap(),
        ),
        (
            "highlights.scm",
            std::fs::read(root.join(&legacy.languages[0].highlights)).unwrap(),
        ),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&zip.finish().unwrap().into_inner()).unwrap()
}

/// The public package inspector must reject selectors that would create duplicate native choices.
#[test]
fn duplicate_language_selectors_are_rejected_before_installation() {
    let package = language_package("novel-duplicates");
    let mut files = package.files;
    let source = String::from_utf8(files["plugin.toml"].clone()).unwrap();
    files.insert(
        "plugin.toml".into(),
        source
            .replace(
                "extensions = [\"novel\"]",
                "extensions = [\"novel\", \"NOVEL\"]",
            )
            .into_bytes(),
    );
    assert!(repack(files).is_err());
}

/// Reinspection keeps fixture changes within the actual package validation boundary.
pub(in crate::extensions) fn repack(files: BTreeMap<String, Vec<u8>>) -> anyhow::Result<Package> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        zip.start_file(name, zip::write::SimpleFileOptions::default())?;
        zip.write_all(&bytes)?;
    }
    Package::from_bytes(&zip.finish()?.into_inner())
}

/// Transitional packages still use their WASM grammar, with external services omitted from this fixture.
pub(super) fn legacy_rust_package() -> Package {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/rust");
    let source = std::fs::read_to_string(root.join("plugin.toml")).unwrap();
    let source = source
        .lines()
        .filter(|line| !line.starts_with("file_icons =") && !line.starts_with("lsp_command ="))
        .collect::<Vec<_>>()
        .join("\n");
    let declaration = plugin_schema::PluginManifest::parse(&source).unwrap();
    let language = &declaration.languages[0];
    let manifest = serde_json::json!({"id":"rust","name":"Rust","version":declaration.plugin.version,
        "protocol":1,"contributions":"plugin.toml","storage_limit":1024});
    repack(BTreeMap::from([
        (
            "manifest.json".into(),
            serde_json::to_vec(&manifest).unwrap(),
        ),
        ("plugin.toml".into(), source.into_bytes()),
        (
            language.grammar.to_string_lossy().into_owned(),
            std::fs::read(root.join(&language.grammar)).unwrap(),
        ),
        (
            language.highlights.to_string_lossy().into_owned(),
            std::fs::read(root.join(&language.highlights)).unwrap(),
        ),
    ]))
    .unwrap()
}
