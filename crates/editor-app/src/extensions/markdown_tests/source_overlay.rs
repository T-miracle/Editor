//! A separately built SDK guest keeps source-mode modal input visible through the public UI contract.
use super::*;
use harness::NativeMarkdown;
use protocol::ui::{Dialog, Document, MenuItem, Node, PopupMenu};

/// Reuse public example WASM and ordinary package assets; the host has no fixture-specific behavior.
fn overlay_peer(dialog: bool, svg: bool) -> Package {
    let mut files = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
    )
    .unwrap()
    .files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["settings_hook"] = serde_json::json!(false);
    manifest["settings"]["label"]["default"] = serde_json::json!("composable-ui");
    manifest["api"]["required"]["editor.toolbar"] = serde_json::json!("^1");
    manifest["api"]["required"]["editor.layout"] = serde_json::json!("^1");
    manifest["api"]["required"]["ui.collections"] = serde_json::json!("^1");
    manifest["panels"][0]["position"] = serde_json::json!("editor");
    manifest["panels"][0]["file_extensions"] = serde_json::json!(["md"]);
    manifest["panels"][0]["default_visible"] = serde_json::json!(true);
    let mut document = Document::new(
        Node::new(
            "fixture-editor",
            protocol::ui::Kind::NativeEditor {
                document: protocol::api::DocumentVersion {
                    id: "template".into(),
                    path: "notes.md".into(),
                    revision: 0,
                },
            },
        )
        .grow(),
    );
    document.editor_layout = true;
    document.editor_toolbar = Some(Node::button("fixture-toolbar", "Toolbar"));
    if svg {
        let rect = protocol::Rect {
            x: 0.,
            y: 0.,
            w: 32.,
            h: 32.,
        };
        document.editor_toolbar = Some(Node::row("fixture-toolbar-row", vec![
            Node::button("fixture-toolbar", "Toolbar"),
            Node::new("fixture-toolbar-canvas", protocol::ui::Kind::Canvas(protocol::ui::Canvas {
                paint: vec![protocol::Paint::Svg {
                    rect, clip: rect,
                    source: "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"32\" height=\"32\"><rect width=\"32\" height=\"32\" fill=\"red\"/></svg>".into(),
                }], ..Default::default()
            })).width(32.).height(32.),
        ]));
    }
    if dialog {
        document.dialog = Some(Dialog::new(
            "fixture-dialog",
            "Dialog",
            Node::column(
                "fixture-dialog-content",
                vec![
                    Node::text("fixture-dialog-text", "Source modal"),
                    Node::input("caption", Default::default()),
                ],
            ),
        ));
    } else {
        document.menu = Some(PopupMenu {
            id: "fixture-menu".into(),
            x: 20.,
            y: 40.,
            items: vec![MenuItem {
                id: "fixture-action".into(),
                label: "Source menu".into(),
                disabled: false,
                separator_before: false,
            }],
        });
    }
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    files.insert(
        "composed-ui.json".into(),
        serde_json::to_vec(&document).unwrap(),
    );
    language_tests::packages::repack(files).unwrap()
}

/// Restored Source mode must retain a visible modal/popup, exclusive input and its dismissal lifecycle.
#[gpui::test]
#[ignore = "build markdown and current capability-example through the public SDK first"]
fn source_only_toolbar_keeps_sdk_dialog_and_popup_visible(cx: &mut TestAppContext) {
    for dialog in [true, false] {
        let original = "Source text";
        let peer = overlay_peer(dialog, false);
        let (mut fixture, ui) = NativeMarkdown::mount_package(cx, &[("notes.md", original)], &peer);
        fixture.open("notes.md", ui);
        let published = &fixture.manager.live["capability-example"].views["welcome"];
        assert!(
            published.source.is_some() && published.editor_toolbar.is_some(),
            "the real guest must publish a version-bound toolbar: {published:?}"
        );
        assert!(ui.debug_bounds("plugin-ui-fixture-body").is_none());
        assert!(
            ui.debug_bounds("editor-source-toolbar").is_some(),
            "active document {:?}",
            ui.update(|_, cx| fixture.app.read(cx).active_path.clone())
        );
        assert!(
            (ui.debug_bounds("plugin-ui-fixture-dialog-content")
                .is_some()
                || ui.debug_bounds("plugin-popup-menu").is_some()),
            "the source-only contribution must keep its dialog/menu visible"
        );
        if dialog {
            let field = ui.debug_bounds("plugin-ui-caption").unwrap();
            assert!(ui.debug_bounds("plugin-ui-fixture-dialog-text").is_some());
            ui.simulate_click(field.center(), Default::default());
            ui.simulate_input("中文输入");
            ui.run_until_parked();
            fixture.settle(ui);
        }
        assert_eq!(
            ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
            original
        );
        ui.simulate_keystrokes("escape");
        ui.run_until_parked();
        fixture.settle(ui);
        assert!(
            ui.debug_bounds("plugin-ui-fixture-dialog-content")
                .is_none()
        );
        assert!(ui.debug_bounds("editor-source-toolbar").is_some());
        let tree = &fixture.manager.live["capability-example"].views["welcome"];
        assert!(
            tree.dialog.is_none() && tree.menu.is_none(),
            "dismissal must reach the real guest"
        );
        assert!(tree.active_node("fixture-toolbar").is_some());
        fixture.focus_editor(ui);
        ui.simulate_input("可继续编辑");
        ui.run_until_parked();
        fixture.settle(ui);
        assert!(
            ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string())
                .contains("可继续编辑")
        );
    }
}

/// Toolbar canvases use the same authorized raster pipeline and package cleanup as ordinary UI nodes.
#[gpui::test]
#[ignore = "build markdown and current capability-example through the public SDK first"]
fn source_toolbar_svg_uses_the_public_canvas_raster_path(cx: &mut TestAppContext) {
    let peer = overlay_peer(true, true);
    let (mut fixture, ui) =
        NativeMarkdown::mount_package(cx, &[("notes.md", "Source text")], &peer);
    fixture.open("notes.md", ui);
    assert!(
        ui.debug_bounds("plugin-ui-fixture-toolbar-canvas")
            .is_some()
    );
    ui.update(|_, cx| {
        let owner = fixture.app.read(cx).extensions.read(cx);
        let state = owner.worker.state.lock().unwrap();
        assert!(
            state
                .images
                .get("capability-example/welcome/canvas/fixture-toolbar-canvas")
                .is_some_and(|images| images[0].is_some()),
            "source SVG must be rasterized"
        );
    });
    fixture.manager.disable("capability-example").unwrap();
    fixture.settle(ui);
    ui.update(|_, cx| {
        let owner = fixture.app.read(cx).extensions.read(cx);
        let state = owner.worker.state.lock().unwrap();
        assert!(
            !state
                .images
                .contains_key("capability-example/welcome/canvas/fixture-toolbar-canvas")
        );
    });
}
