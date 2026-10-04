//! Independent SDK guests exercise semantic viewports at the public ZIP/Manager boundary.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, api, ui},
};
use serde_json::{Value, json};
use std::io::{Cursor, Write};

const ID: &str = "independent-viewport";

/// Mutate public declarations/assets only; keep the independently compiled example's real WASM.
fn package(edit: impl FnOnce(&mut Value, &mut Value)) -> Package {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/plugin-api-test/capability-example.zip");
    let mut files = Package::read(&path).unwrap().files;
    let mut manifest: Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["id"] = json!(ID);
    manifest["name"] = json!("Independent semantic viewport fixture");
    manifest["scope"] = json!("workspace");
    manifest["api"]["required"] = json!({"package.assets":"^1", "ui.native":"^1",
        "ui.richtext":"^1", "editor.documents":"^1", "editor.viewport":"^1", "configuration":"^1"});
    manifest["api"]["optional"] = json!({});
    manifest["permissions"] = json!(["assets.read", "editor.read"]);
    manifest["settings_hook"] = json!(false);
    manifest["settings"] = json!({"label":{"title":"Fixture UI", "value_type":{"kind":"string","max_length":120},
        "default":"composable-ui", "scope":"user", "apply":"restart_instance"}});
    manifest["panels"] = json!([{"id":"welcome", "title":"Native viewport", "position":"editor", "file_extensions":["sample"]}]);
    manifest["commands"] = json!([{"id":"scope-probe", "title":"Request viewport"}, {"id":"preview-probe", "title":"Publish scene"}]);
    let mut document = json!({"version":1, "revision":1, "editor_viewport":"viewport", "root":{
        "id":"viewport", "kind":{"type":"scroll", "content":{
            "id":"block", "source_range":{"start":0,"end":6}, "kind":{"type":"text","text":"你好"}
        }}
    }});
    edit(&mut manifest, &mut document);
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    files.insert(
        "composed-ui.json".into(),
        serde_json::to_vec(&document).unwrap(),
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

/// Own an isolated trusted workspace and grant exactly the package's public declarations.
fn install(package: &Package) -> (tempfile::TempDir, Manager) {
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(package, package.manifest.permissions.clone())
        .unwrap();
    (root, manager)
}

/// The source identity denotes in-memory text; it does not come from a fixture file or language name.
fn source() -> api::DocumentVersion {
    api::DocumentVersion {
        id: "memory-source".into(),
        path: "notes.sample".into(),
        revision: 7,
    }
}

fn bind(manager: &mut Manager) -> anyhow::Result<()> {
    manager.event(
        ID,
        Some("welcome".into()),
        api::Notification::Preview {
            document: Some(source()),
            text: "你好".into(),
        },
    )
}

fn refusal(result: anyhow::Result<()>, code: api::ErrorCode) {
    let error = result.unwrap_err();
    assert_eq!(
        error.downcast_ref::<api::Failure>().map(|error| error.code),
        Some(code),
        "{error:#}"
    );
}

/// This independently named panel has no editor.presentation or view_modes dependency.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn viewport_independent_binding_checks_capabilities_source_revision_and_event_ranges() {
    let package = package(|_, _| {});
    let (_root, mut manager) = install(&package);
    assert!(manager.live[ID].views["welcome"].editor_viewport.is_none());
    bind(&mut manager).unwrap();
    let scene = manager.live[ID].views["welcome"].clone();
    assert_eq!(scene.editor_viewport.as_deref(), Some("viewport"));
    let position = api::SourceViewport {
        document: source(),
        ui_revision: scene.revision,
        offset: 0,
        line_fraction: 0.5,
        origin: None,
        layout: false,
    };
    for stale in [
        api::SourceViewport {
            ui_revision: scene.revision + 1,
            ..position.clone()
        },
        api::SourceViewport {
            document: api::DocumentVersion {
                revision: 6,
                ..source()
            },
            ..position.clone()
        },
        api::SourceViewport {
            document: api::DocumentVersion {
                id: "other-source".into(),
                ..source()
            },
            ..position.clone()
        },
    ] {
        refusal(
            manager.event(
                ID,
                Some("welcome".into()),
                api::Notification::SourceViewport(stale),
            ),
            api::ErrorCode::StaleRevision,
        );
    }
    refusal(
        manager.event(
            ID,
            Some("welcome".into()),
            api::Notification::SourceViewport(api::SourceViewport {
                origin: Some(0),
                ..position.clone()
            }),
        ),
        api::ErrorCode::InvalidRequest,
    );
    manager
        .event(
            ID,
            Some("welcome".into()),
            api::Notification::SourceViewport(position),
        )
        .unwrap();
    let revision = manager.live[ID].views["welcome"].revision;
    let preview = api::PreviewViewport {
        block: "block".into(),
        source_range: ui::SourceRange { start: 0, end: 6 },
        fraction: 0.5,
        origin: None,
        layout: false,
    };
    refusal(
        manager.event(
            ID,
            Some("welcome".into()),
            api::Notification::Ui(ui::UiEvent {
                revision,
                node: "viewport".into(),
                action: ui::Action::Viewport(api::PreviewViewport {
                    source_range: ui::SourceRange { start: 0, end: 5 },
                    ..preview.clone()
                }),
            }),
        ),
        api::ErrorCode::InvalidRequest,
    );
    manager
        .event(
            ID,
            Some("welcome".into()),
            api::Notification::Ui(ui::UiEvent {
                revision,
                node: "viewport".into(),
                action: ui::Action::Viewport(preview),
            }),
        )
        .unwrap();
    manager
        .event(
            ID,
            Some("welcome".into()),
            api::Notification::Preview {
                document: None,
                text: String::new(),
            },
        )
        .unwrap();
    assert!(manager.live[ID].views["welcome"].editor_viewport.is_none());
}

/// Opting in cannot borrow richtext/viewport/read authority from ordinary UI or another panel.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn viewport_binding_refuses_missing_capability_grant_scope_and_preserves_inert_default() {
    for capability in ["editor.viewport", "ui.richtext"] {
        let package = package(|manifest, document| {
            manifest["api"]["required"]
                .as_object_mut()
                .unwrap()
                .remove(capability);
            document["root"]["kind"]["content"]
                .as_object_mut()
                .unwrap()
                .remove("source_range");
        });
        let (_root, mut manager) = install(&package);
        refusal(bind(&mut manager), api::ErrorCode::CapabilityUnavailable);
    }
    for grant in [false, true] {
        let package = package(|manifest, _| {
            if grant {
                manifest["scope"] = json!("application");
            } else {
                manifest["permissions"] = json!(["assets.read"]);
            }
            // Editor panels themselves require read/workspace declarations at package validation.
            // A legal ordinary panel proves Preview cannot supply that authority implicitly.
            manifest["panels"][0]["position"] = json!("right");
            manifest["panels"][0]
                .as_object_mut()
                .unwrap()
                .remove("file_extensions");
        });
        let (_root, mut manager) = install(&package);
        refusal(bind(&mut manager), api::ErrorCode::PermissionDenied);
    }
    let package = package(|manifest, document| {
        manifest["api"]["required"]
            .as_object_mut()
            .unwrap()
            .remove("editor.viewport");
        document.as_object_mut().unwrap().remove("editor_viewport");
    });
    let (_root, mut manager) = install(&package);
    bind(&mut manager).unwrap();
    let revision = manager.live[ID].views["welcome"].revision;
    refusal(
        manager.event(
            ID,
            Some("welcome".into()),
            api::Notification::SourceViewport(api::SourceViewport {
                document: source(),
                ui_revision: revision,
                offset: 0,
                line_fraction: 0.0,
                origin: None,
                layout: false,
            }),
        ),
        api::ErrorCode::StaleRevision,
    );
}

