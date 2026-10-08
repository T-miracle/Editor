//! Delayed semantic replies exercise real native input, history, command ordering and retirement.
use super::*;
use gpui_kit::EntityInputHandler as _;

/// A plugin's rename input owns native typing and editing actions before its proposal is applied.
#[gpui::test]
#[ignore = "build javascript.zip for the approved private Node runtime"]
fn public_rename_field_owns_typing_confirmation_and_native_history(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rename.linked");
    let source = "<!-- 中文🙂 -->\n<item>内容🙂</item>\n";
    std::fs::write(&path, source).unwrap();
    let log = directory.path().join("wire.jsonl");
    let package = editing_fixture::package(
        "rename-language",
        "fixture",
        true,
        true,
        &log,
        30,
        "versioned",
    );
    let (app, visual, _manager) = shell(cx, directory.path(), vec![package]);
    open(&app, &path, visual);
    let server = visual.update(|_, cx| app.read(cx).language_servers["fixture"].clone());
    server.prepare_until_ready().unwrap();
    immediate_caret(&app, source.find("<item>").unwrap() + 2, visual);
    visual.simulate_keystrokes("f2");
    await_condition(
        visual,
        |visual| visual.debug_bounds("editor-rename-prompt").is_some(),
        "ordinary rename field",
    );
    // Copy observes the focused field's actual value without exposing rename form internals.
    visual.simulate_input("changedd");
    // Editing keys in the name field must not reach the semantic source input handler.
    visual.simulate_keystrokes("backspace");
    visual.simulate_keystrokes("ctrl-a ctrl-c");
    assert_eq!(
        visual.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text())),
        Some("changed".into()),
        "native input must replace the selected old name"
    );
    assert_text(&app, source, visual);
    visual.simulate_keystrokes("ctrl-x ctrl-v");
    assert_text(&app, source, visual);
    // Delete and Undo/Redo are native field actions too; none may reach the source's history.
    visual.simulate_keystrokes("left delete ctrl-z ctrl-y ctrl-a ctrl-c");
    assert_eq!(
        visual.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text())),
        Some("change".into())
    );
    assert_text(&app, source, visual);
    visual.simulate_keystrokes("ctrl-z ctrl-a ctrl-c");
    assert_eq!(
        visual.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text())),
        Some("changed".into())
    );
    visual.simulate_keystrokes("enter");
    pump(visual, 150);
    assert_eq!(
        editing_fixture::count(&log, "textDocument/rename"),
        1,
        "Enter must dispatch one actual request; wire={}",
        std::fs::read_to_string(&log).unwrap()
    );
    await_text(
        &app,
        &source.replace("item", "changed"),
        visual,
        "confirmed public rename",
    );
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    visual.simulate_keystrokes("ctrl-y");
    assert_text(&app, &source.replace("item", "changed"), visual);
}

