//! Independent consumers describe the contract, never the terminal package or its private state.
use plugin_runtime::Package;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
    path::Path,
};

pub const CONTRACT: &str = "interactive.execute";

/// This fixture declaration is an independent consumer's expectation of execution service 1.0.
pub fn methods() -> Value {
    json!({"execute": {
        "parameters": {"type":"record", "fields": {
            "program":{"type":"string","max_bytes":4096},
            "args":{"type":"array","max_items":128,"items":{"type":"string","max_bytes":4096}},
            "cwd":{"type":"string","max_bytes":4096},
            "name":{"type":"string","max_bytes":256}
        }, "optional":["cwd","name"]},
        "result":{"type":"record","fields":{
            "session":{"type":"string","max_bytes":128},
            "state":{"type":"string","max_bytes":32}
        }},
        "permissions":["process.exec","ui.panels"]
    }})
}

/// Repackage through admission so tests cannot silently mutate a validated manifest in memory.
pub fn archive(files: BTreeMap<String, Vec<u8>>, manifest: Value) -> Package {
    let mut files = files;
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&zip.finish().unwrap().into_inner()).unwrap()
}

/// The same SDK guest calls any selected implementation, including a separately packaged alternative.
pub fn fixture(id: &str, provider: bool, execution: bool) -> Package {
    let files = Package::read(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
    )
    .unwrap()
    .files;
    let mut manifest: Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["id"] = json!(id);
    manifest["name"] = json!(id);
    manifest["settings_hook"] = json!(false);
    manifest["api"]["required"]["plugin.services"] = json!("^1");
    manifest["api"]["required"]["process"] = json!(">=1.2, <2");
    manifest["permissions"] = if execution {
        json!(["assets.read", "services.call", "process.exec", "ui.panels"])
    } else {
        json!(["assets.read", "services.call", "ui.panels"])
    };
    if provider {
        // A revoked callback must not regain this otherwise available private-data authority.
        manifest["permissions"]
            .as_array_mut()
            .unwrap()
            .push(json!("storage"));
    }
    let mut methods = methods();
    if !execution {
        // A consumer cannot weaken authority and still match the real execution contract.
        methods["execute"]["permissions"] = json!(["ui.panels"]);
    }
    manifest["plugin_services"] = if provider {
        json!({"provides": {CONTRACT:{"version":"1.0.0","methods":methods}}})
    } else {
        json!({"requires": {CONTRACT:{"version":"^1","optional":true,"methods":methods}}})
    };
    archive(files, manifest)
}
