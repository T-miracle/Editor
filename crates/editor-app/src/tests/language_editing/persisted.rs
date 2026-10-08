//! Hand-edited host preferences distinguish invalid explicit formatters from verified provider withdrawal.
use super::*;
use crate::language::providers;
use serde_json::json;

/// Real packages, native commands, a settings reset and public removal exercise the stored choice boundary.
#[gpui::test]
#[ignore = "build javascript.zip for the approved private Node runtime used by formatter fixtures"]
fn persisted_formatter_typo_blocks_dispatch_and_native_reset_preserves_removal_rules(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.linked");
    let source = "raw 中文🙂\n";
    std::fs::write(&path, source).unwrap();
    let user_log = directory.path().join("user.jsonl");
    let project_log = directory.path().join("project.jsonl");
    let user = editing_fixture::package(
        "valid-user-format",
        "fixture",
        false,
        true,
        &user_log,
        0,
        "user-marker",
    );
    let project = editing_fixture::package(
        "valid-project-format",
        "fixture",
        false,
        true,
        &project_log,
        0,
        "project-marker",
    );
    let (app, visual, mut manager) = shell(cx, directory.path(), vec![user, project]);
    providers::choose(
        protocol::settings::Scope::User,
        "recognition:ext:linked",
        Some("valid-user-format/fixture"),
    )
    .unwrap();
    providers::choose(
        protocol::settings::Scope::User,
        "formatter:fixture",
        Some("valid-user-format/format"),
    )
    .unwrap();
    providers::choose(
        protocol::settings::Scope::Project,
        "formatter:fixture",
        Some("valid-project-format/format"),
    )
    .unwrap();
    providers::set_editing_preference("fixture", true, Some(true)).unwrap();
    visual.update(|_, cx| app.update(cx, |app, cx| app.sync_dynamic_languages(cx)));
    open(&app, &path, visual);
    visual
        .update(|_, cx| app.read(cx).language_edits.formatters["fixture"].clone())
        .prepare_until_ready()
        .unwrap();
    immediate_caret(&app, source.len(), visual);
    visual.simulate_input("X");
    let entered = format!("{source}X");
    assert_text(&app, &entered, visual);
    assert_invalid_project_dispatch(
        &app,
        &mut manager,
        directory.path(),
        &path,
        &entered,
        [&user_log, &project_log],
        visual,
    );

    // Open the real separate settings window. Subsequent contexts use the public window handle,
    // so the same EditorApp and document return after the native Project-scope reset.
    let main_window = visual.update(|window, _| window.window_handle());
    let settings = visual.debug_bounds("settings-trigger").unwrap();
    visual.simulate_click(settings.center(), Default::default());
    let dialog_window = visual
        .update(|_, cx| cx.windows())
        .into_iter()
        .find(|handle| *handle != main_window)
        .unwrap();
    let dialog_cx = VisualTestContext::from_window(dialog_window, cx).into_mut();
    clear_invalid_project_with_settings(dialog_cx);
    let visual = VisualTestContext::from_window(main_window, cx).into_mut();
    // Escape removed the modal; observe its lifecycle from the still-open editor window.
    visual.update(|_, cx| assert!(app.read(cx).dialog_window.is_none()));
    assert_normal_withdrawal(
        &app,
        &mut manager,
        directory.path(),
        &entered,
        [&user_log, &project_log],
        visual,
    );
}

/// A valid JSON edit must not fall through to remembered providers in manual or opt-in native saving.
fn assert_invalid_project_dispatch(
    app: &Entity<EditorApp>,
    manager: &mut Manager,
    workspace: &Path,
    path: &Path,
    entered: &str,
    logs: [&Path; 2],
    visual: &mut VisualTestContext,
) {
    let file = manager.root().join("language-providers.json");
    let mut stored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    let workspace_key = workspace.canonicalize().unwrap().display().to_string();
    assert_eq!(
        stored["user"]["formatter:fixture"],
        json!("valid-user-format/format")
    );
    assert_eq!(
        stored["projects"][&workspace_key]["formatter:fixture"],
        json!("valid-project-format/format")
    );
    stored["projects"][&workspace_key]["formatter:fixture"] = json!("typo-never-installed/format");
    std::fs::write(&file, serde_json::to_vec(&stored).unwrap()).unwrap();
    reload_preferences(app, manager, workspace, visual);
    visual.simulate_keystrokes("shift-alt-f");
    pump(visual, 120);
    visual.simulate_keystrokes("ctrl-s");
    pump(visual, 120);
    for log in logs {
        assert_eq!(
            editing_fixture::count(log, "textDocument/formatting"),
            0,
            "unknown explicit ID must never dispatch a fallback formatter"
        );
    }
    assert_text(app, entered, visual);
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        entered,
        "invalid formatting preference still saves ordinary native text"
    );
    assert!(
        visual.update(|_, cx| app.read(cx).status.contains("typo-never-installed/format")),
        "native status must retain the configuration error after Save"
    );
    let row = providers::rows()
        .into_iter()
        .find(|row| row.key == "formatter:fixture")
        .unwrap();
    assert_eq!(
        row.source, "project",
        "the invalid explicit source cannot be relabeled automatic"
    );
    assert!(row.selected.is_none());
}

