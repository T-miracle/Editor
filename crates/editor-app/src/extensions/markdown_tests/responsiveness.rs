//! Real package regressions for modest documents, native wheel notches and typing latency.
use super::*;
use harness::NativeMarkdown;

/// A source navigation key must reclaim the preview after a wheel gesture and leave links clickable.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_source_home_reclaims_preview_and_link(cx: &mut TestAppContext) {
    let source = "[下一页](next.md)\n\n".to_owned()
        + &(0..70)
            .map(|i| format!("段落 {i} 正文。\n\n"))
            .collect::<String>();
    let (mut fixture, ui) =
        NativeMarkdown::mount(cx, &[("notes.md", &source), ("next.md", "目标\n")]);
    fixture.open("notes.md", ui);
    let position = ui
        .debug_bounds("plugin-ui-preview-scroll")
        .unwrap()
        .center();
    ui.simulate_event(gpui_kit::ScrollWheelEvent {
        position,
        delta: gpui_kit::ScrollDelta::Lines(gpui_kit::point(0., -12.)),
        ..Default::default()
    });
    ui.run_until_parked();
    fixture.settle(ui);
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-home");
    ui.run_until_parked();
    fixture.settle(ui);
    let link = ui.debug_bounds("plugin-ui-b-0-paragraph").unwrap();
    let pane = ui.debug_bounds("plugin-ui-preview-scroll").unwrap();
    let target = gpui_kit::point(link.left() + px(12.), link.center().y);
    assert!(
        pane.contains(&target),
        "source Home must reveal the link: {link:?}, {pane:?}"
    );
    ui.simulate_click(target, Default::default());
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        "目标\n"
    );
}

/// Formatting immediately after typing flushes the pending snapshot and still edits the accepted line.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_toolbar_flushes_coalesced_source(cx: &mut TestAppContext) {
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", "原文\n")]);
    fixture.open("notes.md", ui);
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-home");
    ui.update(|window, cx| {
        window.dispatch_keystroke(gpui_kit::Keystroke::parse("中").unwrap(), cx);
    });
    fixture.click("plugin-ui-format-heading", ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        "# 中原文\n"
    );
}

/// A few screens of ordinary prose must not run preview generation while only source is visible.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_source_only_typing_suspends_preview(cx: &mut TestAppContext) {
    let source = (0..70)
        .map(|i| format!("段落 {i}：普通文档输入与阅读。\n\n"))
        .collect::<String>();
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", &source)]);
    fixture.open("notes.md", ui);
    fixture.click("plugin-tool-markdown/preview/display-source", ui);
    let before = fixture.manager.live["markdown"].views["preview"]
        .root
        .clone();
    fixture.focus_editor(ui);

    // Time one native frame directly; simulate_input also drains background work and is not input latency.
    let mut frames = Vec::new();
    for text in ["中", "文", "输", "入", "测", "量"] {
        ui.update(|window, cx| {
            window.dispatch_keystroke(gpui_kit::Keystroke::parse(text).unwrap(), cx);
        });
        frames.push(ui.update(|window, cx| {
            let start = std::time::Instant::now();
            window.draw(cx).clear(cx);
            start.elapsed()
        }));
    }
    eprintln!("source-only native frame samples: {frames:?}");
    frames.sort();
    assert!(
        frames[3] < Duration::from_millis(40),
        "nested toolbar layout must not block every input frame: {frames:?}"
    );
    fixture.settle(ui);
    let after = &fixture.manager.live["markdown"].views["preview"].root;
    assert_eq!(
        *after, before,
        "hidden content must not be parsed or rebuilt; source authority stays fresh for toolbar commands"
    );
    fixture.click("plugin-tool-markdown/preview/display-split", ui);
    assert!(
        serde_json::to_string(&fixture.manager.live["markdown"].views["preview"].root)
            .unwrap()
            .contains("中文输入测量"),
        "showing the preview must catch up to the latest source"
    );
    // The same input priority must hold while visible blocks are actually drawn in split mode.
    frames.clear();
    for text in ["分", "栏", "输", "入", "测", "量"] {
        ui.update(|window, cx| {
            window.dispatch_keystroke(gpui_kit::Keystroke::parse(text).unwrap(), cx);
        });
        frames.push(ui.update(|window, cx| {
            let start = std::time::Instant::now();
            window.draw(cx).clear(cx);
            start.elapsed()
        }));
    }
    eprintln!("split native frame samples: {frames:?}");
    frames.sort();
    assert!(
        frames[3] < Duration::from_millis(40),
        "visible block layout must not stall input: {frames:?}"
    );
}

/// A discrete Windows wheel gesture is expressed in lines, not tiny synthetic pixel deltas.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_wheel_notch_does_not_bounce(cx: &mut TestAppContext) {
    let source = (0..70)
        .map(|i| format!("段落 {i}：普通文档输入与阅读。\n\n"))
        .collect::<String>();
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", &source)]);
    fixture.open("notes.md", ui);
    for _ in 0..3 {
        let position = ui
            .debug_bounds("plugin-ui-preview-scroll")
            .unwrap()
            .center();
        let before = ui.debug_bounds("plugin-ui-preview-body").unwrap().top();
        ui.simulate_event(gpui_kit::ScrollWheelEvent {
            position,
            delta: gpui_kit::ScrollDelta::Lines(gpui_kit::point(0., -3.)),
            ..Default::default()
        });
        ui.run_until_parked();
        let manual = ui.debug_bounds("plugin-ui-preview-body").unwrap().top();
        assert!(
            manual < before,
            "one wheel notch must move the visible preview"
        );
        for _ in 0..4 {
            fixture.settle(ui);
        }
        let settled = ui.debug_bounds("plugin-ui-preview-body").unwrap().top();
        assert!(
            (settled - manual).abs() < px(1.),
            "wheel notch bounced: {before:?} -> {manual:?} -> {settled:?}"
        );
    }
}
