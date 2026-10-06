//! Selection edits and source toolbars cross an independently named actual SDK package boundary.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, api, ui::Kind},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
};

/// Minimal archive mutations preserve real WASM dispatch instead of adding a test-only host API.
fn package(
    edit: impl FnOnce(&mut Value, &mut BTreeMap<String, Vec<u8>>),
) -> anyhow::Result<Package> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/plugin-api-test/capability-example.zip");
    let mut files = Package::read(&path)?.files;
    let mut manifest: Value = serde_json::from_slice(&files["manifest.json"])?;
    manifest["id"] = json!("edit-fixture");
    manifest["name"] = json!("Versioned document edit fixture");
    manifest["api"]["required"] =
        json!({"package.assets":"^1", "ui.native":"^1", "editor.edit":"^1"});
    manifest["api"]["optional"] = json!({});
    manifest["permissions"] = json!(["assets.read", "editor.read", "editor.write"]);
    manifest["settings"] = json!({});
    manifest["settings_hook"] = json!(false);
    manifest["commands"] = json!([{"id":"scope-probe", "title":"Probe typed edit request"}]);
    edit(&mut manifest, &mut files);
    files.insert("manifest.json".into(), serde_json::to_vec(&manifest)?);
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        archive.start_file(name, zip::write::SimpleFileOptions::default())?;
        archive.write_all(&bytes)?;
    }
    Package::from_bytes(&archive.finish()?.into_inner())
}

/// Request acceptance and typed refusal are observable in the guest's ordinary native output.
fn text(manager: &Manager) -> String {
    let Kind::Text { text } = &manager.live["edit-fixture"].views["welcome"].root.kind else {
        panic!("Expected the independent native probe view")
    };
    text.clone()
}

/// A source-bound toolbar is contributed by the package's ordinary configured UI asset.
fn toolbar_package(
    edit: impl FnOnce(&mut Value, &mut BTreeMap<String, Vec<u8>>),
) -> anyhow::Result<Package> {
    package(|manifest, files| {
        manifest["api"]["required"] = json!({"package.assets":"^1", "ui.native":"^1",
            "editor.documents":"^1", "editor.toolbar":"^1", "configuration":"^1"});
        manifest["permissions"] = json!(["assets.read", "editor.read"]);
        manifest["panels"][0]["position"] = json!("editor");
        manifest["panels"][0]["file_extensions"] = json!(["sample"]);
        manifest["settings"] = json!({"label":{"title":"Fixture UI", "value_type":{"kind":"string","max_length":120},
            "default":"composable-ui", "scope":"user", "apply":"restart_instance"}});
        files.insert("composed-ui.json".into(), serde_json::to_vec(&json!({
            "version":1,"revision":1,"root":{"id":"body","kind":{"type":"text","text":"Preview"}},
            "editor_toolbar":{"id":"toolbar","layout":{"wrap":true},"kind":{"type":"row","children":[
                {"id":"zoom","tooltip":"缩放 / Zoom","kind":{"type":"button","label":"Zoom"}}
            ]}}
        })).unwrap());
        edit(manifest, files);
    })
}

/// The manager supplies immutable unsaved text and a document identity to the declared preview.
fn preview(manager: &mut Manager) -> anyhow::Result<()> {
    manager.event(
        "edit-fixture",
        Some("welcome".into()),
        api::Notification::Preview {
            document: Some(api::DocumentVersion {
                id: "memory-only".into(),
                path: "source.sample".into(),
                revision: 7,
            }),
            text: "你好".into(),
        },
    )
}

/// The independent package can negotiate editor.edit without any Markdown-specific host dependency.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn edit_capability_negotiates_on_independent_real_guest() {
    let package = package(|_, _| {}).unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(root.path().into(), Environment::default()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    assert!(text(&manager).contains("Hello from a versioned package asset."));
}

