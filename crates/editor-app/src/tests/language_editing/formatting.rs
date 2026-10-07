//! Independent installed formatters prove actual routing, saving and whole-proposal lifetime guards.
use super::*;

/// A separate formatter changes output once without stealing grammar or primary analysis.
#[gpui::test]
#[ignore = "build javascript.zip for standard formatting and its approved private Node runtime"]
fn installed_javascript_formatter_can_be_replaced_without_changing_analysis(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sample.js");
    let source = "const answer={value:1};\n";
    std::fs::write(&path, source).unwrap();
    let javascript = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/javascript.zip"),
    )
    .unwrap();
    let primary = editing_fixture::package(
        "separate-analysis",
        "javascript",
        true,
        false,
        &directory.path().join("analysis.jsonl"),
        0,
        "analysis",
    );
    let (app, visual, mut manager) = shell(cx, directory.path(), vec![javascript, primary]);
    open(&app, &path, visual);
    let main = visual.update(|_, cx| app.read(cx).language_servers["javascript"].clone());
    main.prepare_until_ready().unwrap();
    let grammars = crate::language::providers::grammars();
    visual.simulate_keystrokes("shift-alt-f");
    await_condition(
        visual,
        |visual| text(&app, visual).contains("const answer = { value: 1 };"),
        "standard JavaScript formatting",
    );
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    let alt_log = directory.path().join("alternate.jsonl");
    let project_log = directory.path().join("project.jsonl");
    for (id, log) in [
        ("alternate-format", &alt_log),
        ("project-format", &project_log),
    ] {
        let package = editing_fixture::package(id, "javascript", false, false, log, 150, id);
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
        crate::extensions::lsp_tests::publish(&app, &mut manager, visual);
    }
    assert_eq!(
        crate::language::providers::formatters()["javascript"].as_deref(),
        Some("javascript/format"),
        "new installs preserve the adopted standard provider"
    );
    crate::language::providers::choose(
        protocol::settings::Scope::User,
        "formatter:javascript",
        Some("alternate-format/format"),
    )
    .unwrap();
    visual.update(|_, cx| app.update(cx, |app, cx| app.sync_dynamic_language_servers(cx)));
    visual.simulate_keystrokes("shift-alt-f");
    let alternate = format!("/* alternate-format */\n{source}");
    await_text(&app, &alternate, visual, "different plugin formatter");
    assert_eq!(
        editing_fixture::count(&alt_log, "textDocument/formatting"),
        1
    );
    assert_eq!(
        editing_fixture::count(&project_log, "textDocument/formatting"),
        0
    );
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    assert_eq!(crate::language::providers::grammars(), grammars);
    assert!(
        visual.update(|_, cx| Arc::ptr_eq(&app.read(cx).language_servers["javascript"], &main))
    );
    let uri = crate::language::navigation::file_uri(&path).unwrap();
    let completion = main
        .completions(uri.clone(), source.into(), lsp_types::Position::new(0, 5))
        .unwrap();
    assert!(format!("{completion:?}").contains("analysis-kept"));
    recognition_retires_both_roles(&app, &mut manager, &path, &alt_log, visual);
    assert!(
        crate::language::providers::choose(
            protocol::settings::Scope::User,
            "formatter:javascript",
            Some("missing/format")
        )
        .is_err()
    );
    assert_eq!(
        crate::language::providers::formatters()["javascript"].as_deref(),
        Some("alternate-format/format")
    );
    crate::language::providers::choose(
        protocol::settings::Scope::Project,
        "formatter:javascript",
        Some("project-format/format"),
    )
    .unwrap();
    visual.update(|_, cx| app.update(cx, |app, cx| app.sync_dynamic_language_servers(cx)));
    visual.simulate_keystrokes("ctrl-s");
    await_condition(
        visual,
        |_| std::fs::read_to_string(&path).unwrap() == source,
        "ordinary save",
    );
    assert_eq!(
        editing_fixture::count(&project_log, "textDocument/formatting"),
        0
    );
    crate::language::providers::set_editing_preference("javascript", true, Some(true)).unwrap();
    visual.simulate_keystrokes("ctrl-s");
    let project = format!("/* project-format */\n{source}");
    await_condition(
        visual,
        |_| std::fs::read_to_string(&path).unwrap() == project,
        "opt-in save formatting",
    );
    assert_text(&app, &project, visual);
    assert_eq!(
        editing_fixture::count(&project_log, "textDocument/formatting"),
        1
    );
    assert_eq!(
        editing_fixture::count(&alt_log, "textDocument/formatting"),
        1
    );
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    crate::language::providers::set_editing_preference("javascript", true, None).unwrap();
    visual.simulate_keystrokes("ctrl-s");
    await_condition(
        visual,
        |_| std::fs::read_to_string(&path).unwrap() == source,
        "save clean source before lifetime cases",
    );
    // A late formatting proposal cannot overwrite native typing or a closed/reopened incarnation.
    immediate_caret(&app, source.len(), visual);
    let requests = editing_fixture::count(&project_log, "textDocument/formatting");
    // simulate_keystrokes drains the scheduler. Dispatch both native events before that drain instead.
    visual.update(|window, cx| {
        window.dispatch_keystroke(gpui_kit::Keystroke::parse("shift-alt-f").unwrap(), cx);
        window.dispatch_keystroke(gpui_kit::Keystroke::parse("X").unwrap(), cx);
    });
    pump(visual, 350);
    assert_text(&app, &format!("{source}X"), visual);
    assert_eq!(
        editing_fixture::count(&project_log, "textDocument/formatting"),
        requests + 1
    );
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    // Undo restores bytes but still marks the native session dirty; save before testing a real close.
    visual.simulate_keystrokes("ctrl-s");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
    let previous = visual.update(|_, cx| app.read(cx).editor.clone());
    visual.update(|window, cx| {
        window.dispatch_keystroke(gpui_kit::Keystroke::parse("shift-alt-f").unwrap(), cx);
        app.update(cx, |app, cx| {
            let native_path = app.active_path.clone().unwrap();
            app.close_tab(native_path, window, cx);
            app.open_file(path.clone(), window, cx);
        })
    });
    assert!(
        visual.update(|_, cx| app.read(cx).editor != previous),
        "reopen must allocate another actual native incarnation"
    );
    pump(visual, 350);
    assert_text(&app, source, visual);
    visual.update(|window, cx| {
        window.dispatch_keystroke(gpui_kit::Keystroke::parse("shift-alt-f").unwrap(), cx);
    });
    manager.disable("project-format").unwrap();
    crate::extensions::lsp_tests::publish(&app, &mut manager, visual);
    pump(visual, 350);
    assert_text(&app, source, visual);
    manager.uninstall("project-format", false).unwrap();
    manager.uninstall("alternate-format", false).unwrap();
    crate::extensions::lsp_tests::publish(&app, &mut manager, visual);
    visual.simulate_keystrokes("shift-alt-f");
    await_condition(
        visual,
        |visual| text(&app, visual).contains("const answer = { value: 1 };"),
        "sole standard formatter fallback",
    );
    visual.simulate_keystrokes("ctrl-z");
    assert_text(&app, source, visual);
    let invalid_log = directory.path().join("invalid.jsonl");
    let invalid = editing_fixture::package(
        "invalid-format",
        "javascript",
        false,
        false,
        &invalid_log,
        0,
        "invalid",
    );
    manager
        .install(&invalid, invalid.manifest.permissions.clone())
        .unwrap();
    crate::extensions::lsp_tests::publish(&app, &mut manager, visual);
    crate::language::providers::choose(
        protocol::settings::Scope::Project,
        "formatter:javascript",
        Some("invalid-format/format"),
    )
    .unwrap();
    visual.update(|_, cx| app.update(cx, |app, cx| app.sync_dynamic_language_servers(cx)));
    manager
        .update_setting(
            "invalid-format",
            protocol::settings::Scope::Project,
            "tool",
            Some(serde_json::json!(directory.path().join("missing.exe"))),
        )
        .unwrap();
    crate::extensions::lsp_tests::publish(&app, &mut manager, visual);
    visual.simulate_keystrokes("shift-alt-f");
    pump(visual, 80);
    assert_text(&app, source, visual);
    assert_eq!(
        editing_fixture::count(&invalid_log, "textDocument/formatting"),
        0,
        "explicit missing executable must not silently run another formatter"
    );
    manager
        .update_setting(
            "invalid-format",
            protocol::settings::Scope::Project,
            "tool",
            None,
        )
        .unwrap();
    crate::extensions::lsp_tests::publish(&app, &mut manager, visual);
    visual.simulate_keystrokes("shift-alt-f");
    await_condition(
        visual,
        |_| editing_fixture::count(&invalid_log, "textDocument/formatting") == 1,
        "invalid full proposal",
    );
    pump(visual, 80);
    assert_text(&app, source, visual);
}

