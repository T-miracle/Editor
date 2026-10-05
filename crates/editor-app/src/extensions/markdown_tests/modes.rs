//! Mode buttons exercise native activation, retained document state and workspace restoration.
use super::*;
use harness::NativeMarkdown;

/// The complete native layout must render at the default thread stack with a usable editor extent.
#[gpui::test]
#[ignore = "build migrated markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_initial_layout_borrows_editor_with_full_height(cx: &mut TestAppContext) {
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", "# 初次打开\n")]);
    fixture.open("notes.md", ui);
    let source = ui
        .debug_bounds("editor-source-pane")
        .expect("mounted source");
    let preview = ui
        .debug_bounds("plugin-ui-preview-root")
        .expect("mounted preview");
    assert!(
        source.size.height > px(500.) && preview.size.height > px(500.),
        "source={source:?}, preview={preview:?}"
    );
    assert!(ui.debug_bounds("editor-source-toolbar").is_some());
}

/// SVG opts into the same public presentation contract; native controls require no language branch.
#[gpui::test]
#[ignore = "build svg through scripts/build-plugins.ps1 first"]
fn delivered_svg_modes_share_native_status_buttons_and_preserve_document(cx: &mut TestAppContext) {
    let package = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/svg.zip"),
    )
    .unwrap();
    let original = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\"/>";
    let (mut fixture, ui) = NativeMarkdown::mount_package(cx, &[("image.svg", original)], &package);
    fixture.open("image.svg", ui);
    let tools = ui.debug_bounds("plugin-windows-group").unwrap();
    let separator = ui
        .debug_bounds("plugin-tool-group-separator")
        .expect("SVG contributes the generic mode group");
    let source_button = ui
        .debug_bounds("plugin-tool-svg/preview/display-source")
        .unwrap();
    assert!(tools.right() <= separator.left() && separator.right() <= source_button.left());
    let identity = ui.update(|_, cx| fixture.app.read(cx).editor.entity_id());
    for (selector, source, preview) in [
        ("plugin-tool-svg/preview/display-source", true, false),
        ("plugin-tool-svg/preview/display-preview", false, true),
        ("plugin-tool-svg/preview/display-split", true, true),
    ] {
        fixture.click(selector, ui);
        assert_eq!(ui.debug_bounds("editor-source-pane").is_some(), source);
        assert_eq!(
            ui.debug_bounds("plugin-ui-preview-canvas").is_some(),
            preview
        );
        assert_eq!(
            ui.update(|_, cx| fixture.app.read(cx).editor.entity_id()),
            identity
        );
        assert_eq!(
            ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
            original
        );
    }
    ui.update(|_, cx| apply_theme(builtin_theme(true), cx));
    fixture.settle(ui);
    for selector in [
        "plugin-tool-svg/preview/display-source",
        "plugin-tool-svg/preview/display-split",
        "plugin-tool-svg/preview/display-preview",
    ] {
        assert!(
            ui.debug_bounds(selector).is_some(),
            "dark-theme SVG mode {selector}"
        );
    }
}

