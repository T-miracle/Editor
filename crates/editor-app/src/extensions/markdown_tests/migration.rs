//! Actual installed guests import legacy intent and own both display controls and domain resources.
use super::*;
use harness::NativeMarkdown;

/// The actual platform specification must survive ZIP installation and native layout without losing its end.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_platform_specification_previews_and_scrolls_to_end(cx: &mut TestAppContext) {
    let text = include_str!("../../../../../docs/plugins/specs/plugin-api-platform.md");
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("platform.md", text)]);
    fixture.open("platform.md", ui);
    // The native document may normalize line endings; mappings always refer to its memory snapshot.
    let current = ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string());
    let document = &fixture.manager.live["markdown"].views["preview"];
    assert!(document.validate().is_ok());
    assert!(
        document.active_node("preview-limit").is_none(),
        "the actual file must not display the quota fallback"
    );
    // Walk public containers to locate the final mapped rich leaf, rather than relying on a parser-internal ID.
    fn final_leaf(node: &protocol::ui::Node, offset: usize) -> Option<&str> {
        match &node.kind {
            protocol::ui::Kind::Column { children } | protocol::ui::Kind::Row { children } => {
                children
                    .iter()
                    .rev()
                    .find_map(|child| final_leaf(child, offset))
            }
            protocol::ui::Kind::Scroll { content } => final_leaf(content, offset),
            protocol::ui::Kind::RichText { .. }
                if node
                    .source_range
                    .is_some_and(|range| range.start <= offset && offset < range.end) =>
            {
                Some(&node.id)
            }
            _ => None,
        }
    }
    // GPUI's debug selector requires a static label; retain this single test-only identity for the process.
    let last: &'static str = Box::leak(
        format!(
            "plugin-ui-{}",
            final_leaf(&document.root, current.trim_end().len() - 1)
                .expect("complete final paragraph")
        )
        .into_boxed_str(),
    );
    let viewport = ui.debug_bounds("plugin-ui-preview-scroll").unwrap();
    assert!(
        ui.debug_bounds("plugin-ui-preview-body")
            .unwrap()
            .size
            .height
            > viewport.size.height
    );
    ui.simulate_event(gpui_kit::ScrollWheelEvent {
        position: viewport.center(),
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-100_000.))),
        ..Default::default()
    });
    ui.run_until_parked();
    // This gesture deliberately overshoots the document. Allow native layout to clamp
    // against the final measured height before checking that delayed synchronization stays stable.
    fixture.settle(ui);
    let clamped = ui.debug_bounds("plugin-ui-preview-body").unwrap().top();
    fixture.settle(ui);
    let last = ui
        .debug_bounds(last)
        .expect("final paragraph rendered at the bottom");
    assert!(
        last.top() < viewport.bottom() && last.bottom() > viewport.top(),
        "the end of the full document remains reachable"
    );
    assert!(
        (ui.debug_bounds("plugin-ui-preview-body").unwrap().top() - clamped).abs() <= px(0.2),
        "synchronized source receipts must retain the clamped end of the document"
    );
}