/// Queue ordering preserves fast typing through native history, saving, settings and provider retirement.
#[gpui::test]
#[ignore = "build javascript.zip for the approved private Node runtime"]
fn delayed_linked_service_preserves_first_input_history_save_and_retirement(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sample.linked");
    let source = "<!-- 中文🙂 -->\n<item>内容🙂</item>\n";
    std::fs::write(&path, source).unwrap();
    let package = editing_fixture::package(
        "slow-language",
        "fixture",
        true,
        true,
        &directory.path().join("wire.jsonl"),
        150,
        "slow",
    );
    let (app, visual, mut manager) = shell(cx, directory.path(), vec![package]);
    open(&app, &path, visual);
    visual.update(|_, cx| {
        let server = &app.read(cx).language_servers["fixture"];
        let result = server.prepare_until_ready();
        assert!(
            result.is_ok(),
            "startup={result:?}; diagnostic={:?}; log={:?}",
            server.recovery_status(),
            manager.runtime_logs().records("slow-language")
        );
    });
    let start = source.find("<item>").unwrap() + 2;
    assert_eq!(
        crate::language::providers::language_for_path(&path).as_deref(),
        Some("fixture"),
        "fixture recognition must be published"
    );
    assert!(
        visual.update(|_, cx| app.read(cx).language_edits.bridge.is_some()),
        "native linked input must be bound"
    );
    let changed = source.replace("item", "iXtem");
    let one = source.replacen("item", "iXtem", 1);
    // Several commands enter before any new-caret pair response can finish.
    immediate_caret(&app, start, visual);
    visual.simulate_input("X");
    visual.simulate_keystrokes("ctrl-z ctrl-y");
    pump(visual, 750);
    assert_eq!(
        text(&app, visual),
        changed,
        "fast input Undo then Redo; wire={}",
        std::fs::read_to_string(directory.path().join("wire.jsonl")).unwrap()
    );
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    let close = source.find("</item>").unwrap() + 3;
    // Paste and deletion share the first waiting command's actual native cursor and paired history.
    immediate_caret(&app, start, visual);
    visual.update(|_, cx| cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("Y".into())));
    visual.simulate_input("X");
    visual.simulate_keystrokes("ctrl-v backspace");
    await_text(
        &app,
        &changed,
        visual,
        "first input then queued paste and deletion",
    );
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, &source.replace("item", "iXYtem"), visual);
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, &changed, visual);
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    // Cancelling during slow prepare cannot reopen a field after its reply arrives.
    immediate_caret(&app, start, visual);
    let prepares = editing_fixture::count(
        &directory.path().join("wire.jsonl"),
        "textDocument/prepareRename",
    );
    visual.simulate_keystrokes("f2");
    await_condition(
        visual,
        |_| {
            editing_fixture::count(
                &directory.path().join("wire.jsonl"),
                "textDocument/prepareRename",
            ) > prepares
        },
        "actual prepare request before cancel",
    );
    visual.simulate_keystrokes("escape");
    pump(visual, 350);
    assert!(visual.debug_bounds("editor-rename-prompt").is_none());
    assert_text(&app, source, visual);
    immediate_caret(&app, close, visual);
    let bridge = visual.update(|_, cx| app.read(cx).language_edits.bridge.clone().unwrap());
    visual.update(|window, cx| {
        bridge.update(cx, |bridge, cx| {
            bridge.replace_and_mark_text_in_range(None, "拼", Some(0..1), window, cx)
        })
    });
    visual.update(|window, cx| {
        bridge.update(cx, |bridge, cx| {
            bridge.replace_text_in_range(None, "标签", window, cx)
        })
    });
    await_text(
        &app,
        &source.replace("item", "i标签tem"),
        visual,
        "fast IME commit",
    );
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    // Switching tabs drains entered characters and queued Base history before replacing the focus path.
    let other = directory.path().join("other.txt");
    std::fs::write(&other, "other document").unwrap();
    immediate_caret(&app, start, visual);
    visual.simulate_input("X");
    let prepare_calls = editing_fixture::count(
        &directory.path().join("wire.jsonl"),
        "textDocument/prepareRename",
    );
    visual.simulate_keystrokes("ctrl-z ctrl-y");
    visual.simulate_keystrokes("ctrl-s shift-alt-f f2");
    open(&app, &other, visual);
    pump(visual, 350);
    assert_text(&app, "other document", visual);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        source,
        "waiting Save cannot follow a tab switch"
    );
    assert_eq!(
        editing_fixture::count(
            &directory.path().join("wire.jsonl"),
            "textDocument/formatting"
        ),
        0
    );
    assert_eq!(
        editing_fixture::count(
            &directory.path().join("wire.jsonl"),
            "textDocument/prepareRename"
        ),
        prepare_calls
    );
    assert!(visual.debug_bounds("editor-rename-prompt").is_none());
    open(&app, &path, visual);
    assert_text(&app, &one, visual);
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    visual.simulate_keystrokes("ctrl-s");
    await_condition(
        visual,
        |_| std::fs::read_to_string(&path).unwrap() == source,
        "clean document before close",
    );
    immediate_caret(&app, start, visual);
    visual.simulate_input("X");
    assert!(
        visual.update(|_, cx| app
            .read(cx)
            .language_edits
            .bridge
            .as_ref()
            .unwrap()
            .read(cx)
            .has_pending()),
        "close scenario must enter during semantic wait"
    );
    // Native tabs keep canonical identity, which uses a Windows verbatim prefix after open_file.
    let native_path = visual.update(|_, cx| app.read(cx).active_path.clone().unwrap());
    visual.update(|window, cx| {
        app.update(cx, |app, cx| app.close_tab(native_path.clone(), window, cx))
    });
    await_text(
        &app,
        &one,
        visual,
        "closing pending input preserves dirty document",
    );
    assert!(visual.update(|_, cx| {
        app.read(cx)
            .tabs
            .iter()
            .any(|tab| tab.path() == native_path && tab.is_dirty())
    }));
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    // A selected replacement cannot allocate another input queue ahead of the retired one.
    let replacement = editing_fixture::package(
        "replacement-language",
        "fixture",
        true,
        true,
        &directory.path().join("replacement.jsonl"),
        150,
        "replacement",
    );
    manager
        .install(&replacement, replacement.manifest.permissions.clone())
        .unwrap();
    crate::extensions::lsp_tests::publish(&app, &mut manager, visual);
    immediate_caret(&app, start, visual);
    visual.simulate_input("X");
    crate::language::providers::choose(
        protocol::settings::Scope::User,
        "lsp:fixture",
        Some("replacement-language/analysis"),
    )
    .unwrap();
    visual.update(|_, cx| app.update(cx, |app, cx| app.sync_dynamic_language_servers(cx)));
    visual.simulate_input("Y");
    await_text(
        &app,
        &source.replacen("item", "iXYtem", 1),
        visual,
        "ordered input across provider switch",
    );
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    visual.simulate_keystrokes("ctrl-y");
    assert_text(&app, &source.replacen("item", "iXYtem", 1), visual);
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    crate::language::providers::choose(
        protocol::settings::Scope::User,
        "lsp:fixture",
        Some("slow-language/analysis"),
    )
    .unwrap();
    visual.update(|_, cx| app.update(cx, |app, cx| app.sync_dynamic_language_servers(cx)));
    visual.update(|_, cx| {
        app.read(cx).language_servers["fixture"]
            .prepare_until_ready()
            .unwrap()
    });
    immediate_caret(&app, start, visual);
    visual.simulate_input("X");
    visual.simulate_keystrokes("ctrl-s");
    await_condition(
        visual,
        |_| std::fs::read_to_string(&path).unwrap() == changed,
        "save after fast native input",
    );
    assert_text(&app, &changed, visual);
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    visual.simulate_keystrokes("ctrl-s");
    await_condition(
        visual,
        |_| std::fs::read_to_string(&path).unwrap() == source,
        "restore disk after Undo",
    );
    // Turning off semantics preserves characters already entered into the original native document.
    immediate_caret(&app, start, visual);
    visual.simulate_input("X");
    crate::language::providers::set_editing_preference("fixture", false, Some(false)).unwrap();
    visual.update(|_, cx| app.update(cx, |app, cx| app.sync_linked_input(cx)));
    visual.simulate_input("Y");
    await_text(
        &app,
        &source.replacen("item", "iXYtem", 1),
        visual,
        "ordered pending input after linked disabled",
    );
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    visual.simulate_keystrokes("ctrl-y");
    assert_text(&app, &source.replacen("item", "iXYtem", 1), visual);
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    crate::language::providers::set_editing_preference("fixture", false, None).unwrap();
    visual.update(|_, cx| app.update(cx, |app, cx| app.sync_linked_input(cx)));
    // User-wide policy can be overridden by one language without freezing the other editing preference.
    crate::language::providers::set_editing_preference("*", false, Some(false)).unwrap();
    immediate_caret(&app, start, visual);
    visual.simulate_input("X");
    assert_text(&app, &one, visual);
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    crate::language::providers::set_editing_preference("fixture", false, Some(true)).unwrap();
    immediate_caret(&app, start, visual);
    visual.simulate_input("X");
    await_text(
        &app,
        &changed,
        visual,
        "language linked override of global policy",
    );
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    crate::language::providers::set_editing_preference("fixture", false, None).unwrap();
    crate::language::providers::set_editing_preference("*", false, None).unwrap();
    immediate_caret(&app, start, visual);
    visual.simulate_input("X");
    visual.simulate_keystrokes("right");
    await_text(&app, &one, visual, "pending input before caret movement");
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    association_switch_preserves_pending_input(&app, &mut manager, &path, source, start, visual);
    immediate_caret(&app, start, visual);
    visual.simulate_input("X");
    manager.disable("slow-language").unwrap();
    crate::extensions::lsp_tests::publish(&app, &mut manager, visual);
    visual.simulate_input("Y");
    await_text(
        &app,
        &source.replacen("item", "iXYtem", 1),
        visual,
        "ordered pending input after provider disable",
    );
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    visual.simulate_keystrokes("ctrl-y");
    assert_text(&app, &source.replacen("item", "iXYtem", 1), visual);
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
}

