//! Startup publication races exercise the actual guest and native wheel through public notifications.
use super::*;
use harness::NativeMarkdown;

/// Manual preview input during first publication must survive delayed source layout and locate replies.
/// Drive the real guest/Manager one publication at a time so startup cannot settle before the wheel.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_first_preview_scroll_does_not_return_to_top(cx: &mut TestAppContext) {
    use super::super::composable_tests::{publish, pump};
    let source = (0..100)
        .map(|index| format!("段落 {index:03}：首次打开后的预览滚动应保持当前位置。\n\n"))
        .collect::<String>();
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", &source)]);
    let path = fixture.directory.path().join("notes.md");
    ui.update(|window, cx| {
        fixture
            .app
            .update(cx, |app, cx| app.open_file(path, window, cx))
    });
    ui.run_until_parked();
    for _ in 0..5 {
        pump(&mut fixture.manager, &fixture.app, ui);
        fixture.manager.poll();
        publish(
            &mut fixture.manager,
            &mut fixture.renderer,
            &fixture.app,
            ui,
        );
        if ui.debug_bounds("plugin-ui-preview-scroll").is_some() {
            break;
        }
    }
    let preview = ui
        .debug_bounds("plugin-ui-preview-scroll")
        .expect("first visible preview");
    ui.simulate_event(gpui_kit::ScrollWheelEvent {
        position: preview.center(),
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-900.))),
        ..Default::default()
    });
    ui.run_until_parked();
    let scrolled = ui.debug_bounds("plugin-ui-b-0-paragraph").unwrap();
    assert!(
        scrolled.bottom() < preview.top(),
        "wheel leaves the document beginning"
    );
    // Deliver the manual viewport but keep its reverse locate queued behind startup configuration.
    pump(&mut fixture.manager, &fixture.app, ui);
    fixture.manager.poll();
    // Native startup can finish publishing its locale after the first preview is already interactive.
    // Enter through the public manager notification instead of changing the guest's private driver.
    fixture
        .manager
        .event(
            "markdown",
            None,
            protocol::api::Notification::Theme(protocol::Environment {
                locale: "zh-CN".into(),
                workspace: fixture.directory.path().display().to_string(),
                ..Default::default()
            }),
        )
        .unwrap();
    publish(
        &mut fixture.manager,
        &mut fixture.renderer,
        &fixture.app,
        ui,
    );
    for _ in 0..8 {
        fixture.settle(ui);
    }
    let settled = ui.debug_bounds("plugin-ui-b-0-paragraph").unwrap();
    assert!(
        settled.bottom() < preview.top() - px(400.),
        "delayed startup synchronization must not undo manual preview scrolling: before={scrolled:?}, after={settled:?}, viewport={preview:?}"
    );
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        source
    );
}