/// Public workspace configuration reloads the same store, without a test-only registry mutation API.
fn reload_preferences(
    app: &Entity<EditorApp>,
    manager: &mut Manager,
    workspace: &Path,
    visual: &mut VisualTestContext,
) {
    let other = tempfile::tempdir().unwrap();
    providers::configure(manager.root(), other.path());
    providers::configure(manager.root(), workspace);
    crate::extensions::lsp_tests::publish(app, manager, visual);
}

/// The language settings page shows the erroneous Project value and lets the user clear only that layer.
fn clear_invalid_project_with_settings(visual: &mut VisualTestContext) {
    visual.simulate_resize(size(px(1200.), px(1000.)));
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let languages = visual.debug_bounds("settings-nav-languages").unwrap();
    visual.simulate_click(languages.center(), Default::default());
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        visual
            .debug_bounds("provider-invalid-project-formatter:fixture")
            .is_some(),
        "unknown ID and Project error must have a visible settings row"
    );
    visual.update(|window, cx| {
        apply_theme(builtin_theme(true), cx);
        window.draw(cx).clear(cx);
    });
    assert!(
        visual
            .debug_bounds("provider-invalid-project-formatter:fixture")
            .is_some()
    );
    let scope = visual.debug_bounds("provider-scope").unwrap();
    let mut point = scope.center();
    point.x = scope.left() + scope.size.width * 0.75;
    visual.simulate_click(point, Default::default());
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let reset = visual
        .debug_bounds("provider-reset-formatter:fixture")
        .unwrap();
    visual.simulate_click(reset.center(), Default::default());
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(
        providers::formatters()["fixture"].as_deref(),
        Some("valid-user-format/format")
    );
    assert!(
        visual
            .debug_bounds("provider-invalid-project-formatter:fixture")
            .is_none()
    );
    assert!(
        visual
            .debug_bounds("provider-source-user-formatter:fixture")
            .is_some()
    );
    visual.simulate_keystrokes("escape");
}

/// Verified removed choices follow sole/multiple rules across repeat refresh and store reload.
fn assert_normal_withdrawal(
    app: &Entity<EditorApp>,
    manager: &mut Manager,
    workspace: &Path,
    source: &str,
    logs: [&Path; 2],
    visual: &mut VisualTestContext,
) {
    visual.update(|window, cx| {
        window.activate_window();
        window.draw(cx).clear(cx);
        app.read(cx)
            .editor
            .clone()
            .update(cx, |editor, cx| editor.focus(window, cx))
    });
    visual.simulate_keystrokes("shift-alt-f");
    await_text(
        app,
        &format!("/* user-marker */\n{source}"),
        visual,
        "user provider after native project reset",
    );
    assert_eq!(
        editing_fixture::count(logs[0], "textDocument/formatting"),
        1
    );
    visual.simulate_keystrokes("ctrl-z");
    assert_text(app, source, visual);
    manager.disable("valid-user-format").unwrap();
    crate::extensions::lsp_tests::publish(app, manager, visual);
    reload_preferences(app, manager, workspace, visual);
    assert_eq!(
        providers::formatters()["fixture"].as_deref(),
        Some("valid-project-format/format")
    );
    let row = providers::rows()
        .into_iter()
        .find(|row| row.key == "formatter:fixture")
        .unwrap();
    assert_eq!(row.source, "automatic");
    visual.simulate_keystrokes("shift-alt-f");
    await_text(
        app,
        &format!("/* project-marker */\n{source}"),
        visual,
        "sole formatter after verified withdrawal",
    );
    assert_eq!(
        editing_fixture::count(logs[1], "textDocument/formatting"),
        1
    );
    visual.simulate_keystrokes("ctrl-z");
    assert_text(app, source, visual);

    let third_log = workspace.join("third.jsonl");
    let third = editing_fixture::package(
        "third-format",
        "fixture",
        false,
        false,
        &third_log,
        0,
        "third-marker",
    );
    manager
        .install(&third, third.manifest.permissions.clone())
        .unwrap();
    crate::extensions::lsp_tests::publish(app, manager, visual);
    assert_eq!(
        providers::formatters()["fixture"].as_deref(),
        Some("valid-project-format/format"),
        "new installations cannot steal the sole fallback"
    );
    manager.enable("valid-user-format").unwrap();
    crate::extensions::lsp_tests::publish(app, manager, visual);
    providers::choose(
        protocol::settings::Scope::User,
        "formatter:fixture",
        Some("valid-user-format/format"),
    )
    .unwrap();
    manager.uninstall("valid-user-format", false).unwrap();
    crate::extensions::lsp_tests::publish(app, manager, visual);
    reload_preferences(app, manager, workspace, visual);
    assert!(
        providers::formatters()["fixture"].is_none(),
        "two remaining candidates require an explicit choice"
    );
    let row = providers::rows()
        .into_iter()
        .find(|row| row.key == "formatter:fixture")
        .unwrap();
    assert_eq!(row.candidates.len(), 2);
    assert_eq!(
        row.source, "automatic",
        "normal removal is not an invalid explicit setting"
    );
    visual.simulate_keystrokes("shift-alt-f");
    pump(visual, 100);
    assert_eq!(
        editing_fixture::count(logs[1], "textDocument/formatting"),
        1
    );
    assert_eq!(
        editing_fixture::count(&third_log, "textDocument/formatting"),
        0
    );
}
