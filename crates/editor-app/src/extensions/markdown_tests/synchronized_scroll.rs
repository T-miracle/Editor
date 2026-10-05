//! Real wheel input crosses the public package/worker seam and reveals corresponding native blocks.
use super::*;
use harness::NativeMarkdown;

/// Compare source and preview content after actual source wheel input, not two internal offsets.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_source_scroll_reveals_the_corresponding_native_block(
    cx: &mut TestAppContext,
) {
    let source = (0..100)
        .map(|index| format!("段落 {index:03}：同步阅读对应内容。\n\n"))
        .collect::<String>();
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", &source)]);
    fixture.open("notes.md", ui);
    let position = ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).input_bounds().center());
    ui.simulate_event(gpui_kit::ScrollWheelEvent {
        position,
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-2400.))),
        ..Default::default()
    });
    ui.run_until_parked();
    fixture.settle(ui);
    let source_offset = ui.update(|_, cx| {
        let editor = fixture.app.read(cx).editor.read(cx);
        let mut visible = editor.visible_row_range().expect("laid-out source rows");
        assert!(
            visible.start > 20,
            "native source wheel moved away from the beginning"
        );
        visible
            .find_map(|row| {
                let offset = editor.text().line_start_offset(row);
                let next = editor.text().line_end_offset(row);
                (next > offset).then_some(offset)
            })
            .expect("visible nonempty paragraph")
    });
    let block = fixture.manager.live["markdown"].views["preview"]
        .active_node(&format!("b-{source_offset}-paragraph"))
        .expect("the visible source paragraph has a derived block");
    let selector = Box::leak(format!("plugin-ui-{}", block.id).into_boxed_str());
    let pane = ui.debug_bounds("plugin-ui-preview-root").unwrap();
    let paragraph = ui
        .debug_bounds(selector)
        .expect("corresponding native paragraph layout");
    assert!(
        paragraph.bottom() > pane.top() && paragraph.top() < pane.bottom(),
        "source wheel must reveal corresponding preview content: source={source_offset}, paragraph={paragraph:?}, pane={pane:?}"
    );
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        source
    );
    assert_eq!(
        std::fs::read_to_string(fixture.directory.path().join("notes.md")).unwrap(),
        source
    );
    let revision = fixture.manager.live["markdown"].views["preview"].revision;
    let before = ui.debug_bounds(selector).unwrap();
    fixture.settle(ui);
    assert_eq!(
        fixture.manager.live["markdown"].views["preview"].revision, revision,
        "viewport receipts must not republish the Markdown scene"
    );
    assert_eq!(
        ui.debug_bounds(selector).unwrap(),
        before,
        "settled painting has no feedback loop"
    );

    // The opposite native viewport now drives a source locate without changing its caret or selection.
    let selection = ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).selected_range());
    wheel(ui, "plugin-ui-preview-scroll", -1350.);
    fixture.settle(ui);
    assert_source_matches_preview(&fixture, ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).selected_range()),
        selection
    );
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        source
    );

    fixture.click("plugin-tool-markdown/preview/display-sync", ui);
    let stationary = ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).scroll_offset());
    wheel(ui, "plugin-ui-preview-scroll", 850.);
    fixture.settle(ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).scroll_offset()),
        stationary,
        "turning synchronization off allows independent preview reading"
    );
}

/// Real viewport wheel input leaves native Base in charge of clamping and event dispatch.
fn wheel(ui: &mut gpui_kit::VisualTestContext, selector: &'static str, delta: f32) {
    let position = ui
        .debug_bounds(selector)
        .expect("visible native viewport")
        .center();
    ui.simulate_event(gpui_kit::ScrollWheelEvent {
        position,
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(delta))),
        ..Default::default()
    });
    ui.run_until_parked();
}

