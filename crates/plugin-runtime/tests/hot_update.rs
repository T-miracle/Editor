//! Real SDK guests combine private data, native views, service calls and an owned LSP across cutover.
use plugin_runtime::{
    InstallControl, Manager, Package,
    plugin_protocol::{Environment, ui::Kind},
};
use serde_json::{Value, json};
use std::{
    io::{BufRead, Cursor, Read, Write},
    path::Path,
    sync::Arc,
};

#[path = "support/service_packages.rs"]
mod service_packages;

/// Repack the independent SDK component: behavior comes from assets, never a host test branch.
fn mixed_package(executable: &Path, log: &Path, version: u32, policy: &str) -> Package {
    let mut files = service_packages::package("mixed-provider", true, false, "1.0.0").files;
    let mut manifest: Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["version"] = json!(format!("{version}.0.0"));
    manifest["data_format"] = json!({"version":version,"migration_hook":true});
    for capability in ["storage.migration", "language.lsp", "process"] {
        manifest["api"]["required"][capability] = json!("^1");
    }
    manifest["permissions"]
        .as_array_mut()
        .unwrap()
        .push(json!("process.service.analysis"));
    manifest["services"] = json!({"analysis":{"program":executable,"args":[log]}});
    manifest["language_servers"] =
        json!([{"id":"analysis","language":"hot-update-language","service":"analysis"}]);
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    files.insert("migration-policy.txt".into(), policy.as_bytes().to_vec());
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        archive
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&archive.finish().unwrap().into_inner()).unwrap()
}

/// Observe public guest views instead of inspecting its WASM memory or private host implementation.
fn text(manager: &Manager, id: &str) -> String {
    let document = manager.live[id].views["welcome"].as_ref();
    let Kind::Text { text } = &document.root.kind else {
        panic!("expected guest status view")
    };
    text.clone()
}

fn command(manager: &mut Manager, id: &str, name: &str, args: Value) -> String {
    manager.invoke_command(id, name, args).unwrap();
    text(manager, id)
}

/// A real stdio handshake proves the approved lease launches a functioning transport, not just a handle.
fn handshake(input: &mut impl Write, output: &mut impl Read) {
    let request =
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
            .unwrap();
    write!(input, "Content-Length: {}\r\n\r\n", request.len()).unwrap();
    input.write_all(&request).unwrap();
    input.flush().unwrap();
    let mut reader = std::io::BufReader::new(output);
    let mut length = None;
    loop {
        let mut line = String::new();
        assert!(
            reader.read_line(&mut line).unwrap() > 0,
            "fixture exited before responding"
        );
        if line == "\r\n" {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = Some(value.trim().parse::<usize>().unwrap());
        }
    }
    let mut payload = vec![0; length.unwrap()];
    reader.read_exact(&mut payload).unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&payload).unwrap()["id"], 1);
}