/// Mode changes alter layout alone; source identity, selection and undo history remain native.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_modes_remember_workspace_and_preserve_native_undo(cx: &mut TestAppContext) {
    let (mut fixture, ui) = NativeMarkdown::mount(
        cx,
        &[
            ("notes.md", "# Original\n"),
            ("other.md", "# Other\n"),
            ("plain.txt", "plain"),
        ],
    );
    fixture.open("notes.md", ui);
    let source = ui.debug_bounds("editor-source-pane").unwrap();
    let preview = ui.debug_bounds("plugin-ui-preview-root").unwrap();
    assert!(source.right() <= preview.left() + px(2.));
    // This first assertion is the user-visible RED when the delivered package has no mode contribution.
    assert!(
        ui.debug_bounds("plugin-tool-markdown/preview/display-source")
            .is_some()
    );
    let tools = ui.debug_bounds("plugin-windows-group").unwrap();
    let separator = ui.debug_bounds("plugin-tool-group-separator").unwrap();
    let first = ui
        .debug_bounds("plugin-tool-markdown/preview/display-source")
        .unwrap();
    assert!(tools.right() <= separator.left() && separator.right() <= first.left());
    assert!(separator.size.height <= px(18.));
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-a");
    ui.update(|_, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string("# Unsaved mode test\n".into()))
    });
    ui.simulate_keystrokes("ctrl-v");
    ui.run_until_parked();
    fixture.settle(ui);
    let identity = ui.update(|_, cx| fixture.app.read(cx).editor.entity_id());
    ui.simulate_keystrokes("ctrl-a");
    let selection = ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).selected_range());
    fixture.click("plugin-tool-markdown/preview/display-source", ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).selected_range()),
        selection
    );
    assert!(ui.debug_bounds("editor-source-pane").is_some());
    assert!(ui.debug_bounds("plugin-ui-preview-root").is_none());
    fixture.open("other.md", ui);
    assert!(ui.debug_bounds("plugin-ui-preview-root").is_none());
    fixture.click("plugin-tool-markdown/preview/display-preview", ui);
    assert!(ui.debug_bounds("editor-source-pane").is_none());
    let preview = ui.debug_bounds("plugin-ui-preview-root").unwrap();
    let body = ui.debug_bounds("editor-panel-content").unwrap();
    assert!((preview.size.width - body.size.width).abs() <= px(2.));
    fixture.open("notes.md", ui);
    assert!(ui.debug_bounds("editor-source-pane").is_none());
    assert!(!ui.update(|window, cx| {
        fixture
            .app
            .read(cx)
            .editor
            .focus_handle(cx)
            .is_focused(window)
    }));
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.entity_id()),
        identity
    );
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).selected_range()),
        selection
    );
    fixture.click("plugin-tool-markdown/preview/display-source", ui);
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-z");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        "# Original\n"
    );
    // Complete both native key events: Base buttons activate on key-up, unlike simulated IME input.
    fixture.click("plugin-tool-markdown/preview/display-source", ui);
    for key in ["tab", "space"] {
        let keystroke = gpui_kit::Keystroke::parse(key).unwrap();
        ui.simulate_event(gpui_kit::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        ui.simulate_event(gpui_kit::KeyUpEvent { keystroke });
    }
    ui.run_until_parked();
    fixture.settle(ui);
    assert!(
        ui.debug_bounds("editor-source-pane").is_some()
            && ui.debug_bounds("plugin-ui-preview-root").is_some()
    );
    fixture.click("plugin-tool-markdown/preview/display-preview", ui);
    let workspace = Workspace::open(fixture.directory.path()).unwrap();
    let saved = crate::app::session::SessionState::load(workspace.root());
    assert!(saved.legacy_display_payload("markdown/preview").is_none());
    assert_eq!(fixture.selected_tool("display-preview"), true);
    // Restore in another real window for the same workspace, with a fresh source entity token.
    let (app, reopened) = NativeMarkdown::window(workspace, cx);
    fixture.app = app;
    fixture.open("notes.md", reopened);
    assert!(reopened.debug_bounds("editor-source-pane").is_none());
    assert!(reopened.debug_bounds("plugin-ui-preview-root").is_some());
    assert!(!reopened.update(|window, cx| {
        fixture
            .app
            .read(cx)
            .editor
            .focus_handle(cx)
            .is_focused(window)
    }));
    reopened.update(|_, cx| apply_theme(builtin_theme(true), cx));
    fixture.settle(reopened);
    fixture.open("plain.txt", reopened);
    assert!(
        reopened
            .debug_bounds("plugin-tool-markdown/preview/display-source")
            .is_none()
    );
    fixture.open("notes.md", reopened);
    fixture.manager.disable("markdown").unwrap();
    fixture.settle(reopened);
    assert!(
        reopened
            .debug_bounds("plugin-tool-markdown/preview/display-source")
            .is_none()
    );
    assert!(reopened.debug_bounds("plugin-ui-preview-root").is_none());
    assert!(reopened.debug_bounds("editor-source-pane").is_some());
    let unrelated = tempfile::tempdir().unwrap();
    let other = crate::app::session::SessionState::load(unrelated.path());
    assert!(
        serde_json::to_value(other)
            .unwrap()
            .get("editor_preview_modes")
            .is_none()
    );
}
