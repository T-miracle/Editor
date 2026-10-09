//! Native selections grant exact, revocable authority through a real SDK package and Manager.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, api, ui::Kind},
};
use serde_json::{Value, json};
use std::{
    io::{Cursor, Write},
    path::Path,
};

/// The production probe component reports public host results in its ordinary view.
fn package(id: &str) -> Package {
    package_with(id, |_| {})
}

/// Declaration mutations exercise admission without changing the real guest transport.
fn package_with(id: &str, edit: impl FnOnce(&mut Value)) -> Package {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/plugin-api-test");
    let mut archives = std::fs::read_dir(folder)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("capability-example-")
        })
        .collect::<Vec<_>>();
    archives.sort();
    let mut files = Package::read(
        archives
            .last()
            .expect("build current capability-example first"),
    )
    .unwrap()
    .files;
    let mut manifest: Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["id"] = json!(id);
    manifest["settings"] = json!({});
    manifest["settings_hook"] = json!(false);
    manifest["api"]["required"] =
        json!({"package.assets":"^1", "ui.native":"^1", "files.selection":"^1"});
    manifest["api"]["optional"] = json!({});
    manifest["permissions"] = json!(["assets.read", "files.select"]);
    // New demo contributions are independent of these explicit authority consumers.
    manifest["commands"] = json!([
        {"id":"scope-probe","title":"Probe"},
        {"id":"service-open","title":"Open service"},
        {"id":"service-call","title":"Call service"}
    ]);
    edit(&mut manifest);
    files.insert("service-label.txt".into(), b"Selection provider".to_vec());
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

fn manager(root: &Path) -> Manager {
    Manager::open(
        root.join("runtime"),
        Environment {
            workspace: root.display().to_string(),
            ..Default::default()
        },
    )
    .unwrap()
}

fn install(manager: &mut Manager, id: &str) {
    let package = package(id);
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
}

fn select_operation(mode: &str) -> Value {
    json!({"method":"editor", "timeout_ms":30000, "operation":{"kind":"interaction",
        "operation":{"kind":"select", "title":"Choose", "mode":mode, "multiple":false, "suggested_name":null}}})
}

fn start_selection(manager: &mut Manager, id: &str, mode: &str) -> plugin_runtime::EditorRequest {
    assert!(matches!(
        invoke(manager, id, select_operation(mode)).unwrap(),
        api::Value::Accepted(_)
    ));
    let request = manager
        .live
        .get_mut(id)
        .unwrap()
        .take_editor_requests()
        .pop()
        .unwrap();
    assert!(request.begin());
    request
}

fn selected_handle(
    manager: &mut Manager,
    id: &str,
    mode: &str,
    path: &Path,
) -> api::ResourceHandle {
    start_selection(manager, id, mode).finish_selection(vec![path.to_path_buf()]);
    manager.poll();
    let update: api::RequestUpdate = serde_json::from_str(&text(manager, id)).unwrap();
    let api::RequestUpdate::Completed {
        result:
            Ok(api::EditorValue::Interaction(
                plugin_runtime::plugin_protocol::interaction::Value::Selected(resources),
            )),
    } = update
    else {
        panic!("selection failed: {update:?}")
    };
    resources.into_iter().next().unwrap().handle
}

fn read(
    manager: &mut Manager,
    id: &str,
    handle: &api::ResourceHandle,
    path: &str,
) -> Result<api::Value, api::Failure> {
    invoke(
        manager,
        id,
        json!({"method":"read_file", "handle":handle,"path":path}),
    )
}