/// Explicit target versions and UTF-8 byte selections survive asynchronous SDK request/completion routing.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn edit_requests_use_their_own_capability_and_preserve_target_and_byte_ranges() {
    let package = package(|_, _| {}).unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(root.path().into(), Environment::default()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let document = api::DocumentVersion {
        id: "open-document".into(),
        path: "source.sample".into(),
        revision: 7,
    };
    for operation in [
        json!({"kind":"read_document_selection", "document":document}),
        json!({"kind":"replace_document_range", "document":document,
            "range":{"start":0,"end":6}, "text":"**你好**", "selection":{"start":2,"end":8},
            "expected_selection":{"start":0,"end":6}}),
    ] {
        manager
            .invoke_command(
                "edit-fixture",
                "scope-probe",
                json!({"method":"editor", "operation":operation, "timeout_ms":30000}),
            )
            .unwrap();
        let accepted: Result<api::Value, api::Failure> =
            serde_json::from_str(&text(&manager)).unwrap();
        assert!(matches!(accepted.unwrap(), api::Value::Accepted(_)));
        let request = manager
            .live
            .get_mut("edit-fixture")
            .unwrap()
            .take_editor_requests()
            .pop()
            .unwrap();
        assert_eq!(
            serde_json::to_value(request.operation()).unwrap(),
            operation
        );
        assert!(request.begin());
        let result = if operation["kind"] == "read_document_selection" {
            api::EditorValue::DocumentSelection {
                document: document.clone(),
                range: api::TextRange { start: 0, end: 6 },
                text: "你好".into(),
            }
        } else {
            assert!(request.enter_side_effect());
            api::EditorValue::Edited {
                document: api::DocumentVersion {
                    revision: 8,
                    ..document.clone()
                },
                selection: api::TextRange { start: 2, end: 8 },
            }
        };
        request.finish(Ok(result));
        manager.poll();
        assert!(
            text(&manager).contains(if operation["kind"] == "read_document_selection" {
                "DocumentSelection"
            } else {
                "Edited"
            })
        );
    }
}

/// A malformed edit is refused before a host request handle can be allocated.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn edit_requests_reject_inverted_ranges_and_oversized_replacements() {
    let package = package(|_, _| {}).unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(root.path().into(), Environment::default()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    for (field, value, expected) in [
        (
            "range",
            json!({"start":6,"end":0}),
            api::ErrorCode::InvalidRequest,
        ),
        (
            "selection",
            json!({"start":8,"end":2}),
            api::ErrorCode::InvalidRequest,
        ),
        (
            "expected_selection",
            json!({"start":6,"end":0}),
            api::ErrorCode::InvalidRequest,
        ),
        ("text", json!("x"), api::ErrorCode::LimitExceeded),
    ] {
        let mut operation = json!({"kind":"replace_document_range", "document":{"id":"open","path":"source.sample","revision":7},
            "range":{"start":0,"end":6}, "text":"**你好**", "selection":{"start":2,"end":8}, "expected_selection":null});
        operation[field] = value;
        // The guest constructs bounded repeated text; command arguments retain their ordinary quota.
        let mut arguments = json!({"method":"editor","operation":operation,"timeout_ms":30000});
        if field == "text" {
            arguments["repeat_text"] = json!(1024 * 1024 + 1);
        }
        manager
            .invoke_command("edit-fixture", "scope-probe", arguments)
            .unwrap();
        let result: Result<api::Value, api::Failure> =
            serde_json::from_str(&text(&manager)).unwrap();
        assert_eq!(
            result.unwrap_err().code,
            expected,
            "malformed field: {field}"
        );
        assert!(
            manager
                .live
                .get_mut("edit-fixture")
                .unwrap()
                .take_editor_requests()
                .is_empty()
        );
    }
}

/// An independently named WASM guest negotiates and routes a toolbar through its owning editor panel.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn toolbar_capability_negotiates_and_routes_source_bound_native_events() {
    use plugin_runtime::plugin_protocol::ui::{Action, UiEvent};
    let package = toolbar_package(|_, _| {}).unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(root.path().into(), Environment::default()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    assert!(
        manager.live["edit-fixture"].views["welcome"]
            .editor_toolbar
            .is_none()
    );
    preview(&mut manager).unwrap();
    let document = manager.live["edit-fixture"].views["welcome"].clone();
    assert_eq!(document.source.as_ref().unwrap().revision, 7);
    let toolbar = document.editor_toolbar.as_ref().unwrap();
    assert!(toolbar.layout.wrap);
    assert_eq!(
        document.active_node("zoom").unwrap().tooltip.as_deref(),
        Some("缩放 / Zoom")
    );
    manager
        .event(
            "edit-fixture",
            Some("welcome".into()),
            api::Notification::Ui(UiEvent {
                revision: document.revision,
                node: "zoom".into(),
                action: Action::Click,
            }),
        )
        .unwrap();
    assert_eq!(
        manager.live["edit-fixture"].views["welcome"].revision,
        document.revision + 1
    );
    assert_eq!(
        manager.live["edit-fixture"].views["welcome"].source,
        document.source
    );
}

