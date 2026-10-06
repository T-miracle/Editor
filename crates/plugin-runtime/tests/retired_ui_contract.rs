//! Admission rejects retired presentation fields before a package can obtain live resources.
use plugin_runtime::Package;
use serde_json::{Value, json};
use std::io::{Cursor, Write};

/// Only archive inspection is exercised here; actual executable behavior uses independently built SDK guests.
fn inspect(edit: impl FnOnce(&mut Value)) -> anyhow::Result<Package> {
    let mut manifest = json!({
        "id":"contract-fixture", "name":"Contract Fixture", "version":"1.0.0", "protocol":7,
        "api":{"base":"^1", "required":{"ui.native":"^1"}},
        "component":"fixture.wasm", "permissions":[], "storage_limit":1024,
        "panels":[{"id":"panel", "title":"Panel", "position":"right"}],
        "commands":[{"id":"action", "title":"Action", "menu":true}]
    });
    edit(&mut manifest);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("manifest.json", zip::write::SimpleFileOptions::default())?;
    zip.write_all(&serde_json::to_vec(&manifest)?)?;
    zip.start_file("fixture.wasm", zip::write::SimpleFileOptions::default())?;
    zip.write_all(b"\0asm\x0d\0\x01\0")?;
    Package::from_bytes(&zip.finish()?.into_inner())
}

/// Removed fields cannot silently retain host domain controls, and rejection gives a concrete SDK update route.
#[test]
fn retired_presentation_and_host_artwork_are_rejected_before_installation() {
    inspect(|_| {}).unwrap();
    for field in [
        "toolbar",
        "toolbar_icon",
        "view_modes",
        "editor.presentation",
    ] {
        let result = inspect(|manifest| match field {
            "view_modes" => {
                manifest["panels"][0][field] = json!({
                    "source":"icons/source.svg", "split":"icons/split.svg", "preview":"icons/preview.svg"
                })
            }
            "editor.presentation" => manifest["api"]["required"][field] = json!("^1"),
            _ => manifest["commands"][0][field] = json!("icons/host-artwork.svg"),
        });
        let error = result
            .err()
            .unwrap_or_else(|| panic!("accepted retired contract: {field}"));
        let message = format!("{error:#}");
        assert!(
            message.contains(field),
            "missing incompatible field: {message}"
        );
        assert!(
            message.contains("更新插件"),
            "missing update guidance: {message}"
        );
    }
}