/// Identify the actual visible preview paragraph, then require that its source row is visible too.
fn assert_source_matches_preview(fixture: &NativeMarkdown, ui: &mut gpui_kit::VisualTestContext) {
    let viewport = ui.debug_bounds("plugin-ui-preview-scroll").unwrap();
    let mut candidates = Vec::new();
    fixture.manager.live["markdown"].views["preview"]
        .root
        .visit(&mut |node| {
            if let Some(range) = node.source_range
                && node.id.ends_with("-paragraph")
            {
                let selector = Box::leak(format!("plugin-ui-{}", node.id).into_boxed_str());
                if let Some(bounds) = ui.debug_bounds(selector)
                    && bounds.bottom() > viewport.top()
                    && bounds.top() < viewport.bottom()
                {
                    candidates.push((range, (bounds.top() - viewport.top()).abs()));
                }
            }
        });
    let (range, _) = candidates
        .into_iter()
        .min_by(|left, right| left.1.partial_cmp(&right.1).unwrap())
        .expect("real visible preview paragraph");
    ui.update(|_, cx| {
        let editor = fixture.app.read(cx).editor.read(cx);
        let row = editor.text().offset_to_point(range.start).row;
        let visible = editor.visible_row_range().unwrap();
        let end_row = editor.text().offset_to_point(range.end.saturating_sub(1)).row;
        assert!(row < visible.end && end_row >= visible.start,
            "preview paragraph {:?} at source rows {row}..={end_row} must intersect source {visible:?}, source offset {:?}",
            range, editor.scroll_offset());
    });
}

