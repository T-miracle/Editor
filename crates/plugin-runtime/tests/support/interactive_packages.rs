//! Independent consumers describe the contract, never the terminal package or its private state.
use plugin_runtime::Package;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
    path::Path,
};

pub const CONTRACT: &str = "interactive.execute";

/// The same declaration with the provider's authority removed.
///
/// A method's `permissions` state what a *provider* must be granted, and admission requires them to
/// be a subset of the package that declares them. A consumer stating what it requires is not asking
/// for that authority — it is asking that the provider have it — so copying the field into a
/// requirement compiles only while it happens to match, which is why this exists rather than a
/// per-call edit.
pub fn as_requirement(methods: Value) -> Value {
    let Value::Object(methods) = methods else {
        return methods;
    };
    Value::Object(
        methods
            .into_iter()
            .map(|(name, mut method)| {
                if let Value::Object(fields) = &mut method {
                    fields.remove("permissions");
                }
                (name, method)
            })
            .collect(),
    )
}

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
                "name":{"type":"string","max_bytes":256},
                // A consumer must require the environment shape as well: the match is exact per
                // method, so a consumer that omits it matches no provider the host can call.
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

/// The debug contract the host declares, so a package can be built against exactly that shape.
///
/// This is the host's requirement restated as a package would declare it. A package that declares it
/// offers debugging; one that omits a method has not offered that ability, and the host must report
/// that rather than treating the package as unusable.
pub const DEBUG_CONTRACT: &str = "debug.session";

/// One method declaration, written as the host declares it.
fn method(parameters: Value, result: Value, permissions: &[&str]) -> Value {
    json!({
        "parameters": parameters,
        "result": result,
        "permissions": permissions,
    })
}

/// The debug methods a package may declare, keyed by name so a caller can drop one.
pub fn debug_methods() -> Value {
    let session = json!({"type":"record","fields":{
        "session":{"type":"string","max_bytes":128}}});
    let session_state = json!({"type":"record","fields":{
        "session":{"type":"string","max_bytes":128},
        "state":{"type":"string","max_bytes":32}}});
    json!({
        "start": method(
            json!({"type":"record","fields":{
                "program":{"type":"string","max_bytes":4096},
                "args":{"type":"array","max_items":128,"items":{"type":"string","max_bytes":4096}},
                "cwd":{"type":"string","max_bytes":4096},
                "name":{"type":"string","max_bytes":256},
                "env":{"type":"array","max_items":64,"items":{"type":"record","fields":{
                    "name":{"type":"string","max_bytes":128},
                    "value":{"type":"string","max_bytes":32768}}}},
                "breakpoints":{"type":"array","max_items":512,"items":{"type":"record","fields":{
                    "source":{"type":"string","max_bytes":4096},
                    "line":{"type":"integer","min":1,"max":2147483647}}}},
                "stop_on_entry":{"type":"boolean"}},
                "optional":["cwd","name","env","breakpoints","stop_on_entry"]}),
            session_state.clone(),
            &["process.exec","ui.panels"]),
        "set_breakpoints": method(
            json!({"type":"record","fields":{
                "session":{"type":"string","max_bytes":128},
                "breakpoints":{"type":"array","max_items":512,"items":{"type":"record","fields":{
                    "source":{"type":"string","max_bytes":4096},
                    "line":{"type":"integer","min":1,"max":2147483647}}}}}}),
            json!({"type":"record","fields":{
                "breakpoints":{"type":"array","max_items":512,"items":{"type":"record","fields":{
                    "source":{"type":"string","max_bytes":4096},
                    "line":{"type":"integer","min":1,"max":2147483647},
                    "verified":{"type":"boolean"}}}}}}),
            &["process.exec"]),
        "resume": method(session.clone(), session_state.clone(), &["process.exec"]),
        "pause": method(session.clone(), session_state.clone(), &["process.exec"]),
        "step": method(
            json!({"type":"record","fields":{
                "session":{"type":"string","max_bytes":128},
                "kind":{"type":"string","max_bytes":32}}}),
            session_state.clone(),
            &["process.exec"]),
        "frames": method(
            session.clone(),
            json!({"type":"record","fields":{
                "frames":{"type":"array","max_items":256,"items":{"type":"record","fields":{
                    "id":{"type":"integer","min":0,"max":2147483647},
                    "name":{"type":"string","max_bytes":512},
                    "source":{"type":"string","max_bytes":4096},
                    "line":{"type":"integer","min":1,"max":2147483647}}}}}}),
            &["process.exec"]),
        "variables": method(
            json!({"type":"record","fields":{
                "session":{"type":"string","max_bytes":128},
                "frame":{"type":"integer","min":0,"max":2147483647}}}),
            json!({"type":"record","fields":{
                "variables":{"type":"array","max_items":512,"items":{"type":"record","fields":{
                    "name":{"type":"string","max_bytes":512},
                    "value":{"type":"string","max_bytes":4096}}}}}}),
            &["process.exec"]),
        "status": method(
            session.clone(),
            json!({"type":"record","fields":{
                "session":{"type":"string","max_bytes":128},
                "state":{"type":"string","max_bytes":32},
                "reason":{"type":"string","max_bytes":64},
                "source":{"type":"string","max_bytes":4096},
                "line":{"type":"integer","min":1,"max":2147483647}},
                "optional":["reason","source","line"]}),
            &["process.exec"]),
        "stop": method(session, session_state, &["process.exec"]),
    })
}

