//! New UI capabilities enter through inspected independent SDK packages, never plugin-specific host hooks.
use plugin_runtime::{Manager, Package, plugin_protocol::Environment};
use serde_json::{Value, json};
use std::io::{Cursor, Write};
#[path = "composable_ui/icons.rs"]
mod ui_icons;

/// Owned content defaults and adjustable native panes are independently negotiated before publication.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn content_defaults_require_capability_and_adjustable_panes_use_current_native_version() {
    for negotiated in [false, true] {
        let package = package_with_ui(
            |manifest| {
                manifest["settings_hook"] = json!(false);
                manifest["settings"]["label"]["default"] = json!("composable-ui");
                manifest["api"]["required"]["ui.native"] = json!(">=1.1,<2");
                if negotiated {
                    manifest["api"]["required"]["ui.content_colors"] = json!("^1");
                }
            },
            |tree| {
                tree["content_colors"] = json!({"text.foreground":1122867});
                tree["root"] = json!({"id":"panes","layout":{"grow":true,"resizable":true},"kind":{"type":"row","children":[{"id":"a","kind":{"type":"text","text":"A"}},{"id":"b","kind":{"type":"text","text":"B"}}]}});
            },
        );
        let directory = tempfile::tempdir().unwrap();
        let mut manager =
            Manager::open(directory.path().join("plugins"), Environment::default()).unwrap();
        let result = manager.install(&package, package.manifest.permissions.clone());
        if negotiated {
            result.unwrap();
            let document = &manager.live["capability-example"].views["welcome"];
            assert_eq!(document.content_colors["text.foreground"], 0x112233);
            assert!(document.root.layout.resizable);
        } else {
            let error = result.unwrap_err();
            assert_eq!(
                error
                    .downcast_ref::<plugin_runtime::plugin_protocol::api::Failure>()
                    .unwrap()
                    .code,
                plugin_runtime::plugin_protocol::api::ErrorCode::CapabilityUnavailable
            );
            assert!(manager.live.is_empty());
        }
    }
}

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

/// A separate package identity exercises only the native rich-text contract and optional preview read.
fn richtext_package(node: Value, negotiated: bool, preview: bool) -> Package {
    package_with_ui(
        |manifest| {
            manifest["id"] = json!("richtext-fixture");
            manifest["name"] = json!("Rich text fixture");
            manifest["api"]["required"] =
                json!({"package.assets":"^1","ui.native":"^1","configuration":"^1"});
            manifest["api"]["optional"] = json!({});
            manifest["permissions"] = json!(["assets.read"]);
            if negotiated {
                manifest["api"]["required"]["ui.richtext"] = json!("^1");
            }
            if preview {
                manifest["api"]["required"]["editor.documents"] = json!("^1");
                manifest["permissions"] = json!(["assets.read", "editor.read"]);
                manifest["panels"][0]["position"] = json!("editor");
                manifest["panels"][0]["file_extensions"] = json!(["sample"]);
            }
            manifest["settings_hook"] = json!(false);
            manifest["settings"]["label"]["default"] = json!("composable-ui");
        },
        |tree| tree["root"] = node,
    )
}

/// Both new node kinds require their own capability without changing the existing UI document version.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn richtext_nodes_negotiate_independently_of_markdown() {
    use plugin_runtime::plugin_protocol::api::{ErrorCode, Failure};
    for kind in [
        json!({"type":"rich_text","html":"<p><strong>你好</strong></p>"}),
        json!({"type":"code_block","text":"let value = 1;\n","language":"rust"}),
    ] {
        for negotiated in [false, true] {
            let package =
                richtext_package(json!({"id":"block","kind":kind.clone()}), negotiated, false);
            let temp = tempfile::tempdir().unwrap();
            let mut manager =
                Manager::open(temp.path().join("plugins"), Environment::default()).unwrap();
            let result = manager.install(&package, package.manifest.permissions.clone());
            if negotiated {
                result.unwrap();
                let document = manager.live["richtext-fixture"].views["welcome"].as_ref();
                assert_eq!(document.version, 1);
                assert_eq!(serde_json::to_value(&document.root.kind).unwrap(), kind);
                assert!(document.root.source_range.is_none());
            } else {
                let error = result.unwrap_err();
                assert_eq!(
                    error.downcast_ref::<Failure>().map(|error| error.code),
                    Some(ErrorCode::CapabilityUnavailable),
                    "unexpected rejection: {error:#}"
                );
                assert!(manager.installed.is_empty() && manager.live.is_empty());
            }
        }
    }
}

/// Source mappings need the rich-text capability even when attached to an otherwise ordinary text node.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn richtext_source_ranges_require_capability_and_preserve_utf8_byte_offsets() {
    use plugin_runtime::plugin_protocol::{
        api::{DocumentVersion, ErrorCode, Failure, Notification},
        ui::SourceRange,
    };
    for negotiated in [false, true] {
        let package = richtext_package(
            json!({"id":"block","source_range":{"start":0,"end":6},
                "kind":{"type":"text","text":"你好"}}),
            negotiated,
            true,
        );
        let temp = tempfile::tempdir().unwrap();
        let mut manager =
            Manager::open(temp.path().join("plugins"), Environment::default()).unwrap();
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
        // Until a versioned preview arrives, the independent guest emits only static content.
        let initial = manager.live["richtext-fixture"].views["welcome"].as_ref();
        assert!(initial.source.is_none() && initial.root.source_range.is_none());
        let source = DocumentVersion {
            id: "memory-only".into(),
            path: "source.sample".into(),
            revision: 7,
        };
        let result = manager.event(
            "richtext-fixture",
            Some("welcome".into()),
            Notification::Preview {
                document: Some(source.clone()),
                text: "你好".into(),
            },
        );
        if negotiated {
            result.unwrap();
            let document = manager.live["richtext-fixture"].views["welcome"].as_ref();
            assert_eq!(document.source, Some(source));
            assert_eq!(
                document.root.source_range,
                Some(SourceRange { start: 0, end: 6 })
            );
        } else {
            let error = result.unwrap_err();
            assert_eq!(
                error.downcast_ref::<Failure>().map(|error| error.code),
                Some(ErrorCode::CapabilityUnavailable),
                "unexpected rejection: {error:#}"
            );
            assert!(manager.live["richtext-fixture"].views.is_empty());
        }
    }
}

/// Malformed guest mappings are rejected at the public manager boundary before publishing a view.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn richtext_source_ranges_reject_inverted_and_over_quota_bounds() {
    use plugin_runtime::plugin_protocol::api::{DocumentVersion, ErrorCode, Failure, Notification};
    for (start, end) in [(6, 0), (0, 1024 * 1024 + 1)] {
        let package = richtext_package(
            json!({"id":"block","source_range":{"start":start,"end":end},
                "kind":{"type":"text","text":"你好"}}),
            true,
            true,
        );
        let temp = tempfile::tempdir().unwrap();
        let mut manager =
            Manager::open(temp.path().join("plugins"), Environment::default()).unwrap();
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
        let error = manager
            .event(
                "richtext-fixture",
                Some("welcome".into()),
                Notification::Preview {
                    document: Some(DocumentVersion {
                        id: "memory-only".into(),
                        path: "source.sample".into(),
                        revision: 7,
                    }),
                    text: "你好".into(),
                },
            )
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<Failure>().map(|error| error.code),
            Some(ErrorCode::InvalidRequest),
            "unexpected rejection for {start}..{end}: {error:#}"
        );
        assert!(manager.live["richtext-fixture"].views.is_empty());
    }
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