/// Failed activation restores data and contributions, but never revives old native/request ownership.
#[test]
#[ignore = "build the independent capability-example SDK guest and lsp_fixture first"]
fn mixed_update_rollback_replaces_all_owners_and_preserves_latest_data() {
    let root = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("source.txt"), "workspace").unwrap();
    let executable = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug/examples/lsp_fixture.exe")
        .canonicalize()
        .unwrap();
    let mut manager = Manager::open(
        root.path().into(),
        Environment {
            workspace: workspace.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let log = root.path().join("wire.jsonl");
    let old = mixed_package(&executable, &log, 1, "ok");
    manager
        .install(&old, old.manifest.permissions.clone())
        .unwrap();
    let consumer = service_packages::package("service-consumer", false, false, "^1");
    manager
        .install(&consumer, consumer.manifest.permissions.clone())
        .unwrap();
    assert_eq!(
        command(
            &mut manager,
            "service-consumer",
            "service-open",
            json!("example.echo")
        ),
        "Service opened"
    );
    let old_identity = manager.instance_id("mixed-provider").unwrap().to_owned();
    let old_lease = manager.language_services()["mixed-provider/analysis"]
        .as_ref()
        .unwrap()
        .clone();
    let (_old_process, mut input, mut output) = old_lease.spawn().unwrap();
    handshake(&mut input, &mut output);
    command(
        &mut manager,
        "mixed-provider",
        "scope-write",
        json!({"text":"before"}),
    );

    let candidate = mixed_package(&executable, &log, 2, "activate-fail");
    let control = InstallControl::default();
    let prepared = manager
        .prepare_installation(&candidate, candidate.manifest.permissions.clone(), &control)
        .unwrap();
    // The prepared candidate has no publication authority; commands and service routing still use the old guest.
    command(
        &mut manager,
        "mixed-provider",
        "scope-write",
        json!({"text":"latest"}),
    );
    assert!(old_lease.is_active());
    assert_eq!(
        manager.instance_id("mixed-provider"),
        Some(old_identity.as_str())
    );
    assert_eq!(
        command(
            &mut manager,
            "service-consumer",
            "service-call",
            json!({"method":"editor-continuation","value":"","timeout_ms":30000})
        ),
        "Accepted"
    );
    manager.poll();
    let pending = manager
        .live
        .get_mut("mixed-provider")
        .unwrap()
        .take_editor_requests()
        .pop()
        .unwrap();

    assert!(manager.commit_installation(prepared, &control).is_err());
    assert_eq!(manager.installed["mixed-provider"].digest, old.digest);
    assert_ne!(
        manager.instance_id("mixed-provider"),
        Some(old_identity.as_str())
    );
    assert!(
        !pending.begin(),
        "published work cannot survive the retired provider incarnation"
    );
    assert!(
        !old_lease.is_active(),
        "rollback must retire the pre-cutover native lease"
    );
    let restored = manager.language_services()["mixed-provider/analysis"]
        .as_ref()
        .unwrap()
        .clone();
    assert!(!Arc::ptr_eq(&old_lease, &restored));
    assert!(restored.is_active());
    let (_new_process, mut new_input, mut new_output) = restored.spawn().unwrap();
    handshake(&mut new_input, &mut new_output);
    assert_eq!(
        command(&mut manager, "mixed-provider", "scope-read", Value::Null),
        "workspace|latest"
    );
    assert!(
        command(
            &mut manager,
            "service-consumer",
            "service-call",
            json!({"method":"echo","value":"stale","timeout_ms":30000})
        )
        .contains("InvalidHandle")
    );
    assert_eq!(
        command(
            &mut manager,
            "service-consumer",
            "service-open",
            json!("example.echo")
        ),
        "Service opened"
    );
    command(
        &mut manager,
        "service-consumer",
        "service-call",
        json!({"method":"echo","value":"fresh","timeout_ms":30000}),
    );
    manager.poll();
    assert!(text(&manager, "service-consumer").contains("mixed-provider:fresh"));
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("source.txt")).unwrap(),
        "workspace"
    );
    // Corrupt an old package asset to inject a distinct recovery failure through ordinary guest activation.
    // Both errors must survive in the installed record; no test-only runtime branch performs this injection.
    std::fs::write(
        root.path()
            .join("packages/mixed-provider")
            .join(&old.digest)
            .join("welcome.txt"),
        [0xff],
    )
    .unwrap();
    let failure = manager
        .install(&candidate, candidate.manifest.permissions.clone())
        .unwrap_err();
    let message = format!("{failure:#}");
    assert!(
        message.contains("Update failed:") && message.contains("restart failed:"),
        "{message}"
    );
    assert_eq!(
        manager.installed["mixed-provider"].error.as_deref(),
        Some(message.as_str())
    );
    assert!(manager.instance_id("mixed-provider").is_none());
    assert!(!restored.is_active());
    assert!(!manager.live.contains_key("mixed-provider"));
}