/// The visible chain has the same Base keyboard behavior and workspace restoration as mode buttons.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_sync_scroll_control_remembers_workspace_and_withdraws_on_disable(
    cx: &mut TestAppContext,
) {
    let (mut fixture, ui) =
        NativeMarkdown::mount(cx, &[("notes.md", "# First\n"), ("other.md", "# Other\n")]);
    let workspace_root = Workspace::open(fixture.directory.path())
        .unwrap()
        .root()
        .to_path_buf();
    fixture.open("notes.md", ui);
    let modes = ui
        .debug_bounds("plugin-tool-markdown/preview/display-preview")
        .unwrap();
    let chain = ui
        .debug_bounds("plugin-tool-markdown/preview/display-sync")
        .unwrap();
    assert!(
        modes.right() <= chain.left(),
        "chain follows the three mode buttons"
    );
    let saved = crate::app::session::SessionState::load(&workspace_root);
    assert!(
        saved.legacy_display_payload("markdown/preview").is_none(),
        "missing preference defaults on"
    );
    fixture.click("plugin-tool-markdown/preview/display-sync", ui);
    assert_eq!(fixture.selected_tool("display-sync"), false);
    // Complete key-down and key-up: Base activates a focused button on release.
    for (key, expected) in [("space", true), ("enter", false)] {
        let keystroke = gpui_kit::Keystroke::parse(key).unwrap();
        ui.simulate_event(gpui_kit::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        ui.simulate_event(gpui_kit::KeyUpEvent { keystroke });
        ui.run_until_parked();
        fixture.settle(ui);
        assert_eq!(
            fixture.selected_tool("display-sync"),
            expected,
            "native {key} toggles the focused chain"
        );
    }
    assert_eq!(fixture.selected_tool("display-sync"), false);
    fixture.click("plugin-tool-markdown/preview/display-source", ui);
    fixture.click("plugin-tool-markdown/preview/display-sync", ui);
    assert_eq!(
        fixture.selected_tool("display-sync"),
        false,
        "source-only chain is disabled"
    );
    fixture.click("plugin-tool-markdown/preview/display-preview", ui);
    fixture.click("plugin-tool-markdown/preview/display-sync", ui);
    assert_eq!(
        fixture.selected_tool("display-sync"),
        false,
        "preview-only chain is disabled"
    );
    fixture.click("plugin-tool-markdown/preview/display-split", ui);
    fixture.open("other.md", ui);
    assert_eq!(fixture.selected_tool("display-sync"), false);
    let workspace = Workspace::open(fixture.directory.path()).unwrap();
    let (app, reopened) = NativeMarkdown::window(workspace, cx);
    fixture.app = app;
    fixture.open("notes.md", reopened);
    fixture.click("plugin-tool-markdown/preview/display-sync", reopened);
    assert_eq!(fixture.selected_tool("display-sync"), true);
    fixture.manager.disable("markdown").unwrap();
    fixture.settle(reopened);
    assert!(
        reopened
            .debug_bounds("plugin-tool-markdown/preview/display-sync")
            .is_none()
    );
    assert!(reopened.debug_bounds("plugin-ui-preview-root").is_none());
    let other = tempfile::tempdir().unwrap();
    assert!(
        crate::app::session::SessionState::load(other.path())
            .legacy_display_payload("markdown/preview")
            .is_none()
    );
}

/// The last manual source driver survives a delayed tall image, table layout, wrapping and divider reflow.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_sync_scroll_refreshes_image_table_wrap_and_divider_geometry(
    cx: &mut TestAppContext,
) {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
        time::Duration,
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (ready_tx, ready) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        let mut request = [0_u8; 4096];
        socket.read(&mut request).unwrap();
        ready_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(30)).unwrap();
        let png = image_preview::png(320, 1200);
        write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", png.len()).unwrap();
        socket.write_all(&png).unwrap();
    });
    let mut source = format!(
        "![延迟长图](http://{address}/tall.png)\n\n| 列一 | 列二 |\n| --- | --- |\n| 表格内容 | 同步内容 |\n\n"
    );
    source.push_str(
        &(0..90)
            .map(|index| format!("段落 {index:03}：{}\n\n", "中文软换行内容。".repeat(8)))
            .collect::<String>(),
    );
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", &source)]);
    fixture.open("notes.md", ui);
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    let position = ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).input_bounds().center());
    ui.simulate_event(gpui_kit::ScrollWheelEvent {
        position,
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-1200.))),
        ..Default::default()
    });
    ui.run_until_parked();
    fixture.settle(ui);
    release.send(()).unwrap();
    image_preview::complete(&mut fixture, ui);
    server.join().unwrap();
    assert_source_matches_preview(&fixture, ui);
    let old_width = ui
        .debug_bounds("plugin-ui-preview-root")
        .unwrap()
        .size
        .width;
    let divider = ui
        .debug_bounds("plugin-split-divider-markdown-layout")
        .unwrap()
        .center();
    ui.simulate_mouse_down(divider, gpui_kit::MouseButton::Left, Default::default());
    ui.run_until_parked();
    // Base starts the native drag after the threshold; a following movement resizes the panes.
    ui.simulate_mouse_move(
        divider + gpui_kit::point(px(-12.), px(0.)),
        gpui_kit::MouseButton::Left,
        Default::default(),
    );
    ui.run_until_parked();
    let destination = divider + gpui_kit::point(px(-180.), px(0.));
    ui.simulate_mouse_move(
        destination,
        Some(gpui_kit::MouseButton::Left),
        Default::default(),
    );
    ui.run_until_parked();
    ui.simulate_mouse_up(destination, gpui_kit::MouseButton::Left, Default::default());
    ui.run_until_parked();
    fixture.settle(ui);
    assert!(
        (ui.debug_bounds("plugin-ui-preview-root")
            .unwrap()
            .size
            .width
            - old_width)
            .abs()
            > px(100.),
        "real divider drag changes wrap geometry"
    );
    assert_source_matches_preview(&fixture, ui);
    wheel(ui, "plugin-ui-preview-scroll", -900.);
    // Test windows advance native frames explicitly; drain the public contract's bounded layout seek.
    for _ in 0..8 {
        fixture.settle(ui);
    }
    assert_source_matches_preview(&fixture, ui);
    ui.simulate_resize(gpui_kit::size(px(950.), px(700.)));
    for _ in 0..8 {
        fixture.settle(ui);
    }
    assert_source_matches_preview(&fixture, ui);
    // Public visibility requests use these production host actions; hiding keeps the same instance.
    let position = ui.debug_bounds("editor-source-pane").unwrap().center();
    ui.simulate_mouse_down(position, gpui_kit::MouseButton::Left, Default::default());
    ui.update(|_, cx| {
        fixture.app.update(cx, |app, cx| {
            app.hide_plugin_panel("markdown", "preview", cx)
        })
    });
    fixture.settle(ui);
    assert!(ui.debug_bounds("plugin-ui-preview-root").is_none());
    ui.simulate_mouse_up(position, gpui_kit::MouseButton::Left, Default::default());
    ui.update(|window, cx| {
        let panel = fixture.app.read(cx).plugin_panels["markdown/preview"].clone();
        panel.update(cx, |panel, cx| panel.show(window, cx));
    });
    fixture.settle(ui);
    wheel(ui, "plugin-ui-preview-scroll", -400.);
    for _ in 0..8 {
        fixture.settle(ui);
    }
    assert_source_matches_preview(&fixture, ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        source
    );
}

