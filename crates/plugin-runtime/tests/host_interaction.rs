//! Real SDK-built packages enter native host interaction through the public Manager boundary.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, api, ui::Kind},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
    path::Path,
};

/// Re-identification changes declarations, never the SDK-built component or request transport.
fn package(id: &str, edit: impl FnOnce(&mut Value)) -> Package {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/plugin-api-test");
    let archive = folder.join("capability-example-0.18.0.zip");
    let mut files: BTreeMap<_, _> = Package::read(&archive).unwrap().files;
    let mut manifest: Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["id"] = json!(id);
    manifest["version"] = json!("1.0.0");
    manifest["settings"] = json!({});
    manifest["settings_hook"] = json!(false);
    manifest["api"]["required"] =
        json!({"package.assets":"^1", "ui.native":"^1", "ui.interaction":"^1"});
    manifest["api"]["optional"] = json!({});
    manifest["permissions"] = json!(["assets.read", "ui.interaction", "files.select"]);
    // Individual consumers declare only the operations each behavioral case exercises.
    manifest["commands"] =
        json!([{"id":"scope-probe","title":"Probe"},{"id":"command-release","title":"Release"}]);
    edit(&mut manifest);
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

/// The result visible in the guest's ordinary native view is the public request result.
fn text(manager: &Manager, id: &str) -> String {
    let Kind::Text { text } = &manager.live[id].views["welcome"].root.kind else {
        panic!("Expected native result text")
    };
    text.clone()
}

/// A valid input returns acceptance first, then a typed confirmed value from the host consumer.
#[test]
#[ignore = "build capability-example through current --plugin-package before running"]
fn native_input_returns_accepted_then_confirmed_value() {
    let directory = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        directory.path().join("runtime"),
        Environment {
            workspace: directory.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let package = package("input-consumer", |_| {});
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    manager.invoke_command("input-consumer", "scope-probe", json!({
        "method":"editor", "timeout_ms":30000,
        "operation":{"kind":"interaction", "operation":{"kind":"input", "title":"Name", "value":"", "placeholder":null, "password":false, "max_bytes":64}}
    })).unwrap();
    let result: Result<api::Value, api::Failure> =
        serde_json::from_str(&text(&manager, "input-consumer")).unwrap();
    assert!(matches!(result.unwrap(), api::Value::Accepted(_)));
    let request = manager
        .live
        .get_mut("input-consumer")
        .unwrap()
        .take_editor_requests()
        .pop()
        .unwrap();
    assert!(request.begin());
    request.finish(Ok(api::EditorValue::Interaction(
        plugin_runtime::plugin_protocol::interaction::Value::Input("你好".into()),
    )));
    manager.poll();
    let result: api::RequestUpdate =
        serde_json::from_str(&text(&manager, "input-consumer")).unwrap();
    assert!(
        matches!(result, api::RequestUpdate::Completed { result: Ok(api::EditorValue::Interaction(plugin_runtime::plugin_protocol::interaction::Value::Input(value))) } if value == "你好")
    );
}

/// Discovery reads installed typed metadata, never executing the provider or borrowing its grants.
#[test]
#[ignore = "build capability-example through current --plugin-package before running"]
fn typed_command_discovery_reports_the_exact_declared_schema() {
    let directory = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        directory.path().join("runtime"),
        Environment {
            workspace: directory.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let package = package("typed-provider", |manifest| {
        manifest["api"]["required"]["plugin.commands"] = json!("^1");
        manifest["commands"].as_array_mut().unwrap().push(json!({"id":"echo", "title":"Echo", "signature":{
            "parameters":{"type":"string","max_bytes":64}, "result":{"type":"string","max_bytes":64},"permissions":[]
        }}));
    });
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    manager
        .invoke_command(
            "typed-provider",
            "scope-probe",
            json!({"method":"commands", "operation":{"kind":"discover"}}),
        )
        .unwrap();
    let result: Result<api::Value, api::Failure> =
        serde_json::from_str(&text(&manager, "typed-provider")).unwrap();
    let api::Value::Commands(descriptors) = result.unwrap() else {
        panic!("expected typed metadata")
    };
    assert_eq!(descriptors.len(), 1);
    assert_eq!(
        (&*descriptors[0].plugin, &*descriptors[0].command),
        ("typed-provider", "echo")
    );
    assert_eq!(
        serde_json::to_value(&descriptors[0].signature.parameters).unwrap(),
        json!({"type":"string","max_bytes":64})
    );
    assert_eq!(
        serde_json::to_value(&descriptors[0].signature.result).unwrap(),
        json!({"type":"string","max_bytes":64})
    );
    // Implementation-only command contracts are not selectable services in the host settings.
    assert!(manager.service_choices().iter().all(|choice| {
        !choice
            .contract
            .starts_with(plugin_runtime::plugin_protocol::commands::CONTRACT_PREFIX)
    }));
}

/// Two independent package instances exercise arguments, values, errors and immutable cancellation.
#[test]
#[ignore = "build capability-example through current --plugin-package before running"]
fn typed_commands_validate_values_and_preserve_the_original_call_authority() {
    let directory = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        directory.path().join("runtime"),
        Environment {
            workspace: directory.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let provider = package("command-provider", |manifest| {
        manifest["api"]["required"]["plugin.commands"] = json!("^1");
        manifest["permissions"]
            .as_array_mut()
            .unwrap()
            .push(json!("workspace.read"));
        for (id, parameters, result, permissions) in [
            (
                "echo",
                json!({"type":"string","max_bytes":64}),
                json!({"type":"string","max_bytes":64}),
                json!([]),
            ),
            (
                "bad-result",
                json!({"type":"string","max_bytes":64}),
                json!({"type":"boolean"}),
                json!([]),
            ),
            (
                "fail",
                json!({"type":"null"}),
                json!({"type":"null"}),
                json!([]),
            ),
            (
                "defer",
                json!({"type":"null"}),
                json!({"type":"string","max_bytes":64}),
                json!([]),
            ),
            (
                "defer-native",
                json!({"type":"null"}),
                json!({"type":"string","max_bytes":64}),
                json!(["ui.interaction"]),
            ),
            (
                "authority",
                json!({"type":"null"}),
                json!({"type":"null"}),
                json!([]),
            ),
            (
                "needs-workspace",
                json!({"type":"null"}),
                json!({"type":"null"}),
                json!(["workspace.read"]),
            ),
        ] {
            manifest["commands"].as_array_mut().unwrap().push(json!({"id":id,"title":id,"signature":{"parameters":parameters,"result":result,"permissions":permissions}}));
        }
    });
    manager
        .install(&provider, provider.manifest.permissions.clone())
        .unwrap();
    let consumer = package("command-consumer", |manifest| {
        manifest["api"]["required"]["plugin.commands"] = json!("^1");
        manifest["permissions"]
            .as_array_mut()
            .unwrap()
            .push(json!("commands.call"));
    });
    manager
        .install(&consumer, consumer.manifest.permissions.clone())
        .unwrap();
    let invoke = |manager: &mut Manager, command: &str, arguments: Value| {
        manager.invoke_command("command-consumer", "scope-probe", json!({"method":"commands","operation":{"kind":"invoke","plugin":"command-provider","command":command,"arguments":arguments,"timeout_ms":30000}})).unwrap();
        serde_json::from_str::<Result<api::Value, api::Failure>>(&text(manager, "command-consumer"))
            .unwrap()
    };
    assert_eq!(
        invoke(&mut manager, "echo", json!(42)).unwrap_err().code,
        api::ErrorCode::InvalidRequest
    );
    assert_eq!(
        invoke(&mut manager, "needs-workspace", Value::Null)
            .unwrap_err()
            .code,
        api::ErrorCode::PermissionDenied
    );
    assert!(matches!(
        invoke(&mut manager, "echo", json!("你好")),
        Ok(api::Value::Accepted(_))
    ));
    manager.poll();
    let update: api::RequestUpdate<Value> =
        serde_json::from_str(&text(&manager, "command-consumer")).unwrap();
    assert!(
        matches!(update, api::RequestUpdate::Completed { result: Ok(value) } if value == json!("你好"))
    );
    for (command, argument, code) in [
        ("bad-result", json!("x"), api::ErrorCode::InvalidRequest),
        ("fail", Value::Null, api::ErrorCode::OperationFailed),
        ("authority", Value::Null, api::ErrorCode::PermissionDenied),
    ] {
        let wait = manager
            .invoke_typed_command("command-provider", command, argument, 30000)
            .unwrap();
        manager.poll();
        assert!(
            matches!(wait.status(), api::RequestUpdate::Completed { result: Err(error) } if error.code == code)
        );
    }
    let wait = manager
        .invoke_typed_command("command-provider", "defer", Value::Null, 30000)
        .unwrap();
    manager.poll();
    wait.cancel(api::CancelMode::TryTerminate, api::ErrorCode::Cancelled)
        .unwrap();
    manager
        .invoke_command("command-provider", "command-release", Value::Null)
        .unwrap();
    let late: Result<(), api::Failure> =
        serde_json::from_str(&text(&manager, "command-provider")).unwrap();
    assert_eq!(late.unwrap_err().code, api::ErrorCode::InvalidHandle);
    assert!(matches!(
        wait.status(),
        api::RequestUpdate::Cancelled {
            reason: api::ErrorCode::Cancelled,
            ..
        }
    ));
    let timeout = manager
        .invoke_typed_command("command-provider", "defer", Value::Null, 1)
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    manager.poll();
    assert!(matches!(
        timeout.status(),
        api::RequestUpdate::Cancelled {
            reason: api::ErrorCode::TimedOut,
            ..
        }
    ));
    let retired = manager
        .invoke_typed_command("command-provider", "echo", json!("late"), 30000)
        .unwrap();
    manager.disable("command-provider").unwrap();
    manager.poll();
    assert!(retired.status().is_terminal());
    assert!(!text(&manager, "command-consumer").contains("late"));
}

/// Cancelling the original typed wait immediately retires its pending native input.
#[test]
#[ignore = "build capability-example through current --plugin-package before running"]
fn cancelling_a_typed_command_closes_its_native_work_before_the_next_manager_tick() {
    let directory = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        directory.path().join("runtime"),
        Environment {
            workspace: directory.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let provider = package("native-command-provider", |manifest| {
        manifest["api"]["required"]["plugin.commands"] = json!("^1");
        manifest["commands"].as_array_mut().unwrap().push(json!({"id":"defer-native","title":"Native defer","signature":{
            "parameters":{"type":"null"}, "result":{"type":"string","max_bytes":64},"permissions":["ui.interaction"]
        }}));
    });
    manager
        .install(&provider, provider.manifest.permissions.clone())
        .unwrap();
    let wait = manager
        .invoke_typed_command(
            "native-command-provider",
            "defer-native",
            Value::Null,
            30000,
        )
        .unwrap();
    manager.poll();
    let request = manager
        .live
        .get_mut("native-command-provider")
        .unwrap()
        .take_editor_requests()
        .pop()
        .unwrap();
    assert!(request.begin());
    wait.cancel(api::CancelMode::TryTerminate, api::ErrorCode::Cancelled)
        .unwrap();
    assert!(
        request.status().is_terminal(),
        "pending native work must observe the original wait immediately"
    );
    request.finish(Ok(api::EditorValue::Interaction(
        plugin_runtime::plugin_protocol::interaction::Value::Input("late".into()),
    )));
    assert!(!matches!(
        request.status(),
        api::RequestUpdate::Completed { result: Ok(_) }
    ));
    manager.poll();
    assert!(wait.status().is_terminal());
}
