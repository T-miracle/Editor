//! Navigation starts at real native rich-text links and keeps the editor's document-open authority.
use super::*;
use harness::NativeMarkdown;

mod synchronized;

/// Exercise the actual Base press/release contract; dispatch_keystroke alone omits native KeyUp.
fn activate_key(ui: &mut gpui_kit::VisualTestContext, key: &str) {
    let keystroke = gpui_kit::Keystroke::parse(key).unwrap();
    ui.simulate_event(gpui_kit::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    ui.simulate_event(gpui_kit::KeyUpEvent { keystroke });
}

/// Activate the first glyphs of the native link, rather than its full-width paragraph wrapper.
fn activate_first_link(fixture: &mut NativeMarkdown, ui: &mut gpui_kit::VisualTestContext) {
    let bounds = ui.debug_bounds("plugin-ui-b-0-paragraph").unwrap();
    ui.simulate_click(
        gpui_kit::point(bounds.left() + px(12.), bounds.center().y),
        Default::default(),
    );
    ui.run_until_parked();
    fixture.settle(ui);
}

/// A normal relative Markdown link opens its existing target through the editor, without editing the source.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_relative_link_opens_native_document(cx: &mut TestAppContext) {
    let original = "[打开](next.md)\n";
    let (mut fixture, ui) =
        NativeMarkdown::mount(cx, &[("notes.md", original), ("next.md", "# 目标\n")]);
    fixture.open("notes.md", ui);
    assert!(
        ui.opened_url().is_none(),
        "layout cannot launch the browser"
    );
    let bounds = ui.debug_bounds("plugin-ui-b-0-paragraph").unwrap();
    // The only link is the first two glyphs of this native text line, not the stretched wrapper's centre.
    ui.simulate_click(
        gpui_kit::point(bounds.left() + px(12.), bounds.center().y),
        Default::default(),
    );
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).active_path.clone()),
        Some(
            fixture
                .directory
                .path()
                .join("next.md")
                .canonicalize()
                .unwrap()
        ),
        "a rendered link must invoke the controlled editor open path"
    );
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        "# 目标\n"
    );
    assert!(
        ui.opened_url().is_none(),
        "a document link stays in the editor"
    );
    fixture.open("notes.md", ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        original
    );
}

/// A duplicate Unicode heading has a stable suffix and navigation makes that actual native title visible.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_heading_link_reveals_duplicate_title(cx: &mut TestAppContext) {
    let original = format!(
        "[跳转](#标题-1)\n\n{}# 标题\n\n正文\n\n# 标题\n\n{}",
        "长段落测试。\n\n".repeat(70),
        "结尾段落。\n\n".repeat(20)
    );
    let target_id = format!("b-{}-heading", original.rfind("# 标题").unwrap());
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", &original)]);
    fixture.open("notes.md", ui);
    assert!(
        fixture.manager.live["markdown"].views["preview"]
            .active_node(&target_id)
            .is_some()
    );
    // GPUI's diagnostic selector takes a static string; assert the fixture offset before using it.
    assert_eq!(original.rfind("# 标题"), Some(1439));
    let selector = "plugin-ui-b-1439-heading";
    let pane = ui.debug_bounds("plugin-ui-preview-root").unwrap();
    assert!(
        ui.debug_bounds(selector)
            .is_none_or(|bounds| bounds.top() >= pane.bottom()),
        "target begins outside the visible preview"
    );
    activate_first_link(&mut fixture, ui);
    let title = ui.debug_bounds(selector).expect("native heading layout");
    assert!(
        title.top() >= pane.top() && title.bottom() <= pane.bottom(),
        "clicked heading is visible: title={title:?}, pane={pane:?}"
    );
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        original
    );
    assert!(ui.opened_url().is_none());
}

