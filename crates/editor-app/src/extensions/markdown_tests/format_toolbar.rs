//! Format commands cross the delivered guest, public requests and the native undoable document.
use super::*;
use harness::NativeMarkdown;

const TOOLS: &[(&str, &str)] = &[
    ("plugin-ui-format-heading", "# 中文"),
    ("plugin-ui-format-bold", "**中文**"),
    ("plugin-ui-format-italic", "*中文*"),
    ("plugin-ui-format-strike", "~~中文~~"),
    ("plugin-ui-format-inline-code", "`中文`"),
    ("plugin-ui-format-code-block", "```\n中文\n```"),
    ("plugin-ui-format-quote", "> 中文"),
    ("plugin-ui-format-unordered", "- 中文"),
    ("plugin-ui-format-ordered", "1. 中文"),
    ("plugin-ui-format-task", "- [ ] 中文"),
    ("plugin-ui-format-link", "[中文](https://example.com)"),
    ("plugin-ui-format-image", "![中文](image.png)"),
    ("plugin-ui-format-table", "| 中文 |  |"),
];

/// Pointer focus must preserve UTF-8 selection and each formatting command owns one undo step.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_toolbar_formats_chinese_and_selects_templates(cx: &mut TestAppContext) {
    let original = "你好，世界";
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", original)]);
    fixture.open("notes.md", ui);
    let toolbar = ui
        .debug_bounds("editor-source-toolbar")
        .expect("source toolbar");
    let source = ui.debug_bounds("editor-source-pane").unwrap();
    assert!(toolbar.top() >= source.top() && toolbar.bottom() < source.bottom());
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-a");
    fixture.click("plugin-ui-format-bold", ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        format!("**{original}**")
    );
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).selected_range()),
        2..2 + original.len()
    );
    assert_eq!(
        std::fs::read_to_string(fixture.directory.path().join("notes.md")).unwrap(),
        original
    );
    // The completed request returns focus to the visible editor for ordinary undo and template input.
    ui.simulate_keystrokes("ctrl-z");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        original
    );
    ui.simulate_keystrokes("ctrl-y");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        format!("**{original}**")
    );
    ui.simulate_keystrokes("ctrl-end");
    fixture.click("plugin-ui-format-link", ui);
    assert!(!ui.update(|_, cx| {
        fixture
            .app
            .read(cx)
            .editor
            .read(cx)
            .selected_text()
            .to_string()
            .is_empty()
    }));
    ui.simulate_input("新的链接");
    ui.run_until_parked();
    fixture.settle(ui);
    assert!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string())
            .contains("[新的链接](")
    );
    fixture.click("editor-preview-source-mode", ui);
    assert!(ui.debug_bounds("editor-source-toolbar").is_some());
    for selector in [
        "plugin-ui-format-heading",
        "plugin-ui-format-bold",
        "plugin-ui-format-italic",
        "plugin-ui-format-strike",
        "plugin-ui-format-inline-code",
        "plugin-ui-format-code-block",
        "plugin-ui-format-quote",
        "plugin-ui-format-unordered",
        "plugin-ui-format-ordered",
        "plugin-ui-format-task",
        "plugin-ui-format-link",
        "plugin-ui-format-image",
        "plugin-ui-format-table",
    ] {
        assert!(ui.debug_bounds(selector).is_some());
    }
    fixture.click("editor-preview-preview-mode", ui);
    assert!(ui.debug_bounds("editor-source-toolbar").is_none());
    fixture.click("editor-preview-split-mode", ui);
    assert!(ui.debug_bounds("editor-source-toolbar").is_some());
    fixture.manager.disable("markdown").unwrap();
    fixture.settle(ui);
    assert!(ui.debug_bounds("editor-source-toolbar").is_none());
}

/// A narrow native divider and keyboard intent exercise every public formatting control without clipping.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_toolbar_wraps_all_commands_and_cancels_superseded_intent(
    cx: &mut TestAppContext,
) {
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", "中文")]);
    fixture.open("notes.md", ui);
    for &(selector, expected) in TOOLS {
        fixture.focus_editor(ui);
        ui.simulate_keystrokes("ctrl-a");
        fixture.click(selector, ui);
        let text = ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string());
        assert!(text.contains(expected), "{selector}: {text:?}");
        ui.simulate_keystrokes("ctrl-z");
        ui.run_until_parked();
        fixture.settle(ui);
        assert_eq!(
            ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
            "中文"
        );
    }
    let source = ui.debug_bounds("editor-source-pane").unwrap();
    let divider = ui.debug_bounds("editor-preview-divider").unwrap().center();
    let target = point(source.left() + px(120.), divider.y);
    ui.simulate_mouse_down(divider, MouseButton::Left, Default::default());
    ui.run_until_parked();
    ui.simulate_mouse_move(
        point(divider.x - px(12.), divider.y),
        MouseButton::Left,
        Default::default(),
    );
    ui.run_until_parked();
    ui.simulate_mouse_move(target, MouseButton::Left, Default::default());
    ui.run_until_parked();
    ui.simulate_mouse_up(target, MouseButton::Left, Default::default());
    ui.run_until_parked();
    fixture.settle(ui);
    let source = ui.debug_bounds("editor-source-pane").unwrap();
    assert!(
        source.size.width <= px(165.),
        "divider must produce a narrow source: {source:?}"
    );
    let toolbar = ui.debug_bounds("editor-source-toolbar").unwrap();
    let mut bounds = Vec::new();
    for &(selector, _) in TOOLS {
        let button = ui.debug_bounds(selector).unwrap();
        assert!(
            button.left() >= toolbar.left() - px(1.) && button.right() <= toolbar.right() + px(1.),
            "{selector} is clipped: {button:?}, toolbar {toolbar:?}"
        );
        assert!(button.bottom() <= toolbar.bottom() + px(1.));
        assert!(
            bounds.iter().all(
                |previous: &Bounds<Pixels>| button.right() <= previous.left()
                    || previous.right() <= button.left()
                    || button.bottom() <= previous.top()
                    || previous.bottom() <= button.top()
            ),
            "toolbar buttons must not overlap"
        );
        bounds.push(button);
    }
    // Two intents enter real WASM before the worker runs. The later keyboard click cancels the first.
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-a");
    let first = ui
        .debug_bounds("plugin-ui-format-heading")
        .unwrap()
        .center();
    ui.simulate_click(first, Default::default());
    ui.run_until_parked();
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
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        "**中文**"
    );
    ui.simulate_keystrokes("ctrl-z");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        "中文"
    );
}
