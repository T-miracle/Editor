//! Preview task clicks cross the actual guest and the sole native document/undo transaction.
use super::*;
use harness::NativeMarkdown;

/// Every blank marker admitted by the installed parser is editable, preserving its original byte on undo.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_parser_blank_tasks_toggle_and_undo(cx: &mut TestAppContext) {
    let original = "- [\t] 制表符\n- [\u{b}] 垂直空白\n- [\u{c}] 换页空白\n";
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", original)]);
    fixture.open("notes.md", ui);
    fixture.click("plugin-checkbox-marker-b-2-task", ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        original.replacen("[\t]", "[x]", 1),
        "a parser-created tab marker must support the same native Toggle"
    );
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-z");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        original
    );
}

/// A Chinese task changes only its marker and one undo restores the complete original document.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_task_click_updates_source_with_one_undo(cx: &mut TestAppContext) {
    let original = "- [ ] 第一项\n  - [x] 子项\n- [X] 第三项\n";
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", original)]);
    fixture.open("notes.md", ui);
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-end");
    let selection = ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).selected_range());
    fixture.click("plugin-checkbox-marker-b-2-task", ui);
    let checked = original.replacen("[ ]", "[x]", 1);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        checked,
        "a native preview click must update precisely the first source marker; node={:?}; bounds={:?}; feedback={:?}",
        fixture.manager.live["markdown"].views["preview"].active_node("b-2-task"),
        ui.debug_bounds("plugin-ui-b-2-task"),
        fixture.manager.live["markdown"].views["preview"].active_node("format-error")
    );
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).selected_range()),
        selection,
        "task writes preserve the source caret"
    );
    assert!(matches!(
        fixture.manager.live["markdown"].views["preview"]
            .active_node("b-2-task")
            .map(|node| &node.kind),
        Some(protocol::ui::Kind::Checkbox { checked: true, .. })
    ));
    assert!(ui.debug_bounds("plugin-ui-b-2-task").is_some());
    assert_eq!(
        std::fs::read_to_string(fixture.directory.path().join("notes.md")).unwrap(),
        original,
        "task writes remain ordinary unsaved document edits"
    );
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-z");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        original
    );
    assert!(matches!(
        fixture.manager.live["markdown"].views["preview"]
            .active_node("b-2-task")
            .map(|node| &node.kind),
        Some(protocol::ui::Kind::Checkbox { checked: false, .. })
    ));
    ui.simulate_keystrokes("ctrl-y");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        checked
    );
    fixture.click("plugin-checkbox-marker-b-2-task", ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        original,
        "the same native checkbox supports unchecking after fresh preview publication"
    );
}

/// Nested/uppercase markers stay independent; native keyboard activation also works with source hidden.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_nested_tasks_support_preview_keyboard_and_keep_other_content_readonly(
    cx: &mut TestAppContext,
) {
    let original =
        "- [ ] 父任务\n  - [X] 子任务\n- [x] 末项\n\n普通文字 [ ]\n\n```\n- [ ] 代码\n```\n";
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", original)]);
    fixture.open("notes.md", ui);
    let mut task_count = 0;
    fixture.manager.live["markdown"].views["preview"]
        .root
        .visit(&mut |node| {
            if matches!(node.kind, protocol::ui::Kind::Checkbox { .. }) {
                task_count += 1;
            }
        });
    assert_eq!(
        task_count, 3,
        "plain prose and literal code cannot become editable tasks"
    );
    fixture.click("plugin-checkbox-marker-b-20-task", ui);
    let unchecked =
        "- [ ] 父任务\n  - [ ] 子任务\n- [x] 末项\n\n普通文字 [ ]\n\n```\n- [ ] 代码\n```\n";
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        unchecked,
        "only the nested uppercase marker changes"
    );
    fixture.click("plugin-tool-markdown/preview/display-preview", ui);
    assert!(ui.debug_bounds("editor-source-pane").is_none());
    fixture.click("plugin-checkbox-marker-b-20-task", ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        unchecked.replacen("[ ] 子任务", "[x] 子任务", 1)
    );
    // Base controls activate on a complete key gesture, retaining their keyed focus after publication.
    let keystroke = gpui_kit::Keystroke::parse("space").unwrap();
    ui.simulate_event(gpui_kit::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    ui.simulate_event(gpui_kit::KeyUpEvent { keystroke });
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        unchecked,
        "Space toggles the focused preview task without a source input target"
    );
    for dark in [true, false] {
        ui.update(|_, cx| apply_theme(builtin_theme(dark), cx));
        fixture.settle(ui);
        let marker = ui.debug_bounds("plugin-checkbox-marker-b-20-task").unwrap();
        assert_eq!(marker.size, size(px(16.), px(16.)));
        assert!(matches!(
            fixture.manager.live["markdown"].views["preview"]
                .active_node("b-20-task")
                .map(|node| &node.kind),
            Some(protocol::ui::Kind::Checkbox { checked: false, .. })
        ));
    }
    fixture.click("plugin-tool-markdown/preview/display-source", ui);
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-z");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        unchecked.replacen("[ ] 子任务", "[x] 子任务", 1),
        "one undo reverses only the keyboard click"
    );
}

/// Editing through a preview control must retain its keyboard target while source remains visible.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_split_task_retains_focus_for_successive_keyboard_toggles(
    cx: &mut TestAppContext,
) {
    let original = "- [ ] 中文\n";
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", original)]);
    fixture.open("notes.md", ui);
    fixture.click("plugin-checkbox-marker-b-2-task", ui);
    assert!(
        !ui.update(|window, cx| {
            fixture
                .app
                .read(cx)
                .editor
                .focus_handle(cx)
                .is_focused(window)
        }),
        "a task click must not redirect the next key to source text"
    );
    let keystroke = gpui_kit::Keystroke::parse("space").unwrap();
    ui.simulate_event(gpui_kit::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    ui.simulate_event(gpui_kit::KeyUpEvent { keystroke });
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        original,
        "the second complete key gesture unchecks the same task"
    );
    assert!(!ui.update(|window, cx| {
        fixture
            .app
            .read(cx)
            .editor
            .focus_handle(cx)
            .is_focused(window)
    }));
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-z");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        "- [x] 中文\n"
    );
}