/// A source version alone never grants the capability to publish controls above the editor.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn toolbar_publication_requires_its_own_negotiated_capability() {
    let package = toolbar_package(|manifest, _| {
        manifest["api"]["required"]
            .as_object_mut()
            .unwrap()
            .remove("editor.toolbar");
    })
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(root.path().into(), Environment::default()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let error = preview(&mut manager).unwrap_err();
    assert_eq!(
        error
            .downcast_ref::<api::Failure>()
            .map(|failure| failure.code),
        Some(api::ErrorCode::CapabilityUnavailable),
        "{error:#}"
    );
    assert!(manager.live["edit-fixture"].views.is_empty());
}

/// Reading and replacing selections require distinct grants and never borrow application editor authority.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn edit_requests_require_capability_workspace_and_the_matching_read_write_grant() {
    for case in ["capability", "read", "write", "application"] {
        let package = package(|manifest, _| match case {
            "capability" => {
                manifest["api"]["required"]
                    .as_object_mut()
                    .unwrap()
                    .remove("editor.edit");
            }
            "read" => manifest["permissions"] = json!(["assets.read", "editor.write"]),
            "write" => manifest["permissions"] = json!(["assets.read", "editor.read"]),
            _ => manifest["scope"] = json!("application"),
        })
        .unwrap();
        let root = tempfile::tempdir().unwrap();
        let mut manager = Manager::open(root.path().into(), Environment::default()).unwrap();
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
        for kind in ["read_document_selection", "replace_document_range"] {
            let mut operation =
                json!({"kind":kind, "document":{"id":"open","path":"source.sample","revision":7}});
            if kind == "replace_document_range" {
                operation["range"] = json!({"start":0,"end":0});
                operation["text"] = json!("你好");
                operation["selection"] = json!({"start":0,"end":6});
            }
            manager
                .invoke_command(
                    "edit-fixture",
                    "scope-probe",
                    json!({"method":"editor","operation":operation,"timeout_ms":30000}),
                )
                .unwrap();
            let result: Result<api::Value, api::Failure> =
                serde_json::from_str(&text(&manager)).unwrap();
            let denied = case == "capability"
                || case == "application"
                || (case == "read" && kind == "read_document_selection")
                || (case == "write" && kind == "replace_document_range");
            let requests = manager
                .live
                .get_mut("edit-fixture")
                .unwrap()
                .take_editor_requests();
            if denied {
                assert_eq!(
                    result.unwrap_err().code,
                    if case == "capability" {
                        api::ErrorCode::CapabilityUnavailable
                    } else {
                        api::ErrorCode::PermissionDenied
                    },
                    "{case}/{kind}"
                );
                assert!(requests.is_empty());
            } else {
                assert!(
                    matches!(result.unwrap(), api::Value::Accepted(_)),
                    "{case}/{kind}"
                );
                assert_eq!(requests.len(), 1);
            }
        }
        // Retirement disposes any accepted but intentionally unexecuted probe calls.
        manager.disable("edit-fixture").unwrap();
        assert_eq!(manager.resource_count(), 0);
    }
}

/// Revoking a queued range edit seals the shared side-effect gate and discards late completion data.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn edit_cancellation_and_retirement_block_pending_range_writes() {
    let package = package(|_, _| {}).unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(root.path().into(), Environment::default()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    for cancel in [true, false] {
        manager.invoke_command("edit-fixture", "scope-probe", json!({"method":"editor","timeout_ms":30000,
            "operation":{"kind":"replace_document_range", "document":{"id":"open","path":"source.sample","revision":7},
                "range":{"start":0,"end":0}, "text":"你好", "selection":{"start":0,"end":6}}})).unwrap();
        let request = manager
            .live
            .get_mut("edit-fixture")
            .unwrap()
            .take_editor_requests()
            .pop()
            .unwrap();
        if cancel {
            manager.invoke_command("edit-fixture", "scope-probe", json!({"method":"cancel_request", "handle":request.handle(), "mode":"try_terminate"})).unwrap();
            assert!(text(&manager).contains("not_executed"));
        } else {
            manager.disable("edit-fixture").unwrap();
        }
        assert!(!request.begin());
        assert!(!request.enter_side_effect());
        request.finish(Ok(api::EditorValue::Edited {
            document: api::DocumentVersion {
                id: "LATE".into(),
                path: "source.sample".into(),
                revision: 8,
            },
            selection: api::TextRange { start: 0, end: 6 },
        }));
        assert!(!format!("{:?}", request.status()).contains("LATE"));
        if cancel {
            manager.poll();
            assert!(text(&manager).contains("Cancelled"));
        }
    }
    assert_eq!(manager.resource_count(), 0);
}