/// Native text and IME keep their mounted surface while the background guest has not caught up.
#[gpui::test]
#[ignore = "build migrated markdown through scripts/build-plugins.ps1 first"]
fn migrated_markdown_keeps_native_input_visible_during_pending_guest_refresh(
    cx: &mut TestAppContext,
) {
    let text = format!(
        "# Large\n{}",
        "Paragraph with readonly derived preview content.\n\n".repeat(10_000)
    );
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("large.md", &text)]);
    fixture.open("large.md", ui);
    assert!(
        ui.debug_bounds("plugin-ui-preview-limit").is_some(),
        "oversized derived content should leave the plugin alive with an explicit limit"
    );
    fixture.focus_editor(ui);
    let revision = fixture.manager.live["markdown"].views["preview"].revision;
    let started = std::time::Instant::now();
    // Deliberately do not pump the manager: this represents an in-flight parse on the real worker.
    for input in ["a", "b", "c", "中文"] {
        ui.simulate_input(input);
        ui.run_until_parked();
        assert!(
            ui.debug_bounds("editor-source-pane").is_some(),
            "pending plugin work hid native text"
        );
        assert!(
            ui.debug_bounds("plugin-ui-preview-root").is_some(),
            "pending work discarded the previous readonly tree"
        );
        assert!(
            ui.update(|window, cx| fixture
                .app
                .read(cx)
                .editor
                .focus_handle(cx)
                .is_focused(window)),
            "pending plugin work stole focus"
        );
    }
    eprintln!(
        "readonly Markdown bytes={}, pending native input elapsed={:?}",
        text.len(),
        started.elapsed()
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(3),
        "native input waited for guest work: {:?}",
        started.elapsed()
    );
    assert_eq!(
        fixture.manager.live["markdown"].views["preview"].revision,
        revision
    );
    fixture.settle(ui);
    assert!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string())
            .starts_with("abc中文")
    );
    let version = ui.update(|_, cx| {
        let app = fixture.app.read(cx);
        app.plugin_document_version(app.active_text_tab_index().unwrap())
            .unwrap()
    });
    assert_eq!(
        fixture.manager.live["markdown"].views["preview"]
            .source
            .as_ref(),
        Some(&version)
    );
}

/// An imported SVG source choice remains local to text-capable SVG, while PNG keeps preview-only geometry.
#[gpui::test]
#[ignore = "build migrated Image through scripts/build-plugins.ps1 first"]
fn migrated_image_keeps_svg_intent_separate_from_raster_files(cx: &mut TestAppContext) {
    let package = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/svg.zip"),
    )
    .unwrap();
    let (mut fixture, ui) = NativeMarkdown::mount_package(
        cx,
        &[(
            "image.svg",
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\"/>",
        )],
        &package,
    );
    ui.update(|_, cx| {
        fixture.app.update(cx, |app, _| {
            app.session_state
                .legacy_preview_modes
                .insert("svg/preview".into(), serde_json::json!("source"));
        })
    });
    fixture.open("image.svg", ui);
    assert!(ui.debug_bounds("editor-source-pane").is_some());
    assert!(ui.debug_bounds("plugin-ui-preview-canvas").is_none());
    let png = fixture.directory.path().join("image.png");
    image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 128]))
        .save(&png)
        .unwrap();
    fixture.open("image.png", ui);
    assert!(ui.debug_bounds("editor-source-pane").is_none());
    assert!(
        ui.debug_bounds("plugin-tool-svg/preview/display-source")
            .is_none()
    );
    assert!(
        fixture.manager.live["svg"].views["preview"]
            .tools
            .is_empty()
    );
    fixture.open("image.svg", ui);
    assert!(ui.debug_bounds("editor-source-pane").is_some());
    assert!(ui.debug_bounds("plugin-ui-preview-canvas").is_none());
}