/// A directory grant permits descendants, not traversal, streams, device names or absolute paths.
#[test]
#[ignore = "build capability-example through current host --plugin-package first"]
fn selected_directory_reads_descendants_but_rejects_path_aliases() {
    let root = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    std::fs::create_dir(external.path().join("child")).unwrap();
    std::fs::write(external.path().join("child/data.txt"), b"nested bytes").unwrap();
    let mut manager = manager(root.path());
    install(&mut manager, "directory-reader");
    let handle = selected_handle(
        &mut manager,
        "directory-reader",
        "directory",
        external.path(),
    );
    assert!(
        matches!(read(&mut manager,"directory-reader",&handle,"child/data.txt").unwrap(), api::Value::Bytes(bytes) if bytes == b"nested bytes")
    );
    for path in [
        "../outside",
        "/outside",
        "C:/outside",
        "child/data.txt:secret",
        "CON",
        "child/../data.txt",
        "child\\data.txt",
        "\\\\.\\NUL",
    ] {
        assert!(
            read(&mut manager, "directory-reader", &handle, path).is_err(),
            "accepted {path}"
        );
    }
}

/// Forgery, cross-instance borrowing and explicit release cannot retain authority.
#[test]
#[ignore = "build capability-example through current host --plugin-package first"]
fn selected_handles_are_owned_and_released() {
    let root = tempfile::tempdir().unwrap();
    let external = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(external.path(), b"private choice").unwrap();
    let mut manager = manager(root.path());
    install(&mut manager, "first-reader");
    install(&mut manager, "other-reader");
    let handle = selected_handle(&mut manager, "first-reader", "file", external.path());
    assert_eq!(
        read(&mut manager, "other-reader", &handle, "")
            .unwrap_err()
            .code,
        api::ErrorCode::InvalidHandle
    );
    let mut forged = handle.clone();
    forged.resource += 999;
    assert_eq!(
        read(&mut manager, "first-reader", &forged, "")
            .unwrap_err()
            .code,
        api::ErrorCode::InvalidHandle
    );
    invoke(
        &mut manager,
        "first-reader",
        json!({"method":"close_resource","handle":handle}),
    )
    .unwrap();
    assert_eq!(
        read(&mut manager, "first-reader", &handle, "")
            .unwrap_err()
            .code,
        api::ErrorCode::InvalidHandle
    );
}

/// Late native callbacks after cancellation allocate no selected target and cannot revive a request.
#[test]
#[ignore = "build capability-example through current host --plugin-package first"]
fn cancelled_selection_has_no_late_authority() {
    let root = tempfile::tempdir().unwrap();
    let external = tempfile::NamedTempFile::new().unwrap();
    let mut manager = manager(root.path());
    install(&mut manager, "cancel-reader");
    let baseline = manager.live["cancel-reader"].resource_count();
    let request = start_selection(&mut manager, "cancel-reader", "file");
    request.cancel_from_host(api::CancelMode::TryTerminate);
    request.finish_selection(vec![external.path().to_path_buf()]);
    manager.poll();
    assert!(matches!(
        serde_json::from_str::<api::RequestUpdate>(&text(&manager, "cancel-reader")).unwrap(),
        api::RequestUpdate::Cancelled { .. }
    ));
    assert_eq!(manager.live["cancel-reader"].resource_count(), baseline);
}

/// Disable and workspace trust revocation retire old handles even after a replacement is enabled.
#[test]
#[ignore = "build capability-example through current host --plugin-package first"]
fn selected_authority_does_not_survive_disable_or_trust_revocation() {
    let root = tempfile::tempdir().unwrap();
    let external = tempfile::NamedTempFile::new().unwrap();
    let mut manager = manager(root.path());
    install(&mut manager, "lifetime-reader");
    let handle = selected_handle(&mut manager, "lifetime-reader", "file", external.path());
    manager.disable("lifetime-reader").unwrap();
    manager.enable("lifetime-reader").unwrap();
    assert!(read(&mut manager, "lifetime-reader", &handle, "").is_err());
    let handle = selected_handle(&mut manager, "lifetime-reader", "file", external.path());
    let pending = start_selection(&mut manager, "lifetime-reader", "file");
    manager.set_workspace_trust(false).unwrap();
    pending.finish_selection(vec![external.path().to_path_buf()]);
    assert!(pending.status().is_terminal());
    manager.set_workspace_trust(true).unwrap();
    assert!(read(&mut manager, "lifetime-reader", &handle, "").is_err());
}

