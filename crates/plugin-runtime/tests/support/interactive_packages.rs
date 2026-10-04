//! Independent consumers describe the contract, never the terminal package or its private state.
use plugin_runtime::Package;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
    path::Path,
};

pub const CONTRACT: &str = "interactive.execute";

/// This fixture declaration is an independent consumer's expectation of execution service 1.3.
///
/// `stop` and `status` are declared here too: a consumer that promises to end a program and to
/// report what became of it has to require both, otherwise it would match a provider that can do
/// neither.
pub fn methods() -> Value {
    json!({
        "execute": {
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
        },
        "stop": {
            "parameters":{"type":"record","fields":{
                "session":{"type":"string","max_bytes":128}
            }},
            "result":{"type":"record","fields":{
                "session":{"type":"string","max_bytes":128},
                "state":{"type":"string","max_bytes":32}
            }},
            "permissions":["process.exec"]
        },
        "status": {
            "parameters":{"type":"record","fields":{
                "session":{"type":"string","max_bytes":128}
            }},
            "result":{"type":"record","fields":{
                "session":{"type":"string","max_bytes":128},
                "state":{"type":"string","max_bytes":32},
                "code":{"type":"integer","min":0,"max":2147483647}
            }, "optional":["code"]},
            "permissions":["process.exec"]
        }
    })
}

/// The execution contract a provider must declare to serve this host, shape for shape.
///
/// A host call carries the caller's environment overrides, so a provider that cannot receive them
/// cannot serve this host: the match is exact, and writing the shape here is how an independently
/// packaged provider states that it agrees about what a call means.
pub fn provider_methods() -> Value {
    json!({
        "execute": {
            "parameters": {"type":"record", "fields": {
                "program":{"type":"string","max_bytes":4096},
                "args":{"type":"array","max_items":128,"items":{"type":"string","max_bytes":4096}},
                "cwd":{"type":"string","max_bytes":4096},
                "name":{"type":"string","max_bytes":256},
                "env":{"type":"array","max_items":64,"items":{"type":"record","fields":{
                    "name":{"type":"string","max_bytes":128},
                    "value":{"type":"string","max_bytes":32768}}}}
            }, "optional":["cwd","name","env"]},
            "result":{"type":"record","fields":{
                "session":{"type":"string","max_bytes":128},
                "state":{"type":"string","max_bytes":32}
            }},
            "permissions":["process.exec","ui.panels"]
        },
        "stop": {
            "parameters":{"type":"record","fields":{
                "session":{"type":"string","max_bytes":128}
            }},
            "result":{"type":"record","fields":{
                "session":{"type":"string","max_bytes":128},
                "state":{"type":"string","max_bytes":32}
            }},
            "permissions":["process.exec"]
        },
        "status": {
            "parameters":{"type":"record","fields":{
                "session":{"type":"string","max_bytes":128}
            }},
            "result":{"type":"record","fields":{
                "session":{"type":"string","max_bytes":128},
                "state":{"type":"string","max_bytes":32},
                "code":{"type":"integer","min":0,"max":2147483647}
            }, "optional":["code"]},
            "permissions":["process.exec"]
        }
    })
}

/// An independently packaged provider that declares exactly what this host calls.
pub fn provider(id: &str) -> Package {
    let files = Package::read(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
    )
    .unwrap()
    .files;
    let mut manifest: Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["id"] = json!(id);
    manifest["name"] = json!(id);
    // A provider is a separate package: it never declares the host's settings hook.
    manifest["settings_hook"] = json!(false);
    manifest["api"]["required"]["plugin.services"] = json!("^1");
    manifest["api"]["required"]["process"] = json!(">=1.4, <2");
    manifest["permissions"] = json!(["assets.read", "services.call", "process.exec", "ui.panels"]);
    manifest["plugin_services"] =
        json!({"provides": {CONTRACT:{"version":"1.3.0","methods":provider_methods()}}});
    archive(files, manifest)
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
    manifest["api"]["required"]["process"] = json!(">=1.3, <2");
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
        // A consumer cannot weaken authority and still match the real execution contract. It also
        // cannot end or observe a program it has no authority over, so both are dropped entirely.
        methods["execute"]["permissions"] = json!(["ui.panels"]);
        methods.as_object_mut().unwrap().remove("stop");
        methods.as_object_mut().unwrap().remove("status");
    }
    manifest["plugin_services"] = if provider {
        // A provider implements every method of the execution contract it advertises.
        json!({"provides": {CONTRACT:{"version":"1.3.0","methods":methods}}})
    } else {
        json!({"requires": {CONTRACT:{"version":">=1.3, <2","optional":true,"methods":methods}}})
    };
    archive(files, manifest)
}