/// A valid independent SDK panel can expose and disable synchronization without optional presentation icons.
#[gpui::test]
#[ignore = "verify the current public SDK with scripts/verify-plugin-sdk.ps1 first"]
fn independent_viewport_without_tools_adds_no_host_functions(cx: &mut TestAppContext) {
    use serde_json::json;
    let package = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
    )
    .unwrap();
    let mut files = package.files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["id"] = json!("independent-viewport");
    manifest["scope"] = json!("workspace");
    manifest["api"]["required"] = json!({"package.assets":"^1", "ui.native":"^1", "ui.richtext":"^1",
        "editor.viewport":"^1", "editor.documents":"^1", "editor.layout":"^1", "configuration":"^1"});
    manifest["api"]["optional"] = json!({});
    manifest["permissions"] = json!(["assets.read", "editor.read"]);
    manifest["settings_hook"] = json!(false);
    manifest["settings"] = json!({"label":{"title":"UI", "value_type":{"kind":"string","max_length":120},
        "default":"composable-ui", "scope":"user", "apply":"restart_instance"}});
    manifest["panels"] = json!([{"id":"welcome", "title":"Independent viewport", "position":"editor", "file_extensions":["sample"]}]);
    let mut document = protocol::ui::Document::new(protocol::ui::Node::scroll(
        "viewport",
        protocol::ui::Node::text("source", "independent").source_range(0..12),
    ));
    document.editor_layout = true;
    document.root = protocol::ui::Node::row(
        "independent-layout",
        vec![
            protocol::ui::Node::new(
                "editor",
                protocol::ui::Kind::NativeEditor {
                    document: protocol::api::DocumentVersion {
                        id: "template".into(),
                        path: "notes.sample".into(),
                        revision: 0,
                    },
                },
            )
            .grow(),
            document.root.grow(),
        ],
    )
    .grow();
    document.editor_viewport = Some("viewport".into());
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    files.insert(
        "composed-ui.json".into(),
        serde_json::to_vec(&document).unwrap(),
    );
    let package = language_tests::packages::repack(files).unwrap();
    let (mut fixture, ui) =
        NativeMarkdown::mount_package(cx, &[("notes.sample", "independent\n")], &package);
    fixture.open("notes.sample", ui);
    assert!(ui.debug_bounds("editor-source-pane").is_some());
    assert!(ui.debug_bounds("plugin-ui-viewport").is_some());
    assert!(
        ui.debug_bounds("plugin-tool-markdown/preview/display-source")
            .is_none()
    );
    assert!(
        ui.debug_bounds("plugin-tool-markdown/preview/display-sync")
            .is_none()
    );
    assert!(
        fixture.manager.live["independent-viewport"].views["welcome"]
            .editor_viewport
            .is_some()
    );
    fixture.manager.disable("independent-viewport").unwrap();
    fixture.settle(ui);
    assert!(
        ui.debug_bounds("plugin-tool-markdown/preview/display-sync")
            .is_none()
    );
}

/// Revoking a source observer ends its pointer ownership even when release occurs in an ordinary file.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_sync_scroll_hidden_source_does_not_keep_a_released_pointer(
    cx: &mut TestAppContext,
) {
    let source = (0..100)
        .map(|index| format!("段落 {index:03}：释放后允许新的同步。\n\n"))
        .collect::<String>();
    let (mut fixture, ui) =
        NativeMarkdown::mount(cx, &[("notes.md", &source), ("ordinary.txt", "普通文件\n")]);
    fixture.open("notes.md", ui);
    let position = ui.debug_bounds("editor-source-pane").unwrap().center();
    ui.simulate_mouse_down(position, gpui_kit::MouseButton::Left, Default::default());
    fixture.open("ordinary.txt", ui);
    assert!(ui.debug_bounds("plugin-ui-preview-root").is_none());
    ui.simulate_mouse_up(position, gpui_kit::MouseButton::Left, Default::default());
    fixture.open("notes.md", ui);
    wheel(ui, "plugin-ui-preview-scroll", -1600.);
    for _ in 0..8 {
        fixture.settle(ui);
    }
    assert_source_matches_preview(&fixture, ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        source
    );
}