/// Revoking semantic leases during recognition changes must retain entered native commands and saving.
fn association_switch_preserves_pending_input(
    app: &Entity<EditorApp>,
    manager: &mut Manager,
    path: &Path,
    source: &str,
    start: usize,
    visual: &mut VisualTestContext,
) {
    let recognition =
        crate::extensions::language_tests::packages::language_package("other-recognizer");
    manager
        .install(&recognition, recognition.manifest.permissions.clone())
        .unwrap();
    crate::extensions::lsp_tests::publish(app, manager, visual);
    immediate_caret(app, start, visual);
    visual.simulate_input("X");
    assert!(visual.update(|_, cx| {
        app.read(cx)
            .language_edits
            .bridge
            .as_ref()
            .unwrap()
            .read(cx)
            .has_pending()
    }));
    crate::language::providers::associate_extension("linked", Some("novel")).unwrap();
    visual.update(|_, cx| app.update(cx, |app, cx| app.sync_dynamic_languages(cx)));
    visual.simulate_input("Y");
    visual.simulate_keystrokes("ctrl-s");
    let entered = source.replacen("item", "iXYtem", 1);
    await_text(
        app,
        &entered,
        visual,
        "recognition changes preserve ordered native XY",
    );
    await_condition(
        visual,
        |_| std::fs::read_to_string(path).unwrap() == entered,
        "recognition-change save owns the same native document",
    );
    visual.simulate_keystrokes("ctrl-z");
    assert_text(app, source, visual);
    visual.simulate_keystrokes("ctrl-y");
    assert_text(app, &entered, visual);
    visual.simulate_keystrokes("ctrl-z");
    assert_text(app, source, visual);
    crate::language::providers::associate_extension("linked", None).unwrap();
    visual.update(|_, cx| app.update(cx, |app, cx| app.sync_dynamic_languages(cx)));
    visual.simulate_keystrokes("ctrl-s");
    await_condition(
        visual,
        |_| std::fs::read_to_string(path).unwrap() == source,
        "recognition fixture restores saved native source",
    );
}
