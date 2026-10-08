//! Native input at the semantic queue limits must retain its order across Base history dispatch.
use super::*;

/// Crossing both bounds preserves the overflowing user event and every subsequent native command.
#[gpui::test]
#[ignore = "build javascript.zip for the approved private Node runtime"]
fn bounded_linked_queue_keeps_overflowing_input_after_native_history(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("bounded.linked");
    let source = "<!-- 中文🙂 -->\n<item>内容🙂</item>\n";
    std::fs::write(&path, source).unwrap();
    let log = directory.path().join("wire.jsonl");
    let package = editing_fixture::package(
        "bounded-language",
        "fixture",
        true,
        true,
        &log,
        2000,
        "bounded",
    );
    let (app, visual, _manager) = shell(cx, directory.path(), vec![package]);
    open(&app, &path, visual);
    visual.update(|_, cx| {
        app.read(cx).language_servers["fixture"]
            .prepare_until_ready()
            .unwrap()
    });
    let start = source.find("<item>").unwrap() + 2;
    immediate_caret(&app, start, visual);
    visual.simulate_input("X");
    visual.simulate_keystrokes("ctrl-z");
    // Cut at the restored collapsed caret is inert, but remains a real native command in the FIFO.
    visual.simulate_keystrokes(&vec!["ctrl-x"; 126].join(" "));
    assert_text(&app, source, visual);
    // These adjacent dispatches include another event during the deferred native history turn.
    visual.update(|window, cx| {
        window.dispatch_keystroke(gpui_kit::Keystroke::parse("Y").unwrap(), cx);
        window.dispatch_keystroke(gpui_kit::Keystroke::parse("Z").unwrap(), cx);
    });
    let entered = source.replacen("item", "iYZtem", 1);
    await_text(
        &app,
        &entered,
        visual,
        "128-command overflow after Undo keeps Y then Z",
    );
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    visual.simulate_keystrokes("ctrl-y");
    assert_text(&app, &entered, visual);
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);

    // The command crossing the item bound can itself be history; it must follow the queued Undo.
    immediate_caret(&app, start, visual);
    visual.simulate_input("X");
    visual.simulate_keystrokes("ctrl-z");
    visual.simulate_keystrokes(&vec!["ctrl-x"; 126].join(" "));
    visual.simulate_keystrokes("ctrl-y");
    let restored = source.replacen("item", "iXtem", 1);
    await_text(
        &app,
        &restored,
        visual,
        "129th native Redo follows the pending Undo",
    );
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    visual.simulate_keystrokes("ctrl-y");
    assert_text(&app, &restored, visual);
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);

    immediate_caret(&app, start, visual);
    visual.simulate_input("X");
    visual.simulate_keystrokes("ctrl-z");
    let pasted = "Q".repeat(64 * 1024);
    visual
        .update(|_, cx| cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(pasted.clone())));
    visual.update(|window, cx| {
        window.dispatch_keystroke(gpui_kit::Keystroke::parse("ctrl-v").unwrap(), cx);
        window.dispatch_keystroke(gpui_kit::Keystroke::parse("Y").unwrap(), cx);
    });
    let entered = source.replacen("item", &format!("i{pasted}Ytem"), 1);
    await_text(
        &app,
        &entered,
        visual,
        "64KiB overflow follows queued Undo without losing the paste",
    );
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    visual.simulate_keystrokes("ctrl-y");
    assert_text(&app, &entered, visual);
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
}