/// A relative fragment must wait for the real opened document and reveal its own Unicode heading.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_relative_fragment_uses_opened_target(cx: &mut TestAppContext) {
    let original = "[打开](next.md#%E7%9B%AE%E6%A0%87)\n";
    let target = format!(
        "{}# 目标\n\n{}",
        "目标前置段落。\n\n".repeat(70),
        "目标尾段。\n\n".repeat(20)
    );
    assert_eq!(target.find("# 目标"), Some(1610));
    let (mut fixture, ui) =
        NativeMarkdown::mount(cx, &[("notes.md", original), ("next.md", &target)]);
    fixture.open("notes.md", ui);
    activate_first_link(&mut fixture, ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).active_path.clone()),
        Some(
            fixture
                .directory
                .path()
                .join("next.md")
                .canonicalize()
                .unwrap()
        )
    );
    let title = ui
        .debug_bounds("plugin-ui-b-1610-heading")
        .expect("actual opened target heading is drawn");
    let pane = ui.debug_bounds("plugin-ui-preview-root").unwrap();
    assert!(
        title.top() >= pane.top() && title.bottom() <= pane.bottom(),
        "target heading is visible: {title:?}, {pane:?}"
    );
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        target
    );
    assert!(ui.opened_url().is_none());
}

/// A real loaded image preserves its parsed outer document link without mutating the source.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_image_link_opens_its_outer_target(cx: &mut TestAppContext) {
    let (mut fixture, ui) = NativeMarkdown::mount(
        cx,
        &[
            ("notes.md", "[![打开](img.svg)](next.md)\n"),
            ("next.md", "# 图片链接目标\n"),
        ],
    );
    std::fs::write(fixture.directory.path().join("img.svg"), "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"80\" height=\"60\" viewBox=\"0 0 80 60\"><rect width=\"80\" height=\"60\" fill=\"red\"/></svg>").unwrap();
    fixture.open("notes.md", ui);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while ui.debug_bounds("plugin-image-pixels-b-1-image").is_none() {
        fixture.settle(ui);
        assert!(
            std::time::Instant::now() < deadline,
            "real linked image loads"
        );
    }
    fixture.click("plugin-image-pixels-b-1-image", ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).active_path.clone()),
        Some(
            fixture
                .directory
                .path()
                .join("next.md")
                .canonicalize()
                .unwrap()
        ),
        "the actual image retains its parsed outer link"
    );
    assert!(ui.opened_url().is_none());
}

/// Preview-only links expose individual visible focus targets for native Tab and Enter/Space.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_links_activate_each_href_from_keyboard(cx: &mut TestAppContext) {
    let original = "[甲](https://example.com/one) 与 [乙](https://example.com/two)\n";
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", original)]);
    fixture.open("notes.md", ui);
    fixture.click("plugin-tool-markdown/preview/display-preview", ui);
    for _ in 0..36 {
        activate_key(ui, "tab");
        ui.run_until_parked();
        fixture.settle(ui);
        if ui
            .debug_bounds("plugin-link-focus-b-0-paragraph-0")
            .is_some()
        {
            break;
        }
    }
    assert!(
        ui.debug_bounds("plugin-link-focus-b-0-paragraph-0")
            .is_some(),
        "first actual href has a visible keyboard focus target"
    );
    activate_key(ui, "enter");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(ui.opened_url().as_deref(), Some("https://example.com/one"));
    activate_key(ui, "tab");
    ui.run_until_parked();
    fixture.settle(ui);
    assert!(
        ui.debug_bounds("plugin-link-focus-b-0-paragraph-1")
            .is_some(),
        "next href receives its own visible focus"
    );
    activate_key(ui, "space");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(ui.opened_url().as_deref(), Some("https://example.com/two"));
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        original
    );
}

/// Tab reveals a distant target's actual native cue before that target can be activated.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_keyboard_link_focus_reveals_the_distant_target(cx: &mut TestAppContext) {
    let original = "前文\n\n".repeat(80) + "[尾部](https://example.com/late)\n";
    assert_eq!(original.find("[尾部]"), Some(640));
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", &original)]);
    fixture.open("notes.md", ui);
    fixture.click("plugin-tool-markdown/preview/display-preview", ui);
    for _ in 0..36 {
        activate_key(ui, "tab");
        ui.run_until_parked();
        fixture.settle(ui);
        if ui
            .debug_bounds("plugin-link-focus-b-640-paragraph-0")
            .is_some()
        {
            break;
        }
    }
    let cue = ui
        .debug_bounds("plugin-link-focus-b-640-paragraph-0")
        .expect("distant href has its own actual keyboard focus");
    let pane = ui.debug_bounds("plugin-ui-preview-root").unwrap();
    assert!(
        cue.top() >= pane.top() && cue.bottom() <= pane.bottom(),
        "focused target must be visible before activation: {cue:?}, {pane:?}"
    );
    assert!(ui.opened_url().is_none());
    activate_key(ui, "enter");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(ui.opened_url().as_deref(), Some("https://example.com/late"));
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        original
    );
}