/// Reassociation revokes both independent document leases even when their services stay selected.
fn recognition_retires_both_roles(
    app: &Entity<EditorApp>,
    manager: &mut Manager,
    path: &Path,
    format_log: &Path,
    visual: &mut VisualTestContext,
) {
    let package = crate::extensions::language_tests::packages::language_package("other-recognizer");
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    crate::extensions::lsp_tests::publish(app, manager, visual);
    let uri = crate::language::navigation::file_uri(path).unwrap();
    let (main, formatter) = visual.update(|_, cx| {
        let app = app.read(cx);
        (
            app.language_servers["javascript"].clone(),
            app.language_edits.formatters["javascript"].clone(),
        )
    });
    assert!(!Arc::ptr_eq(&main, &formatter));
    let main_document = main
        .document(&uri)
        .expect("actual analysis opened this document");
    let format_document = formatter
        .document(&uri)
        .expect("actual formatting opened this document");
    let analysis_log = path.parent().unwrap().join("analysis.jsonl");
    let analysis_closes = editing_fixture::count(&analysis_log, "textDocument/didClose");
    let format_closes = editing_fixture::count(format_log, "textDocument/didClose");
    crate::language::providers::associate_extension("js", Some("novel")).unwrap();
    visual.update(|_, cx| app.update(cx, |app, cx| app.sync_dynamic_languages(cx)));
    assert_eq!(crate::editor::language_for_path(path), "novel");
    assert!(
        !main_document.is_active(),
        "old analysis lease retires synchronously"
    );
    assert!(
        !format_document.is_active(),
        "old independent formatter lease retires synchronously"
    );
    await_condition(
        visual,
        |_| {
            editing_fixture::count(&analysis_log, "textDocument/didClose") == analysis_closes + 1
                && editing_fixture::count(format_log, "textDocument/didClose") == format_closes + 1
        },
        "both old recognition roles deliver exactly one didClose",
    );
    assert!(
        main.is_active() && formatter.is_active(),
        "other JavaScript files retain both services"
    );
    crate::language::providers::associate_extension("js", None).unwrap();
    visual.update(|_, cx| app.update(cx, |app, cx| app.sync_dynamic_languages(cx)));
    assert_eq!(crate::editor::language_for_path(path), "javascript");
}
