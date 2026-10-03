//! New UI capabilities enter through inspected independent SDK packages, never plugin-specific host hooks.
use plugin_runtime::{Manager, Package, plugin_protocol::Environment};
use serde_json::{Value, json};
use std::io::{Cursor, Write};

/// Repackage a real SDK guest to verify capability negotiation and typed preview authority.
fn package(edit: impl FnOnce(&mut Value)) -> Package {
    package_with_ui(edit, |_| {})
}

/// Package mutations remain at the installed ZIP boundary rather than bypassing negotiation.
fn package_with_ui(edit: impl FnOnce(&mut Value), edit_ui: impl FnOnce(&mut Value)) -> Package {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/plugin-api-test/capability-example.zip");
    let mut files = Package::read(&root).unwrap().files;
    let mut manifest: Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    // Each test explicitly chooses its drawing capability contract.
    manifest["api"]["optional"]
        .as_object_mut()
        .unwrap()
        .remove("ui.canvas");
    edit(&mut manifest);
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let mut tree = json!({"version":1,"revision":1,"root":{
        "id":"root","kind":{"type":"column","children":[
            {"id":"zoom","kind":{"type":"button","label":"Zoom"}},
            {"id":"caption","kind":{"type":"input","value":"Native input","placeholder":"Type here"}},
            {"id":"viewport","layout":{"grow":true},"kind":{"type":"canvas","paint":[
                {"Fill":{"rect":{"x":0,"y":0,"w":100,"h":100},"color":3368601}}
            ]}}
        ]}
    }});
    edit_ui(&mut tree);
    files.insert(
        "composed-ui.json".into(),
        serde_json::to_vec(&tree).unwrap(),
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

/// Canvas drawing and character measurements are independently negotiated capabilities.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn canvas_and_grid_cannot_be_used_without_their_negotiated_capabilities() {
    for (canvas, grid, succeed) in [
        (false, false, false),
        (true, false, false),
        (true, true, true),
    ] {
        let package = package_with_ui(
            |manifest| {
                manifest["api"]["optional"] = json!({});
                if canvas {
                    manifest["api"]["required"]["ui.canvas"] = json!("^1");
                }
                if grid {
                    manifest["api"]["required"]["ui.grid"] = json!("^1");
                }
                manifest["settings_hook"] = json!(false);
                manifest["settings"]["label"]["default"] = json!("composable-ui");
            },
            |tree| tree["root"]["kind"]["children"][2]["kind"]["grid"] = json!(true),
        );
        let temp = tempfile::tempdir().unwrap();
        let mut manager =
            Manager::open(temp.path().join("plugins"), Environment::default()).unwrap();
        let result = manager.install(&package, package.manifest.permissions.clone());
        assert_eq!(
            result.is_ok(),
            succeed,
            "canvas={canvas}, grid={grid}: {result:?}"
        );
        if !succeed {
            assert!(manager.live.is_empty());
        }
    }
}

/// UI-only packages compose native controls and drawing without acquiring document/process authority.
#[test]
#[ignore = "build capability-example through the host SDK first"]
fn composed_native_and_canvas_tree_requires_no_editor_or_process_grants() {
    let package = package(|manifest| {
        manifest["api"]["required"] =
            json!({"package.assets":"^1","ui.native":"^1","ui.canvas":"^1","configuration":"^1"});
        manifest["permissions"] = json!(["assets.read"]);
        manifest["settings_hook"] = json!(false);
        manifest["settings"]["label"]["default"] = json!("composable-ui");
    });
    let temp = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(temp.path().join("plugins"), Environment::default()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let document = manager.live["capability-example"].views["welcome"].as_ref();
    let mut ids = Vec::new();
    document.root.visit(&mut |node| ids.push(node.id.clone()));
    assert_eq!(ids, ["root", "zoom", "caption", "viewport"]);
}

/// Events queued before a tree replacement cannot be retargeted to its current nodes or kill the guest.
#[test]
#[ignore = "build capability-example through the host SDK first"]
fn stale_and_missing_native_nodes_return_typed_errors_without_retiring_the_instance() {
    use plugin_runtime::plugin_protocol::{
        api::{ErrorCode, Failure, Notification as Event},
        ui::{Action, UiEvent},
    };
    let package = package(|manifest| {
        manifest["api"]["required"]["ui.canvas"] = json!("^1");
        manifest["settings_hook"] = json!(false);
        manifest["settings"]["label"]["default"] = json!("composable-ui");
    });
    let temp = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(temp.path().join("plugins"), Environment::default()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    for (revision, node, action, expected) in [
        (
            0,
            "caption",
            Action::Change("late".into()),
            ErrorCode::StaleRevision,
        ),
        (1, "removed", Action::Click, ErrorCode::InvalidHandle),
        (
            1,
            "viewport",
            Action::Change("wrong node kind".into()),
            ErrorCode::InvalidRequest,
        ),
    ] {
        let error = manager
            .event(
                "capability-example",
                Some("welcome".into()),
                Event::Ui(UiEvent {
                    revision,
                    node: node.into(),
                    action,
                }),
            )
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<Failure>().map(|error| error.code),
            Some(expected)
        );
        assert!(
            manager.live["capability-example"]
                .views
                .contains_key("welcome")
        );
    }
}

/// Preview text is separately authorized and its source version is echoed in the resulting native tree.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn preview_notifications_require_read_grants_and_preserve_document_identity() {
    use plugin_runtime::plugin_protocol::api::{self, DocumentVersion};
    let package = package(|manifest| {
        manifest["panels"][0]["position"] = json!("editor");
        manifest["panels"][0]["file_extensions"] = json!(["drawing"]);
    });
    let temp = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(temp.path().join("plugins"), Environment::default()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let version = DocumentVersion {
        id: "open-1".into(),
        path: "sample.drawing".into(),
        revision: 4,
    };
    let event = api::Notification::Preview {
        document: Some(version.clone()),
        text: "unsaved drawing".into(),
    };
    manager
        .installed
        .get_mut("capability-example")
        .unwrap()
        .grants
        .remove("editor.read");
    assert!(
        manager
            .event("capability-example", Some("welcome".into()), event.clone())
            .is_err()
    );
    manager
        .installed
        .get_mut("capability-example")
        .unwrap()
        .grants
        .insert("editor.read".into());
    manager
        .event("capability-example", Some("welcome".into()), event)
        .unwrap();
    let document = manager.live["capability-example"].views["welcome"].as_ref();
    assert_eq!(document.source, Some(version));
    assert!(
        serde_json::to_string(document)
            .unwrap()
            .contains("unsaved drawing")
    );
}

/// A versioned native preview needs explicit read authority, not the legacy broad editor command grant.
#[test]
#[ignore = "build capability-example through the host SDK first"]
fn native_canvas_preview_negotiates_without_legacy_editor_commands() {
    let package = package(|manifest| {
        manifest["api"]["required"]["ui.canvas"] = json!("^1");
        manifest["panels"][0]["position"] = json!("editor");
        manifest["panels"][0]["file_extensions"] = json!(["drawing"]);
    });
    assert!(!package.manifest.permissions.contains("editor.commands"));
    let temp = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        temp.path().join("plugins"),
        Environment {
            workspace: temp.path().display().to_string(),
            ..Environment::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    assert!(
        manager.live["capability-example"]
            .views
            .contains_key("welcome")
    );
}

/// A real guest changes layout and vector geometry through the same nodes used by native controls.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn toolbar_updates_unsaved_vector_preview_and_all_three_layouts() {
    use plugin_runtime::plugin_protocol::{
        Paint,
        api::Notification as Event,
        api::{self, DocumentVersion},
        ui::{Action, Kind, UiEvent},
    };
    let package = package(|manifest| {
        manifest["api"]["required"]["ui.canvas"] = json!("^1");
        manifest["panels"][0]["position"] = json!("editor");
        manifest["panels"][0]["file_extensions"] = json!(["drawing"]);
        manifest["settings_hook"] = json!(false);
        manifest["settings"]["label"]["default"] = json!("composable-ui");
    });
    let temp = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(temp.path().join("plugins"), Environment::default()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let source = DocumentVersion {
        id: "memory-only".into(),
        path: "unsaved.drawing".into(),
        revision: 42,
    };
    manager.event("capability-example", Some("welcome".into()), api::Notification::Preview {
        document:Some(source.clone()), text:r#"<svg xmlns="http://www.w3.org/2000/svg" width="160" height="100"><rect width="160" height="100" fill="red"/></svg>"#.into()
    }).unwrap();
    let before = manager.live["capability-example"].views["welcome"].clone();
    assert_eq!(before.source, Some(source));
    manager
        .event(
            "capability-example",
            Some("welcome".into()),
            Event::Ui(UiEvent {
                revision: before.revision,
                node: "zoom".into(),
                action: Action::Click,
            }),
        )
        .unwrap();
    let after = manager.live["capability-example"].views["welcome"].as_ref();
    let Kind::Canvas(canvas) = &after.active_node("viewport").unwrap().kind else {
        panic!("Missing canvas")
    };
    assert!(matches!(canvas.paint[0], Paint::Svg {rect,..} if rect.w == 200. && rect.h == 125.));
    assert!(after.revision > before.revision);
    for mode in ["canvas", "form", "combined"] {
        manager
            .invoke_command("capability-example", "ui-layout", json!(mode))
            .unwrap();
        let tree = manager.live["capability-example"].views["welcome"].as_ref();
        assert_eq!(tree.active_node("caption").is_some(), mode != "canvas");
        assert_eq!(tree.active_node("viewport").is_some(), mode != "form");
    }
    // Theme notifications alter guest drawing content without restarting the instance.
    let revision = manager.live["capability-example"].views["welcome"]
        .as_ref()
        .revision;
    manager
        .event(
            "capability-example",
            Some("welcome".into()),
            Event::Ui(UiEvent {
                revision,
                node: "caption".into(),
                action: Action::Change("Caption".into()),
            }),
        )
        .unwrap();
    manager
        .event(
            "capability-example",
            None,
            Event::Theme(Environment {
                foreground: 0xabcdef,
                ui_font: plugin_runtime::plugin_protocol::FontStyle {
                    size_px: Some(22.),
                    ..Default::default()
                },
                ..Default::default()
            }),
        )
        .unwrap();
    let tree = manager.live["capability-example"].views["welcome"].as_ref();
    let Kind::Canvas(canvas) = &tree.active_node("viewport").unwrap().kind else {
        unreachable!()
    };
    assert!(
        canvas
            .paint
            .iter()
            .any(|paint| matches!(paint,Paint::Text {color:0xabcdef,size,..} if *size==22.))
    );
    manager.uninstall("capability-example", true).unwrap();
    assert!(manager.live.is_empty() && manager.published_entries().is_empty());
}