/// A save destination is a future transaction intent, never a backdoor to direct disk IO.
#[test]
#[ignore = "build capability-example through current host --plugin-package first"]
fn selected_save_does_not_read_or_write_outside_document_transactions() {
    let root = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let mut manager = manager(root.path());
    install(&mut manager, "save-consumer");
    let target = external.path().join("new.txt");
    let handle = selected_handle(&mut manager, "save-consumer", "save", &target);
    assert_eq!(
        read(&mut manager, "save-consumer", &handle, "")
            .unwrap_err()
            .code,
        api::ErrorCode::PermissionDenied
    );
    assert_eq!(
        invoke(
            &mut manager,
            "save-consumer",
            json!({"method":"write_file","handle":handle,"path":"","bytes":[120]})
        )
        .unwrap_err()
        .code,
        api::ErrorCode::UnsupportedOperation
    );
    assert!(!target.exists());
}

/// Delegated calls cannot select or consume even the provider's otherwise valid selected handles.
#[test]
#[ignore = "build capability-example through current host --plugin-package first"]
fn selected_authority_cannot_be_delegated_by_service_or_typed_command() {
    let root = tempfile::tempdir().unwrap();
    let external = tempfile::NamedTempFile::new().unwrap();
    let mut manager = manager(root.path());
    // Selected-file permission is deliberately not a delegatable service grant; both packages
    // possess it directly, but even their valid private handles remain unavailable in a call.
    let signature = json!({"parameters":{"type":"string","max_bytes":4096}, "result":{"type":"string","max_bytes":4096}, "permissions":[]});
    let provider = package_with("selection-provider", |manifest| {
        manifest["api"]["required"]["plugin.services"] = json!("^1.1");
        manifest["api"]["required"]["plugin.commands"] = json!("^1");
        let mut command_signature = signature.clone();
        // A typed signature may require the provider's own picker grant, while a peer still
        // cannot acquire or consume selected resources through the delegated callback.
        command_signature["permissions"] = json!(["files.select"]);
        manifest["commands"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"probe","title":"Probe","signature":command_signature}));
        manifest["plugin_services"] = json!({"provides":{"example.selection":{"version":"1.0.0","methods":{"probe":signature}}}});
    });
    manager
        .install(&provider, provider.manifest.permissions.clone())
        .unwrap();
    let consumer = package_with("selection-consumer", |manifest| {
        manifest["api"]["required"]["plugin.services"] = json!("^1.1");
        manifest["api"]["required"]["plugin.commands"] = json!("^1");
        manifest["permissions"]
            .as_array_mut()
            .unwrap()
            .push(json!("services.call"));
        manifest["permissions"]
            .as_array_mut()
            .unwrap()
            .push(json!("commands.call"));
        manifest["plugin_services"] = json!({"requires":{"example.selection":{"version":"^1","methods":{"probe":signature}}}});
    });
    manager
        .install(&consumer, consumer.manifest.permissions.clone())
        .unwrap();
    let handle = selected_handle(&mut manager, "selection-provider", "file", external.path());
    manager
        .invoke_command(
            "selection-consumer",
            "service-open",
            json!("example.selection"),
        )
        .unwrap();
    for operation in [
        select_operation("file"),
        json!({"method":"read_file","handle":handle,"path":""}),
    ] {
        assert!(matches!(invoke(&mut manager,"selection-consumer",json!({"method":"commands","operation":{
            "kind":"invoke","plugin":"selection-provider","command":"probe","arguments":operation.to_string(),"timeout_ms":30000
        }})).unwrap(),api::Value::Accepted(_)));
        for _ in 0..4 {
            manager.poll();
        }
        let update: api::RequestUpdate<Value> =
            serde_json::from_str(&text(&manager, "selection-consumer")).unwrap();
        let api::RequestUpdate::Completed { result: Ok(result) } = update else {
            panic!("typed probe failed: {update:?}")
        };
        let result: Result<api::Value, api::Failure> =
            serde_json::from_str(result.as_str().unwrap()).unwrap();
        assert_eq!(result.unwrap_err().code, api::ErrorCode::PermissionDenied);
        manager
            .invoke_command(
                "selection-consumer",
                "service-call",
                json!({"method":"probe","value":operation.to_string(),"timeout_ms":30000}),
            )
            .unwrap();
        for _ in 0..4 {
            manager.poll();
        }
        let update: api::RequestUpdate<Value> =
            serde_json::from_str(&text(&manager, "selection-consumer")).unwrap();
        let api::RequestUpdate::Completed { result: Ok(result) } = update else {
            panic!("service probe failed: {update:?}")
        };
        let result: Result<api::Value, api::Failure> =
            serde_json::from_str(result.as_str().unwrap()).unwrap();
        assert_eq!(result.unwrap_err().code, api::ErrorCode::PermissionDenied);
    }
    assert!(
        manager
            .live
            .get_mut("selection-provider")
            .unwrap()
            .take_editor_requests()
            .is_empty()
    );
}

