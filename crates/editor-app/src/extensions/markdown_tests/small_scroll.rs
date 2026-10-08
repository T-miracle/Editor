//! Small semantic gestures retain native source progress and manual preview ownership.
use super::*;
use harness::NativeMarkdown;

/// A preview gesture during the publication delay outranks the earlier source edit, even after catch-up.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_preview_scroll_during_pending_edit_keeps_latest_owner(
    cx: &mut TestAppContext,
) {
    let source = "段落正文。\n\n".repeat(90);
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", &source)]);
    fixture.open("notes.md", ui);
    fixture.focus_editor(ui);
    ui.update(|window, cx| {
        window.dispatch_keystroke(gpui_kit::Keystroke::parse("中").unwrap(), cx);
    });
    // Keep the guest on the old revision until after the actual preview wheel has already moved it.
    wheel(ui, "plugin-ui-preview-scroll", -12.);
    let manual = ui.debug_bounds("plugin-ui-preview-body").unwrap().top();
    fixture.settle(ui);
    fixture.settle(ui);
    let after = ui.debug_bounds("plugin-ui-preview-body").unwrap().top();
    assert!(
        (after - manual).abs() <= px(0.2),
        "the earlier source edit cannot reclaim a later preview wheel: {manual:?} -> {after:?}"
    );
}

/// Even a one-pixel preview gesture owns scrolling after the source was the previous driver.
/// Observe actual source pixels and rendered paragraph bounds through the delivered package.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_small_preview_scroll_moves_source_without_bouncing(cx: &mut TestAppContext) {
    let source = "![大图](large.png)\n\n".to_owned()
        + &(0..100)
            .map(|index| format!("段落 {index:03}：同步阅读对应内容。\n\n"))
            .collect::<String>();
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", &source)]);
    std::fs::write(
        fixture.directory.path().join("large.png"),
        super::image_preview::png(400, 1000),
    )
    .unwrap();
    fixture.open("notes.md", ui);
    super::image_preview::complete(&mut fixture, ui);
    let position = ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).input_bounds().center());
    ui.simulate_event(gpui_kit::ScrollWheelEvent {
        position,
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(0.))),
        ..Default::default()
    });
    ui.run_until_parked();
    fixture.settle(ui);
    let selector = "plugin-ui-preview-body";
    for delta in [-1., -2., -5.] {
        let before = ui.debug_bounds(selector).unwrap().top();
        let source_before = ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).scroll_offset());
        wheel(ui, "plugin-ui-preview-scroll", delta);
        let manual = ui.debug_bounds(selector).unwrap().top();
        assert!(manual < before, "native wheel must translate the preview");
        fixture.settle(ui);
        fixture.settle(ui);
        let after = ui.debug_bounds(selector).unwrap().top();
        let source_after = ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).scroll_offset());
        assert!(
            source_after != source_before,
            "small preview delta {delta} must move source pixels: {source_before:?} -> {source_after:?}"
        );
        assert!(
            (after - manual).abs() <= px(0.2),
            "manual preview position must survive delayed source receipts: {manual:?} -> {after:?}"
        );
    }
}

/// Wrapped table cells also turn small preview deltas into source subpixels, without returning to the old anchor.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_small_table_scroll_retains_manual_preview_position(cx: &mut TestAppContext) {
    let cell = "正文与`行内代码`需要在窄表格中换行。".repeat(55);
    let source = format!("| 内容 | 说明 |\n| --- | --- |\n| {cell} | 说明 |\n\n")
        + &(0..80)
            .map(|index| format!("段落 {index}。\n\n"))
            .collect::<String>();
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("table.md", &source)]);
    fixture.open("table.md", ui);
    wheel(ui, "plugin-ui-preview-scroll", -60.);
    fixture.settle(ui);
    for delta in [-1., -2., -5.] {
        let before = ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).scroll_offset());
        wheel(ui, "plugin-ui-preview-scroll", delta);
        let manual = ui.debug_bounds("plugin-ui-preview-body").unwrap().top();
        fixture.settle(ui);
        fixture.settle(ui);
        let after = ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).scroll_offset());
        assert_ne!(
            after, before,
            "wrapped table delta {delta} must move source pixels"
        );
        assert!(
            (ui.debug_bounds("plugin-ui-preview-body").unwrap().top() - manual).abs() <= px(0.2),
            "delayed source receipts must not pull the table back"
        );
    }
}

/// Real viewport wheel input leaves native Base in charge of clamping and event dispatch.
fn wheel(ui: &mut gpui_kit::VisualTestContext, selector: &'static str, delta: f32) {
    let position = ui
        .debug_bounds(selector)
        .expect("visible native viewport")
        .center();
    ui.simulate_event(gpui_kit::ScrollWheelEvent {
        position,
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(delta))),
        ..Default::default()
    });
    ui.run_until_parked();
}
