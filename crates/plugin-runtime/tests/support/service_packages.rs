//! Shared ZIP fixture construction for public Manager and native editor acceptance tests.
use plugin_runtime::Package;
use serde_json::{Value, json};
use std::io::{Cursor, Write};

/// Test packages use the actual SDK-built component and differ only in declared contracts and assets.
pub fn package(id: &str, provider: bool, optional: bool, version: &str) -> Package {
    let archive = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/plugin-api-test/capability-example.zip");
    let mut files = Package::read(&archive).unwrap().files;
    let mut manifest: Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["id"] = json!(id);
    manifest["name"] = json!(id);
    manifest["settings_hook"] = json!(false);
    manifest["api"]["required"]["plugin.services"] = json!("^1");
    manifest["permissions"] = if provider {
        json!([
            "assets.read",
            "workspace.read",
            "editor.read",
            "storage",
            "services.call"
        ])
    } else {
        json!([
            "assets.read",
            "workspace.read",
            "editor.read",
            "services.call"
        ])
    };
    let mut methods:serde_json::Map<_,_>=["echo","defer","reply-retained","probe","source","cycle","bad-result","trap"].into_iter().map(|id|(id.into(),json!({
        "parameters":{"type":"string","max_bytes":2048},"result":{"type":"string","max_bytes":4096}
    }))).collect();
    for (name, permission) in [
        ("read-close", "workspace.read"),
        ("editor-continuation", "editor.read"),
        ("editor-cancel", "editor.read"),
    ] {
        methods.insert(name.into(), json!({
            "parameters":{"type":"string","max_bytes":2048}, "result":{"type":"string","max_bytes":4096},
            "permissions":[permission]
        }));
    }
    manifest["plugin_services"] = if provider {
        json!({
            "provides":{"example.echo":{"version":version,"methods":methods}},
            "requires":{"example.echo":{"version":"^1","optional":true,"methods":methods}}
        })
    } else {
        json!({"requires":{"example.echo":{"version":version,"optional":optional,"methods":methods}}})
    };
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    files.insert("service-label.txt".into(), id.as_bytes().to_vec());
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        archive
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&archive.finish().unwrap().into_inner()).unwrap()
}

/// A pure provider needs no consumer permission merely to reply to its own incoming requests.
pub fn pure_provider(id: &str) -> Package {
    let mut files = package(id, true, false, "1.0.0").files;
    let mut manifest: Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["plugin_services"]
        .as_object_mut()
        .unwrap()
        .remove("requires");
    manifest["permissions"]
        .as_array_mut()
        .unwrap()
        .retain(|permission| permission != "services.call");
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        archive
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&archive.finish().unwrap().into_inner()).unwrap()
}