/// Request admission is typed and instance-owned; retirement seals already accepted queued work.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn viewport_locate_requests_validate_shape_owner_and_retire_without_late_effects() {
    let package = package(|manifest, _| {
        manifest["settings"] = json!({});
    });
    let (_root, mut manager) = install(&package);
    let operation = json!({"kind":"locate_viewport", "document":source(), "panel":"welcome",
        "ui_revision":9, "target":{"kind":"source", "offset":0, "line_fraction":0.5}, "origin":11});
    for (field, replacement, code) in [
        ("origin", json!(0), api::ErrorCode::InvalidRequest),
        (
            "panel",
            json!("other/welcome"),
            api::ErrorCode::PermissionDenied,
        ),
        (
            "target",
            json!({"kind":"preview","node":"block","fraction":2.0}),
            api::ErrorCode::InvalidRequest,
        ),
    ] {
        let mut invalid = operation.clone();
        invalid[field] = replacement;
        manager
            .invoke_command(
                ID,
                "scope-probe",
                json!({"method":"editor", "operation":invalid, "timeout_ms":30000}),
            )
            .unwrap();
        let ui::Kind::Text { text } = &manager.live[ID].views["welcome"].root.kind else {
            panic!("diagnostic text")
        };
        let result: Result<api::Value, api::Failure> = serde_json::from_str(text).unwrap();
        assert_eq!(result.unwrap_err().code, code);
        assert!(
            manager
                .live
                .get_mut(ID)
                .unwrap()
                .take_editor_requests()
                .is_empty()
        );
    }
    manager
        .invoke_command(
            ID,
            "scope-probe",
            json!({"method":"editor", "operation":operation, "timeout_ms":30000}),
        )
        .unwrap();
    let requests = manager.live.get_mut(ID).unwrap().take_editor_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        serde_json::to_value(requests[0].operation()).unwrap(),
        operation
    );
    manager.disable(ID).unwrap();
    assert!(!requests[0].begin() && !requests[0].enter_side_effect());
    requests[0].finish(Ok(api::EditorValue::Unit));
    manager.poll();
    assert!(manager.live.is_empty() && manager.resource_count() == 0);
}