/// A public Manager host command can use its own installed selection grant without delegating it.
#[test]
#[ignore = "build capability-example through current host --plugin-package first"]
fn direct_host_typed_command_selects_reads_and_releases_its_own_resource() {
    let root = tempfile::tempdir().unwrap();
    let external = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(external.path(), b"host choice").unwrap();
    let mut manager = manager(root.path());
    let package = package_with("host-selection-provider", |manifest| {
        manifest["api"]["required"]["plugin.commands"] = json!("^1");
        manifest["commands"].as_array_mut().unwrap().push(json!({
            "id":"probe", "title":"Probe", "signature":{
                "parameters":{"type":"string","max_bytes":4096},
                "result":{"type":"string","max_bytes":4096},
                "permissions":["files.select"]
            }
        }));
    });
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let invoke_host = |manager: &mut Manager, operation: Value| {
        let completion = manager
            .invoke_typed_command(
                "host-selection-provider",
                "probe",
                json!(operation.to_string()),
                30000,
            )
            .unwrap();
        for _ in 0..4 {
            manager.poll();
        }
        let api::RequestUpdate::Completed { result: Ok(value) } = completion.status() else {
            panic!("host command did not complete: {:?}", completion.status())
        };
        serde_json::from_str::<Result<api::Value, api::Failure>>(value.as_str().unwrap()).unwrap()
    };
    assert!(matches!(
        invoke_host(&mut manager, select_operation("file")).unwrap(),
        api::Value::Accepted(_)
    ));
    let request = manager
        .live
        .get_mut("host-selection-provider")
        .unwrap()
        .take_editor_requests()
        .pop()
        .unwrap();
    assert!(request.begin());
    request.finish_selection(vec![external.path().to_path_buf()]);
    manager.poll();
    let update: api::RequestUpdate =
        serde_json::from_str(&text(&manager, "host-selection-provider")).unwrap();
    let api::RequestUpdate::Completed {
        result:
            Ok(api::EditorValue::Interaction(
                plugin_runtime::plugin_protocol::interaction::Value::Selected(selected),
            )),
    } = update
    else {
        panic!("host selection failed: {update:?}")
    };
    let handle = &selected[0].handle;
    assert!(
        matches!(invoke_host(&mut manager, json!({"method":"read_file","handle":handle,"path":""})).unwrap(), api::Value::Bytes(bytes) if bytes == b"host choice")
    );
    invoke_host(
        &mut manager,
        json!({"method":"close_resource","handle":handle}),
    )
    .unwrap();
    assert_eq!(
        invoke_host(
            &mut manager,
            json!({"method":"read_file","handle":handle,"path":""})
        )
        .unwrap_err()
        .code,
        api::ErrorCode::InvalidHandle
    );
}

