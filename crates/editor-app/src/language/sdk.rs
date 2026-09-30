//! Connect host-owned protocol sources to Rust analysis without editing plugin projects.

use serde_json::{Value, json};
use std::path::Path;

/// Supply Cargo's SDK override and include independent plugin crates excluded by the host workspace.
pub(super) fn initialization_options(root: &Path, language_id: &str) -> anyhow::Result<Value> {
    if language_id != "rust" {
        return Ok(Value::Null);
    }
    // configPath reaches cargo metadata too; extraArgs alone is not forwarded there by all servers.
    // Native diagnostics analyze the unsaved LSP buffer. Older rust-analyzer
    // releases gate type/path diagnostics behind this option; Cargo checks still
    // supplement them on save for errors requiring the full compiler.
    let mut options = json!({
        "cargo": { "configPath": crate::sdk_export::cargo_config()? },
        "diagnostics": { "enable": true, "experimental": { "enable": true } }
    });
    let mut manifests = Vec::new();
    if root.join("Cargo.toml").is_file() {
        manifests.push(root.join("Cargo.toml"));
    }
    // Respect repository ignore rules and never index generated or vendored dependency trees.
    for entry in ignore::WalkBuilder::new(root)
        .filter_entry(|entry| {
            !entry.file_type().is_some_and(|kind| kind.is_dir())
                || !matches!(
                    entry.file_name().to_str(),
                    Some("target" | "vendor" | "node_modules" | ".git")
                )
        })
        .build()
    {
        // An unreadable unrelated directory must not disable the language server.
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                tracing::warn!(%error, "skipping unreadable directory during plugin discovery");
                continue;
            }
        };
        if entry.file_name() == "Cargo.toml"
            && entry
                .path()
                .parent()
                .is_some_and(|parent| parent.join("manifest.json").is_file())
        {
            manifests.push(entry.path().to_path_buf());
        }
    }
    manifests.sort();
    manifests.dedup();
    if !manifests.is_empty() {
        // Match LSP file URIs: Windows verbatim prefixes otherwise create distinct VFS roots.
        let manifests = manifests
            .iter()
            .map(|path| {
                let path = url::Url::from_file_path(path)
                    .ok()
                    .and_then(|uri| uri.to_file_path().ok())
                    .unwrap_or_else(|| path.clone());
                // rust-analyzer lowercases drive letters in LSP document paths.
                // Its source roots must match or live buffers can be classified
                // as immutable library files and their edits silently ignored.
                #[cfg(windows)]
                {
                    let mut path = path.to_string_lossy().into_owned();
                    if path.as_bytes().get(1) == Some(&b':') {
                        path[..1].make_ascii_lowercase();
                    }
                    std::path::PathBuf::from(path)
                }
                #[cfg(not(windows))]
                path
            })
            .collect::<Vec<_>>();
        options["linkedProjects"] = json!(manifests);
    }
    Ok(options)
}

/// Servers can request either the whole language configuration or a dotted subsection.
pub(super) fn configuration_section(options: &Value, section: Option<&str>) -> Value {
    match section {
        None | Some("") | Some("rust-analyzer") => options.clone(),
        Some(section) => section
            .strip_prefix("rust-analyzer.")
            .and_then(|path| {
                path.split('.')
                    .try_fold(options, |value, key| value.get(key))
            })
            .cloned()
            .unwrap_or(Value::Null),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An excluded guest must be linked explicitly while its files remain untouched.
    #[test]
    fn links_nested_guest_and_shares_build_override() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        let plugin = root.path().join("plugins/demo");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(plugin.join("manifest.json"), "{}").unwrap();
        std::fs::write(plugin.join("Cargo.toml"), "# unchanged plugin manifest\n").unwrap();
        let options = initialization_options(root.path(), "rust").unwrap();
        assert_eq!(
            configuration_section(
                &options,
                Some("rust-analyzer.diagnostics.experimental.enable")
            ),
            json!(true)
        );
        assert_eq!(options["linkedProjects"].as_array().unwrap().len(), 2);
        assert_eq!(
            options["cargo"]["configPath"],
            json!(crate::sdk_export::cargo_args().unwrap()[1])
        );
        assert!(!plugin.join("sdk").exists());
        assert!(!plugin.join(".cargo").exists());
        assert_eq!(
            std::fs::read_to_string(plugin.join("Cargo.toml")).unwrap(),
            "# unchanged plugin manifest\n"
        );
        assert_eq!(
            configuration_section(&options, Some("rust-analyzer.cargo.configPath")),
            options["cargo"]["configPath"]
        );
        assert_eq!(
            configuration_section(&options, Some("rust-analyzer")),
            options
        );
    }

    /// Non-Rust servers must not receive Rust Analyzer's host SDK settings.
    #[test]
    fn leaves_other_languages_unconfigured() {
        assert_eq!(
            initialization_options(Path::new("."), "toml").unwrap(),
            Value::Null
        );
    }
}
