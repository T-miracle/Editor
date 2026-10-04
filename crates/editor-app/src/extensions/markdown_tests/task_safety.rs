//! Task requests retain their original source authority through native edits and plugin retirement.
use super::*;
use gpui_kit::EntityInputHandler as _;
use harness::NativeMarkdown;
use protocol::api::{EditorOperation, RequestUpdate};

/// Pause a real guest selection request before the production host queue enters its effect.
fn pending_task(
    fixture: &mut NativeMarkdown,
    ui: &mut gpui_kit::VisualTestContext,
) -> plugin_runtime::EditorRequest {
    let position = ui
        .debug_bounds("plugin-checkbox-marker-b-2-task")
        .unwrap()
        .center();
    ui.simulate_click(position, Default::default());
    ui.run_until_parked();
    super::super::composable_tests::pump(&mut fixture.manager, &fixture.app, ui);
    let request = fixture
        .manager
        .live
        .get_mut("markdown")
        .unwrap()
        .take_editor_requests()
        .pop()
        .unwrap();
    assert!(matches!(
        request.operation(),
        EditorOperation::ReadDocumentSelection { .. }
    ));
    request
}

/// Publish the original handle; no fixture manufactures a successful selection or an edited receipt.
fn execute(
    fixture: &mut NativeMarkdown,
    request: &plugin_runtime::EditorRequest,
    ui: &mut gpui_kit::VisualTestContext,
) {
    ui.update(|_, cx| {
        fixture
            .app
            .read(cx)
            .extensions
            .read(cx)
            .worker
            .state
            .lock()
            .unwrap()
            .editor_requests
            .push(("markdown".into(), request.clone()))
    });
    fixture.settle(ui);
}

/// Rapid native typing and Chinese composition invalidate earlier task offsets rather than overwrite text.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_task_requests_reject_rapid_edits_and_preserve_ime(cx: &mut TestAppContext) {
    let original = "- [ ] 原始任务\n";
    let (mut fixture, ui) =
        NativeMarkdown::mount(cx, &[("notes.md", original), ("compose.md", original)]);
    fixture.open("notes.md", ui);
    let old_ui_revision = fixture.manager.live["markdown"].views["preview"].revision;
    let request = pending_task(&mut fixture, ui);
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-home");
    ui.simulate_input("前言\n");
    ui.run_until_parked();
    execute(&mut fixture, &request, ui);
    assert!(matches!(
        request.status(),
        RequestUpdate::Completed { result: Err(_) } | RequestUpdate::Cancelled { .. }
    ));
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        format!("前言\n{original}")
    );
    assert!(ui.debug_bounds("plugin-checkbox-marker-b-9-task").is_some());
    let stale = fixture
        .manager
        .event(
            "markdown",
            Some("preview".into()),
            PluginEvent::Ui(protocol::ui::UiEvent {
                revision: old_ui_revision,
                node: "b-2-task".into(),
                action: protocol::ui::Action::Toggle(true),
            }),
        )
        .unwrap_err();
    assert!(
        matches!(stale.downcast_ref::<protocol::api::Failure>(), Some(error) if error.code == protocol::api::ErrorCode::StaleRevision)
    );

    fixture.open("compose.md", ui);
    let request = pending_task(&mut fixture, ui);
    fixture.focus_editor(ui);
    ui.update(|window, cx| {
        fixture
            .app
            .read(cx)
            .editor
            .clone()
            .update(cx, |editor, cx| {
                editor.set_selected_range(0..0, cx);
                editor.replace_and_mark_text_in_range(None, "拼", Some(0..1), window, cx);
            })
    });
    ui.run_until_parked();
    execute(&mut fixture, &request, ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        format!("拼{original}")
    );
    assert!(
        ui.update(|window, cx| fixture
            .app
            .read(cx)
            .editor
            .clone()
            .update(cx, |editor, cx| {
                editor.marked_text_range(window, cx).is_some()
            })),
        "a rejected task must not finish native Chinese composition"
    );
}

/// Switching/closing a source and withdrawing its plugin cancel admitted work before any text effect.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_task_requests_cannot_follow_tabs_close_or_disable(cx: &mut TestAppContext) {
    let first = "- [ ] 第一份\n";
    let second = "- [ ] 第二份\n";
    let (mut fixture, ui) =
        NativeMarkdown::mount(cx, &[("first.md", first), ("second.md", second)]);
    fixture.open("first.md", ui);
    let switched = pending_task(&mut fixture, ui);
    fixture.open("second.md", ui);
    execute(&mut fixture, &switched, ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        second
    );
    fixture.open("first.md", ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        first
    );
    let closed = pending_task(&mut fixture, ui);
    let path = fixture
        .directory
        .path()
        .join("first.md")
        .canonicalize()
        .unwrap();
    ui.update(|window, cx| {
        fixture
            .app
            .update(cx, |app, cx| app.close_tab(path, window, cx))
    });
    ui.run_until_parked();
    execute(&mut fixture, &closed, ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        second
    );
    let retired = pending_task(&mut fixture, ui);
    fixture.manager.disable("markdown").unwrap();
    execute(&mut fixture, &retired, ui);
    assert!(matches!(retired.status(), RequestUpdate::Cancelled { .. }));
    assert!(ui.debug_bounds("plugin-checkbox-marker-b-2-task").is_none());
    assert!(ui.debug_bounds("editor-preview-pane").is_none());
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        second
    );
    assert_eq!(
        std::fs::read_to_string(fixture.directory.path().join("first.md")).unwrap(),
        first
    );
}