/// A selected directory never authorizes a junction's destination, including native-selected links.
#[cfg(windows)]
#[test]
#[ignore = "build capability-example through current host --plugin-package first"]
fn selected_directory_rejects_reparse_escape() {
    use std::os::windows::process::CommandExt;
    let root = tempfile::tempdir().unwrap();
    let selected = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), b"outside").unwrap();
    let junction = selected.path().join("redirect");
    let created = std::process::Command::new("powershell.exe").creation_flags(0x08000000)
        .args(["-NoProfile","-NonInteractive","-Command","$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:NANOBUG_SELECTION_LINK -Value $env:NANOBUG_SELECTION_TARGET | Out-Null"])
        .env("NANOBUG_SELECTION_LINK",&junction).env("NANOBUG_SELECTION_TARGET",outside.path()).output().unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let mut manager = manager(root.path());
    install(&mut manager, "link-reader");
    let handle = selected_handle(&mut manager, "link-reader", "directory", selected.path());
    assert_eq!(
        read(&mut manager, "link-reader", &handle, "redirect/secret.txt")
            .unwrap_err()
            .code,
        api::ErrorCode::InvalidPath
    );
    let baseline = manager.live["link-reader"].resource_count();
    start_selection(&mut manager, "link-reader", "file")
        .finish_selection(vec![junction.join("secret.txt")]);
    manager.poll();
    assert!(matches!(
        serde_json::from_str::<api::RequestUpdate>(&text(&manager, "link-reader")).unwrap(),
        api::RequestUpdate::Completed {
            result: Err(api::Failure {
                code: api::ErrorCode::InvalidPath,
                ..
            })
        }
    ));
    assert_eq!(manager.live["link-reader"].resource_count(), baseline);
}

fn text(manager: &Manager, id: &str) -> String {
    let Kind::Text { text } = &manager.live[id].views["welcome"].root.kind else {
        panic!("expected probe result")
    };
    text.clone()
}

fn invoke(manager: &mut Manager, id: &str, operation: Value) -> Result<api::Value, api::Failure> {
    manager
        .invoke_command(id, "scope-probe", operation)
        .unwrap();
    serde_json::from_str(&text(manager, id)).unwrap()
}