/// Session import preserves unrelated data, restores all guest switches, and never overrides new intent.
#[gpui::test]
#[ignore = "build migrated markdown through scripts/build-plugins.ps1 first"]
fn migrated_markdown_imports_legacy_intent_once_and_restores_after_restart(
    cx: &mut TestAppContext,
) {
    let (mut fixture, ui) = NativeMarkdown::mount(
        cx,
        &[("notes.md", "# Legacy\n"), ("same.markdown", "# Same\n")],
    );
    ui.update(|_, cx| {
        fixture.app.update(cx, |app, _| {
            app.session_state
                .legacy_preview_modes
                .insert("markdown/preview".into(), serde_json::json!("source"));
            app.session_state
                .legacy_preview_sync
                .insert("markdown/preview".into(), false);
            app.session_state
                .legacy_preview_toolbar
                .insert("markdown/preview".into(), false);
            app.session_state
                .legacy_preview_modes
                .insert("unrelated/preview".into(), serde_json::json!("future"));
            app.session_state.explorer_reveal_on_tab_switch = true;
        })
    });
    fixture.open("notes.md", ui);
    assert!(fixture.selected_tool("display-source"));
    assert!(!fixture.selected_tool("display-sync") && !fixture.selected_tool("display-toolbar"));
    assert!(ui.debug_bounds("editor-source-pane").is_some());
    assert!(ui.debug_bounds("plugin-ui-preview-root").is_none());
    assert!(ui.debug_bounds("editor-source-toolbar").is_none());
    ui.update(|_, cx| {
        let state = &fixture.app.read(cx).session_state;
        assert!(state.legacy_display_payload("markdown/preview").is_none());
        assert_eq!(
            state.legacy_display_payload("unrelated/preview").unwrap()["mode"],
            "future"
        );
        assert!(state.explorer_reveal_on_tab_switch);
    });
    // A new plugin choice wins over a repeated historical import on a fresh window.
    fixture.click("plugin-tool-markdown/preview/display-preview", ui);
    ui.update(|_, cx| {
        fixture.app.update(cx, |app, _| {
            app.session_state
                .legacy_preview_modes
                .insert("markdown/preview".into(), serde_json::json!("split"));
            app.session_state.save();
        })
    });
    let workspace = Workspace::open(fixture.directory.path()).unwrap();
    let (app, reopened) = NativeMarkdown::window(workspace, cx);
    fixture.app = app;
    fixture.open("same.markdown", reopened);
    assert!(fixture.selected_tool("display-preview"));
    assert!(reopened.debug_bounds("editor-source-pane").is_none());
    assert!(reopened.debug_bounds("plugin-ui-preview-root").is_some());
}

/// Ordinary terminal window focus exposes its guest functions to the right of its native visibility entry.
#[gpui::test]
#[ignore = "build migrated terminal through scripts/build-plugins.ps1 first"]
fn migrated_terminal_functions_use_public_tools_and_keep_window_visibility_separate(
    cx: &mut TestAppContext,
) {
    let package = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/terminal.zip"),
    )
    .unwrap();
    let (mut fixture, ui) = NativeMarkdown::mount_package(cx, &[], &package);
    let window = ui.debug_bounds("plugin-window-terminal/terminal").unwrap();
    if ui.debug_bounds("plugin-ui-output").is_none() {
        fixture.click("plugin-window-terminal/terminal", ui);
    }
    fixture.click("plugin-ui-output", ui);
    let function = ui
        .debug_bounds("plugin-tool-terminal/terminal/terminal.new")
        .unwrap();
    let separator = ui.debug_bounds("plugin-tool-group-separator").unwrap();
    assert!(window.right() <= separator.left() && separator.right() <= function.left());
    assert_eq!(fixture.manager.live["terminal"].process_count(), 1);
    fixture.click("plugin-tool-terminal/terminal/terminal.new", ui);
    assert_eq!(fixture.manager.live["terminal"].process_count(), 2);
    fixture.click("plugin-tool-terminal/terminal/terminal.menu", ui);
    assert!(
        fixture.manager.live["terminal"].views["terminal"]
            .menu
            .is_some()
    );
    ui.simulate_keystrokes("escape");
    ui.run_until_parked();
    fixture.settle(ui);
    fixture.click("plugin-window-terminal/terminal", ui);
    assert!(ui.debug_bounds("plugin-ui-output").is_none());
    assert!(
        ui.debug_bounds("plugin-tool-terminal/terminal/terminal.new")
            .is_none()
    );
    assert_eq!(
        fixture.manager.live["terminal"].process_count(),
        2,
        "hiding retains owned sessions"
    );
    fixture.manager.disable("terminal").unwrap();
    fixture.settle(ui);
    assert_eq!(fixture.manager.resource_count(), 0);
    assert!(ui.debug_bounds("plugin-window-terminal/terminal").is_none());
}