/// The debug methods a package may offer, as `provides` declarations.
///
/// `omit` names the methods a package does not declare, which is how a package with fewer abilities
/// is built without touching the guest: the abilities are a declaration, so the package states them.
pub fn debug_provides(omit: &[&str]) -> Value {
    let mut methods = debug_methods();
    for name in omit {
        methods.as_object_mut().unwrap().remove(*name);
    }
    json!({DEBUG_CONTRACT: {"version": "1.0.0", "methods": methods}})
}

/// An independently packaged debug provider that declares the host's own shape.
pub fn debug_provider(id: &str, omit: &[&str]) -> Package {
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
    // A debug provider drives a debugger, so it needs the process authority the contract declares.
    manifest["api"]["required"]["process"] = json!(">=1.3, <2");
    manifest["permissions"] = json!(["assets.read", "services.call", "process.exec", "ui.panels"]);
    manifest["plugin_services"] = json!({ "provides": debug_provides(omit) });
    archive(files, manifest)
}

/// A consumer that requires the host's session contract as well as an execution provider.
///
/// The example guest already knows how to call a contract by name, so this only states what it is
/// allowed to open; it is a real package built from the public SDK, not a stub.
pub fn session_consumer(id: &str) -> Package {
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
    manifest["permissions"] = json!(["assets.read", "services.call", "ui.panels"]);
    manifest["plugin_services"] = json!({
        "requires": {
            CONTRACT: {"version": ">=1.3, <2", "optional": true, "methods": as_requirement(methods())},
            // The host's own session contract, required as stated so the consumer matches it.
            "session.host": {
                "version": "^1",
                "optional": false,
                "methods": session_methods()
            }
        }
    });
    archive(files, manifest)
}

/// The host's session contract as a consumer must state it to reach it.
pub fn session_methods() -> Value {
    json!({
        "start": {
            "parameters": {"type":"record","fields":{
                "program":{"type":"string","max_bytes":4096},
                "args":{"type":"array","max_items":128,"items":{"type":"string","max_bytes":4096}},
                "cwd":{"type":"string","max_bytes":4096},
                "name":{"type":"string","max_bytes":256},
                "env":{"type":"array","max_items":64,"items":{"type":"record","fields":{
                    "name":{"type":"string","max_bytes":128},
                    "value":{"type":"string","max_bytes":32768}}}}},
                "optional":["cwd","name","env"]},
            "result":{"type":"record","fields":{
                "session":{"type":"string","max_bytes":128},
                "state":{"type":"string","max_bytes":32},
                "located":{"type":"boolean"}}},
            "permissions":[]
        },
        "list": {
            "parameters":{"type":"record","fields":{}},
            "result":{"type":"record","fields":{
                "sessions":{"type":"array","max_items":64,"items":{"type":"record","fields":{
                    "session":{"type":"string","max_bytes":128},
                    "state":{"type":"string","max_bytes":32}}}}}},
            "permissions":[]
        },
        "status": {
            "parameters":{"type":"record","fields":{"session":{"type":"string","max_bytes":128}}},
            "result":{"type":"record","fields":{
                "session":{"type":"string","max_bytes":128},
                "state":{"type":"string","max_bytes":32}}},
            "permissions":[]
        },
        "stop": {
            "parameters":{"type":"record","fields":{"session":{"type":"string","max_bytes":128}}},
            "result":{"type":"record","fields":{
                "session":{"type":"string","max_bytes":128},
                "state":{"type":"string","max_bytes":32}}},
            "permissions":[]
        }
    })
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
        // What is left states no provider authority at all, which is what a consumer's requirement
        // means: the shape it needs, not the grants the provider must hold.
        methods["execute"]
            .as_object_mut()
            .unwrap()
            .remove("permissions");
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