/// A hidden source toolbar cannot swallow navigation failures in the legitimate preview-only mode.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_preview_only_link_failures_remain_visible_in_both_locales(
    cx: &mut TestAppContext,
) {
    let files = [
        ("missing.md", "[无效锚点](#missing)\n"),
        ("web.md", "[网页](https://example.com/refused)\n"),
    ];
    let package = harness::package_without_permission("navigation.external");
    let (mut fixture, ui) = NativeMarkdown::mount_package(cx, &files, &package);
    for ((name, original), chinese, english) in files
        .into_iter()
        .zip([("锚点", "anchor"), ("权限", "permission")])
        .map(|(file, (chinese, english))| (file, chinese, english))
    {
        fixture.open(name, ui);
        fixture.click("plugin-tool-markdown/preview/display-preview", ui);
        assert!(ui.debug_bounds("editor-source-toolbar").is_none());
        assert!(
            ui.debug_bounds("plugin-ui-preview-navigation-feedback")
                .is_none()
        );
        activate_first_link(&mut fixture, ui);
        for (locale, expected) in [("zh-CN", chinese), ("en", english)] {
            fixture
                .manager
                .event(
                    "markdown",
                    None,
                    PluginEvent::Theme(protocol::Environment {
                        workspace: fixture.directory.path().display().to_string(),
                        locale: locale.into(),
                        ..Default::default()
                    }),
                )
                .unwrap();
            super::super::composable_tests::publish(
                &mut fixture.manager,
                &mut fixture.renderer,
                &fixture.app,
                ui,
            );
            let feedback = ui.debug_bounds("plugin-ui-preview-navigation-feedback").unwrap_or_else(|| {
                let document = &fixture.manager.live["markdown"].views["preview"];
                panic!("visible navigation feedback missing: file={name}, locale={locale}, header={:?}, toolbar={:?}, paragraph={:?}", document.active_node("preview-navigation-feedback"), document.active_node("format-error"), ui.debug_bounds("plugin-ui-b-0-paragraph"));
            });
            let pane = ui.debug_bounds("plugin-ui-preview-root").unwrap();
            assert!(feedback.top() >= pane.top() && feedback.bottom() <= pane.bottom());
            let node = fixture.manager.live["markdown"].views["preview"]
                .active_node("preview-navigation-feedback")
                .unwrap();
            let protocol::ui::Kind::Text { text } = &node.kind else {
                panic!("readonly failure text expected")
            };
            assert!(text.to_lowercase().contains(expected));
            assert_eq!(
                ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
                original
            );
            assert!(ui.opened_url().is_none());
        }
    }
}

/// Merely rendering or right-clicking a URL cannot launch the browser; an intentional link click can.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_web_link_opens_only_at_native_click(cx: &mut TestAppContext) {
    let url = "https://example.com/guide?lang=zh#title";
    let original = format!("[网页]({url})\n");
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", &original)]);
    fixture.open("notes.md", ui);
    assert!(ui.opened_url().is_none());
    let bounds = ui.debug_bounds("plugin-ui-b-0-paragraph").unwrap();
    let position = gpui_kit::point(bounds.left() + px(12.), bounds.center().y);
    ui.simulate_mouse_down(position, gpui_kit::MouseButton::Right, Default::default());
    ui.simulate_mouse_up(position, gpui_kit::MouseButton::Right, Default::default());
    ui.run_until_parked();
    fixture.settle(ui);
    assert!(ui.opened_url().is_none());
    activate_first_link(&mut fixture, ui);
    assert_eq!(
        ui.opened_url().as_deref(),
        Some(url),
        "the existing GPUI external boundary records the requested web URL"
    );
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        original
    );
}