/// Picking one external file cannot disclose its sibling or grant private/workspace write access.
#[test]
#[ignore = "build capability-example through current host --plugin-package first"]
fn selected_file_reads_only_the_selected_target() {
    let root = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let selected = external.path().join("chosen.txt");
    std::fs::write(&selected, "chosen bytes").unwrap();
    std::fs::write(external.path().join("secret.txt"), "unselected secret").unwrap();
    let mut manager = Manager::open(
        root.path().join("runtime"),
        Environment {
            workspace: root.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let package = package("selection-reader");
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    assert!(matches!(invoke(&mut manager, "selection-reader", json!({"method":"editor", "timeout_ms":30000,
        "operation":{"kind":"interaction", "operation":{"kind":"select", "title":"Open", "mode":"file", "multiple":false, "suggested_name":null}}})).unwrap(), api::Value::Accepted(_)));
    let request = manager
        .live
        .get_mut("selection-reader")
        .unwrap()
        .take_editor_requests()
        .pop()
        .unwrap();
    assert!(request.begin());
    request.finish_selection(vec![selected]);
    manager.poll();
    let update: api::RequestUpdate =
        serde_json::from_str(&text(&manager, "selection-reader")).unwrap();
    let api::RequestUpdate::Completed {
        result:
            Ok(api::EditorValue::Interaction(
                plugin_runtime::plugin_protocol::interaction::Value::Selected(selected),
            )),
    } = update
    else {
        panic!("expected selected handles: {update:?}")
    };
    let handle = &selected[0].handle;
    assert!(
        matches!(invoke(&mut manager, "selection-reader", json!({"method":"read_file", "handle":handle,"path":""})).unwrap(), api::Value::Bytes(bytes) if bytes == b"chosen bytes")
    );
    assert!(
        invoke(
            &mut manager,
            "selection-reader",
            json!({"method":"read_file", "handle":handle,"path":"secret.txt"})
        )
        .is_err()
    );
}

/// Negotiating a picker interface does not substitute for installation consent.
#[test]
#[ignore = "build capability-example through current host --plugin-package first"]
fn selection_without_installation_permission_never_reaches_native_picker() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = manager(root.path());
    let package = package_with("ungranted-reader", |manifest| {
        manifest["permissions"] = json!(["assets.read"]);
    });
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    assert_eq!(
        invoke(&mut manager, "ungranted-reader", select_operation("file"))
            .unwrap_err()
            .code,
        api::ErrorCode::PermissionDenied
    );
    assert!(
        manager
            .live
            .get_mut("ungranted-reader")
            .unwrap()
            .take_editor_requests()
            .is_empty()
    );
}

/// A timeout or malformed native batch must not leave a valid prefix of grants behind.
#[test]
#[ignore = "build capability-example through current host --plugin-package first"]
fn expired_or_invalid_selection_leaves_no_grants() {
    let root = tempfile::tempdir().unwrap();
    let external = tempfile::NamedTempFile::new().unwrap();
    let mut manager = manager(root.path());
    install(&mut manager, "bounded-reader");
    let baseline = manager.live["bounded-reader"].resource_count();
    let pending = start_selection(&mut manager, "bounded-reader", "file");
    pending.expire(std::time::Instant::now() + std::time::Duration::from_secs(301));
    pending.finish_selection(vec![external.path().to_path_buf()]);
    manager.poll();
    assert!(matches!(
        serde_json::from_str::<api::RequestUpdate>(&text(&manager, "bounded-reader")).unwrap(),
        api::RequestUpdate::Cancelled {
            reason: api::ErrorCode::TimedOut,
            ..
        }
    ));
    assert_eq!(manager.live["bounded-reader"].resource_count(), baseline);
    let pending = start_selection(&mut manager, "bounded-reader", "file");
    pending.finish_selection(vec![
        external.path().to_path_buf(),
        external.path().to_path_buf(),
    ]);
    manager.poll();
    assert!(matches!(
        serde_json::from_str::<api::RequestUpdate>(&text(&manager, "bounded-reader")).unwrap(),
        api::RequestUpdate::Completed {
            result: Err(api::Failure {
                code: api::ErrorCode::InvalidRequest,
                ..
            })
        }
    ));
    assert_eq!(manager.live["bounded-reader"].resource_count(), baseline);
}

/// Merely choosing a file never locks normal rename/atomic-save; replacement invalidates old authority.
#[test]
#[ignore = "build capability-example through current host --plugin-package first"]
fn selected_file_allows_normal_replacement_but_never_authorizes_the_new_object() {
    let root = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let target = external.path().join("chosen.txt");
    std::fs::write(&target, b"original").unwrap();
    let mut manager = manager(root.path());
    install(&mut manager, "identity-reader");
    let handle = selected_handle(&mut manager, "identity-reader", "file", &target);
    std::fs::rename(&target, external.path().join("renamed.txt")).unwrap();
    std::fs::write(&target, b"replacement").unwrap();
    assert_eq!(
        read(&mut manager, "identity-reader", &handle, "")
            .unwrap_err()
            .code,
        api::ErrorCode::InvalidHandle
    );
    let handle = selected_handle(&mut manager, "identity-reader", "file", &target);
    let mut replacement = tempfile::NamedTempFile::new_in(external.path()).unwrap();
    replacement.write_all(b"atomic replacement").unwrap();
    replacement.persist(&target).unwrap();
    assert_eq!(
        read(&mut manager, "identity-reader", &handle, "")
            .unwrap_err()
            .code,
        api::ErrorCode::InvalidHandle
    );
    assert_eq!(std::fs::read(&target).unwrap(), b"atomic replacement");
}
