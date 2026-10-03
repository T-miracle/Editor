//! Mode buttons exercise native activation, retained document state and workspace restoration.
use super::*;
use harness::NativeMarkdown;

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
    let preview = ui.debug_bounds("editor-preview-pane").unwrap();
    assert!(source.right() <= preview.left() + px(2.));
    // This first assertion is the user-visible RED when the delivered package has no mode contribution.
    assert!(ui.debug_bounds("editor-preview-source-mode").is_some());
    let tools = ui.debug_bounds("editor-panel-tools").unwrap();
    let separator = ui.debug_bounds("editor-preview-mode-separator").unwrap();
    let first = ui.debug_bounds("editor-preview-source-mode").unwrap();
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
    fixture.click("editor-preview-source-mode", ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).selected_range()),
        selection
    );
    assert!(ui.debug_bounds("editor-source-pane").is_some());
    assert!(ui.debug_bounds("editor-preview-pane").is_none());
    fixture.open("other.md", ui);
    assert!(ui.debug_bounds("editor-preview-pane").is_none());
    fixture.click("editor-preview-preview-mode", ui);
    assert!(ui.debug_bounds("editor-source-pane").is_none());
    let preview = ui.debug_bounds("editor-preview-pane").unwrap();
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
    fixture.click("editor-preview-source-mode", ui);
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-z");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        "# Original\n"
    );
    // Complete both native key events: Base buttons activate on key-up, unlike simulated IME input.
    fixture.click("editor-preview-source-mode", ui);
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
            && ui.debug_bounds("editor-preview-pane").is_some()
    );
    fixture.click("editor-preview-preview-mode", ui);
    let workspace = Workspace::open(fixture.directory.path()).unwrap();
    let saved = crate::app::session::SessionState::load(workspace.root());
    assert_eq!(
        serde_json::to_value(saved).unwrap()["editor_preview_modes"]["markdown/preview"],
        "preview"
    );
    // Restore in another real window for the same workspace, with a fresh source entity token.
    let (app, reopened) = NativeMarkdown::window(workspace, cx);
    fixture.app = app;
    fixture.open("notes.md", reopened);
    assert!(reopened.debug_bounds("editor-source-pane").is_none());
    assert!(reopened.debug_bounds("editor-preview-pane").is_some());
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
            .debug_bounds("editor-preview-source-mode")
            .is_none()
    );
    fixture.open("notes.md", reopened);
    fixture.manager.disable("markdown").unwrap();
    fixture.settle(reopened);
    assert!(
        reopened
            .debug_bounds("editor-preview-source-mode")
            .is_none()
    );
    assert!(reopened.debug_bounds("editor-preview-pane").is_none());
    assert!(reopened.debug_bounds("editor-source-pane").is_some());
    let unrelated = tempfile::tempdir().unwrap();
    let other = crate::app::session::SessionState::load(unrelated.path());
    assert!(
        serde_json::to_value(other).unwrap()["editor_preview_modes"]
            .as_object()
            .unwrap()
            .is_empty()
    );
}
