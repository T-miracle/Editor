//! A reidentified official parser exercises semantic names that standard LSP cannot link verbatim.
use super::*;
use gpui_kit::EntityInputHandler as _;

/// Both first-input ends, history and IME use an installed foreign-ID HTML package and one native editor.
#[gpui::test]
#[ignore = "build html.zip for the approved private Node runtime and semantic editing service"]
fn unknown_html_semantic_pairs_preserve_mixed_case_cold_input_history_and_ime(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("semantic.html");
    let source = "<main title=\"中文🙂 DIV\"><DIV>body<span></span></div><!-- DIV --></main>";
    std::fs::write(&path, source).unwrap();
    let package = editing_fixture::relabel_html("foreign-semantic-markup");
    let (app, visual, _manager) = shell(cx, directory.path(), vec![package]);
    open(&app, &path, visual);
    visual.update(|window, cx| window.simulate_next_frame(cx));
    let server = visual.update(|_, cx| app.read(cx).language_servers["html"].clone());
    server.prepare_until_ready().unwrap();
    // Initialization and source painting precede the new caret, with no pair request or cache wait.
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let start = source.find("<DIV>").unwrap() + 1;
    let close = source.find("</div>").unwrap() + 2;
    for (caret, name) in [(start + 1, "DXIV"), (close + 1, "dXiv")] {
        immediate_caret(&app, caret, visual);
        visual.simulate_input("X");
        let changed = source.replacen("<DIV>", &format!("<{name}>"), 1).replacen(
            "</div>",
            &format!("</{name}>"),
            1,
        );
        await_text(&app, &changed, visual, "foreign semantic cold input");
        visual.simulate_keystrokes("ctrl-z");
        assert_text(&app, source, visual);
        visual.simulate_keystrokes("ctrl-y");
        assert_text(&app, &changed, visual);
        visual.simulate_keystrokes("ctrl-z");
        assert_text(&app, source, visual);
    }

    // The actual installed input handler retains preedit locally and commits both semantically linked ends.
    immediate_caret(&app, close + 1, visual);
    let bridge = visual.update(|_, cx| app.read(cx).language_edits.bridge.clone().unwrap());
    visual.update(|window, cx| {
        bridge.update(cx, |bridge, cx| {
            bridge.replace_and_mark_text_in_range(None, "拼", Some(0..1), window, cx);
            bridge.replace_text_in_range(None, "标签", window, cx);
        });
    });
    let composed = source
        .replacen("<DIV>", "<d标签iv>", 1)
        .replacen("</div>", "</d标签iv>", 1);
    await_text(&app, &composed, visual, "foreign semantic IME commit");
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    visual.simulate_keystrokes("ctrl-y");
    assert_text(&app, &composed, visual);
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);

    // Canceling a composition preserves the original distinct names; no peer normalization is a user edit.
    immediate_caret(&app, start + 1, visual);
    visual.update(|window, cx| {
        bridge.update(cx, |bridge, cx| {
            bridge.replace_and_mark_text_in_range(None, "拼", Some(0..1), window, cx);
        });
    });
    // Observe preedit before canceling, so an unchanged document cannot falsely pass while input is queued.
    await_text(
        &app,
        &source.replacen("<DIV>", "<D拼IV>", 1),
        visual,
        "foreign semantic IME preedit",
    );
    visual.update(|window, cx| {
        bridge.update(cx, |bridge, cx| {
            bridge.replace_and_mark_text_in_range(None, "", None, window, cx);
            bridge.unmark_text(window, cx);
        });
    });
    await_text(&app, source, visual, "foreign semantic IME cancel");
}