/// Additional drawing and text capabilities apply equally to toolbar descendants and preview content.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn toolbar_nodes_share_negotiated_capability_checks() {
    for kind in [
        json!({"type":"rich_text","html":"<p>你好</p>"}),
        json!({"type":"canvas","paint":[]}),
    ] {
        let package = toolbar_package(|_, files| {
            let mut tree: Value = serde_json::from_slice(&files["composed-ui.json"]).unwrap();
            tree["editor_toolbar"]["kind"]["children"]
                .as_array_mut()
                .unwrap()
                .push(json!({"id":"extra","kind":kind}));
            files.insert(
                "composed-ui.json".into(),
                serde_json::to_vec(&tree).unwrap(),
            );
        })
        .unwrap();
        let root = tempfile::tempdir().unwrap();
        let mut manager = Manager::open(root.path().into(), Environment::default()).unwrap();
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
        let error = preview(&mut manager).unwrap_err();
        assert_eq!(
            error
                .downcast_ref::<api::Failure>()
                .map(|failure| failure.code),
            Some(api::ErrorCode::CapabilityUnavailable),
            "{error:#}"
        );
        assert!(manager.live["edit-fixture"].views.is_empty());
    }
}

/// Negotiating toolbar controls never creates preview read authority or cross-panel ownership.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn toolbar_preview_admission_and_ingress_preserve_scope_permissions_and_panel_ownership() {
    for case in ["read", "application"] {
        let error = toolbar_package(|manifest, _| {
            if case == "read" {
                manifest["permissions"] = json!(["assets.read"]);
            } else {
                manifest["scope"] = json!("application");
            }
        })
        .err()
        .expect("Preview declaration must be rejected");
        assert!(
            format!("{error:#}").contains("Editor previews require document read authority"),
            "{case}: {error:#}"
        );
    }
    let package = toolbar_package(|manifest, _| {
        manifest["panels"][0]["position"] = json!("right");
        manifest["panels"][0]["file_extensions"] = json!([]);
    })
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(root.path().into(), Environment::default()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    for panel in ["welcome", "foreign"] {
        let error = manager
            .event(
                "edit-fixture",
                Some(panel.into()),
                api::Notification::Preview {
                    document: Some(api::DocumentVersion {
                        id: "open".into(),
                        path: "source.sample".into(),
                        revision: 7,
                    }),
                    text: "你好".into(),
                },
            )
            .unwrap_err();
        assert_eq!(
            error
                .downcast_ref::<api::Failure>()
                .map(|failure| failure.code),
            Some(api::ErrorCode::PermissionDenied),
            "{panel}: {error:#}"
        );
    }
    assert!(
        manager.live["edit-fixture"].views["welcome"]
            .editor_toolbar
            .is_none()
    );
}

/// Closing overlays preserves the source-bound toolbar and dismisses only the current modal target.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn toolbar_overlay_dismissal_preserves_source_and_modal_priority() {
    use plugin_runtime::plugin_protocol::ui::{Action, UiEvent};
    let package = toolbar_package(|manifest, files| {
        manifest["api"]["required"]["ui.collections"] = json!("^1");
        let mut tree: Value = serde_json::from_slice(&files["composed-ui.json"]).unwrap();
        tree["dialog"] = json!({"id":"modal","title":"Settings","width":480,
            "content":{"id":"close","kind":{"type":"button","label":"Close"}}});
        tree["menu"] = json!({"id":"popup","x":0,"y":0,"items":[{"id":"choice","label":"Choice"}]});
        files.insert(
            "composed-ui.json".into(),
            serde_json::to_vec(&tree).unwrap(),
        );
    })
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(root.path().into(), Environment::default()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    preview(&mut manager).unwrap();
    for id in ["modal", "popup"] {
        let before = manager.live["edit-fixture"].views["welcome"].clone();
        assert!(before.active_node("zoom").is_none());
        manager
            .event(
                "edit-fixture",
                Some("welcome".into()),
                api::Notification::Ui(UiEvent {
                    revision: before.revision,
                    node: id.into(),
                    action: Action::Dismiss,
                }),
            )
            .unwrap();
        let after = &manager.live["edit-fixture"].views["welcome"];
        assert!(
            after.dialog.is_none(),
            "the dialog target was not dismissed"
        );
        assert_eq!(after.menu.is_some(), id == "modal");
        assert_eq!(after.revision, before.revision + 1);
        assert_eq!(after.source, before.source);
        assert!(after.editor_toolbar.is_some());
    }
    assert!(
        manager.live["edit-fixture"].views["welcome"]
            .active_node("zoom")
            .is_some()
    );
}