/// Edits, switches, closes and retirement seal genuine reverse-scroll requests before host execution.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_sync_scroll_late_locations_cannot_follow_changed_or_retired_sources(
    cx: &mut TestAppContext,
) {
    use protocol::api::{EditorOperation, RequestUpdate};
    let original = (0..100)
        .map(|index| format!("段落 {index:03}：同步对应。\n\n"))
        .collect::<String>();
    let (mut fixture, ui) = NativeMarkdown::mount(
        cx,
        &[
            ("edit.md", &original),
            ("switch.md", &original),
            ("close.md", &original),
            ("drag.md", &original),
            ("disable.md", &original),
            ("other.md", "另一份文档\n"),
        ],
    );
    fixture.open("other.md", ui);
    for name in ["edit.md", "switch.md", "close.md", "drag.md", "disable.md"] {
        fixture.open(name, ui);
        wheel(ui, "plugin-ui-preview-scroll", -1600.);
        super::super::composable_tests::pump(&mut fixture.manager, &fixture.app, ui);
        let mut requests = fixture
            .manager
            .live
            .get_mut("markdown")
            .unwrap()
            .take_editor_requests();
        assert_eq!(
            requests.len(),
            1,
            "one reverse-scroll intent owns one request"
        );
        let request = requests.pop().unwrap();
        let mut manual_offset = None;
        assert!(matches!(
            request.operation(),
            EditorOperation::LocateViewport {
                target: protocol::api::ViewportTarget::Source { .. },
                ..
            }
        ));
        match name {
            "edit.md" => {
                fixture.focus_editor(ui);
                ui.simulate_keystrokes("ctrl-home");
                ui.simulate_input("变化\n");
                ui.run_until_parked();
            }
            "switch.md" => fixture.open("other.md", ui),
            "close.md" => {
                let path = fixture.directory.path().join(name).canonicalize().unwrap();
                ui.update(|window, cx| {
                    fixture
                        .app
                        .update(cx, |app, cx| app.close_tab(path, window, cx))
                });
                ui.run_until_parked();
                fixture.open(name, ui);
            }
            "disable.md" => fixture.manager.disable("markdown").unwrap(),
            "drag.md" => {
                let bounds = ui.debug_bounds("editor-source-pane").unwrap();
                ui.simulate_mouse_down(
                    bounds.center(),
                    gpui_kit::MouseButton::Left,
                    Default::default(),
                );
                ui.run_until_parked();
                manual_offset =
                    Some(ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).scroll_offset()));
            }
            _ => unreachable!(),
        }
        ui.update(|_, cx| {
            fixture
                .app
                .read(cx)
                .extensions
                .read(cx)
                .worker
                .state
                .lock()
                .unwrap()
                .editor_requests
                .push(("markdown".into(), request.clone()))
        });
        fixture.settle(ui);
        if let Some(offset) = manual_offset {
            assert_eq!(
                ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).scroll_offset()),
                offset,
                "late reverse positioning cannot move a manually owned source viewport"
            );
            let bounds = ui.debug_bounds("editor-source-pane").unwrap();
            let outside = gpui_kit::point(bounds.right() + px(100.), bounds.center().y);
            ui.simulate_mouse_move(
                outside,
                Some(gpui_kit::MouseButton::Left),
                Default::default(),
            );
            ui.simulate_mouse_up(outside, gpui_kit::MouseButton::Left, Default::default());
            fixture.settle(ui);
            assert_eq!(
                ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).scroll_offset()),
                offset,
                "release and later paints cannot restore the rejected location"
            );
        }
        assert!(
            matches!(
                request.status(),
                RequestUpdate::Completed { result: Err(_) } | RequestUpdate::Cancelled { .. }
            ),
            "late reverse locate must fail after {name}"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join(name)).unwrap(),
            original
        );
        if name == "switch.md" {
            assert_eq!(
                ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
                "另一份文档\n"
            );
        }
        if name == "disable.md" {
            assert!(ui.debug_bounds("plugin-ui-preview-root").is_none());
        }
    }
}
