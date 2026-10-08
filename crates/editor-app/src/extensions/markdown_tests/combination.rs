//! Combined delivered-package interactions preserve the sole native document across layout and IME changes.
use super::*;
use gpui_kit::{ClipboardItem, EntityInputHandler, Image, ImageFormat, VisualTestContext};
use harness::NativeMarkdown;
use std::time::{Duration, Instant};

/// Read the native authority after each gesture; the rendered tree is only a derived snapshot.
fn text(fixture: &NativeMarkdown, ui: &mut VisualTestContext) -> String {
    ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string())
}

/// A complete editing session catches interactions that isolated formatting or viewport tests miss.
#[gpui::test]
#[ignore = "build markdown and TOML resources through scripts/build-plugins.ps1 first"]
fn delivered_markdown_combines_edits_images_tasks_links_ime_and_scaled_layout(
    cx: &mut TestAppContext,
) {
    let mut original = "```novel\nanswer = 42\n```\n\n- [ ] 组合任务\n\n[下一页](next.md)\n\n| 项目 | 数量 |\n| --- | --- |\n| 中文 | 2 |\n\n".to_owned();
    for index in 0..60 {
        original.push_str(&format!(
            "段落 {index:02}：用于放大字体后的同步阅读与中文换行。\n\n"
        ));
    }
    let (mut fixture, ui) =
        NativeMarkdown::mount(cx, &[("notes.md", &original), ("next.md", "# 链接目标\n")]);
    let provider = language_tests::packages::language_package("combination-language");
    fixture
        .manager
        .install(&provider, provider.manifest.permissions.clone())
        .unwrap();
    language_tests::publish_languages(&fixture.app, &fixture.manager, ui);
    fixture.open("notes.md", ui);
    let deadline = Instant::now() + Duration::from_secs(5);
    while ui
        .debug_bounds("plugin-code-highlight-b-0-code-0")
        .is_none()
    {
        fixture.settle(ui);
        assert!(
            Instant::now() < deadline,
            "the real selected provider must paint the fenced code"
        );
    }

    // Formatting and preview task edits enter the same DocumentSession undo history.
    let label = original.find("组合任务").unwrap();
    ui.update(|_, cx| {
        fixture
            .app
            .read(cx)
            .editor
            .clone()
            .update(cx, |editor, cx| {
                editor.set_selected_range(label..label + "组合任务".len(), cx);
            })
    });
    fixture.click("plugin-ui-format-bold", ui);
    assert!(text(&fixture, ui).contains("- [ ] **组合任务**"));
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-z");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(text(&fixture, ui), original);
    let marker = original.find("[ ]").unwrap();
    let selector = Box::leak(format!("plugin-checkbox-marker-b-{marker}-task").into_boxed_str());
    fixture.click(selector, ui);
    let checked = original.replacen("[ ]", "[x]", 1);
    assert_eq!(text(&fixture, ui), checked);

    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-end");
    let image_bytes = image_preview::png(80, 240);
    let image = Image::from_bytes(ImageFormat::Png, image_bytes.clone());
    ui.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_image(&image)));
    ui.simulate_keystrokes("ctrl-v");
    ui.run_until_parked();
    fixture.settle(ui);
    image_preview::complete(&mut fixture, ui);
    assert_eq!(
        std::fs::read(fixture.directory.path().join("img.png")).unwrap(),
        image_bytes
    );
    let pasted = text(&fixture, ui);
    assert!(pasted.starts_with(&checked) && pasted.contains("](img.png)"));
    ui.simulate_keystrokes("ctrl-z");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        text(&fixture, ui),
        checked,
        "one Undo removes only the image reference"
    );
    assert!(fixture.directory.path().join("img.png").exists());
    ui.simulate_keystrokes("ctrl-y");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(text(&fixture, ui), pasted);

    // A live native preedit is preserved when an asynchronous toolbar intent is refused.
    ui.simulate_keystrokes("ctrl-home");
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
    fixture.settle(ui);
    let composing = text(&fixture, ui);
    fixture.click("plugin-ui-format-bold", ui);
    assert_eq!(text(&fixture, ui), composing);
    assert!(
        ui.update(|window, cx| fixture
            .app
            .read(cx)
            .editor
            .clone()
            .update(cx, |editor, cx| editor.marked_text_range(window, cx)))
            .is_some()
    );
    ui.update(|window, cx| {
        fixture
            .app
            .read(cx)
            .editor
            .clone()
            .update(cx, |editor, cx| {
                editor.unmark_text(window, cx);
            })
    });
    fixture.focus_editor(ui);
    // Commit the preedit as its own paragraph so its text cannot turn the following fence into plain text.
    ui.simulate_keystrokes("right");
    ui.simulate_input("\n\n");
    ui.simulate_keystrokes("ctrl-home");
    ui.simulate_input("中文输入\n\n");
    ui.run_until_parked();
    fixture.settle(ui);
    let edited = text(&fixture, ui);
    assert!(
        edited.starts_with("中文输入\n\n拼\n\n```novel\n"),
        "native committed composition must preserve the fence: {:?}",
        edited.chars().take(70).collect::<String>()
    );
    assert_eq!(
        std::fs::read_to_string(fixture.directory.path().join("notes.md")).unwrap(),
        original
    );

    // The settings font step and local themes must leave the whole toolbar usable in a narrow window.
    ui.update(|_, cx| {
        typography::step_by(cx, 6);
        crate::ui::theme::sync_font_sizes(cx);
        cx.refresh_windows();
    });
    ui.simulate_resize(size(px(960.), px(700.)));
    for dark in [true, false] {
        ui.update(|_, cx| apply_theme(builtin_theme(dark), cx));
        fixture.settle(ui);
        let toolbar = ui.debug_bounds("editor-source-toolbar").unwrap();
        for selector in [
            "plugin-ui-format-heading",
            "plugin-ui-format-bold",
            "plugin-ui-format-image",
            "plugin-ui-format-table",
        ] {
            let bounds = ui.debug_bounds(selector).unwrap();
            assert!(
                bounds.left() >= toolbar.left() - px(1.)
                    && bounds.right() <= toolbar.right() + px(1.)
            );
            assert!(bounds.bottom() <= toolbar.bottom() + px(1.));
        }
        assert!(
            ui.debug_bounds("plugin-tool-markdown/preview/display-sync")
                .is_some()
        );
        assert_eq!(text(&fixture, ui), edited);
    }
    let position = ui
        .debug_bounds("plugin-ui-preview-scroll")
        .unwrap()
        .center();
    ui.simulate_event(gpui_kit::ScrollWheelEvent {
        position,
        delta: gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-1100.))),
        ..Default::default()
    });
    ui.run_until_parked();
    for _ in 0..8 {
        fixture.settle(ui);
    }
    let viewport = ui.debug_bounds("plugin-ui-preview-scroll").unwrap();
    let mut visible = Vec::new();
    fixture.manager.live["markdown"].views["preview"]
        .root
        .visit(&mut |node| {
            if node.id.ends_with("-paragraph")
                && let Some(range) = node.source_range
            {
                let selector = Box::leak(format!("plugin-ui-{}", node.id).into_boxed_str());
                if let Some(bounds) = ui.debug_bounds(selector)
                    && bounds.bottom() > viewport.top()
                    && bounds.top() < viewport.bottom()
                {
                    visible.push((range, (bounds.top() - viewport.top()).abs()));
                }
            }
        });
    let (range, _) = visible
        .into_iter()
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .expect("scaled native preview has a visible paragraph");
    ui.update(|_, cx| {
        let editor = fixture.app.read(cx).editor.read(cx);
        let source_rows = editor.visible_row_range().unwrap();
        let start = editor.text().offset_to_point(range.start).row;
        let end = editor
            .text()
            .offset_to_point(range.end.saturating_sub(1))
            .row;
        assert!(
            start < source_rows.end && end >= source_rows.start,
            "scaled preview block must remain visible in source"
        );
    });
    assert_eq!(text(&fixture, ui), edited);

    // Return to the source start, activate the real relative link, and preserve the unsaved source tab.
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-home");
    ui.run_until_parked();
    for _ in 0..8 {
        fixture.settle(ui);
    }
    let link_offset = edited.find("[下一页]").unwrap();
    let link_selector = Box::leak(format!("plugin-ui-b-{link_offset}-paragraph").into_boxed_str());
    let link = ui.debug_bounds(link_selector).unwrap();
    // Returning through source navigation must reveal the actual link before a real pointer hit.
    let pane = ui.debug_bounds("plugin-ui-preview-scroll").unwrap();
    assert!(
        pane.contains(&point(link.left() + px(12.), link.center().y)),
        "source Home must reveal the link: link={link:?}, pane={pane:?}"
    );
    ui.simulate_click(
        point(link.left() + px(12.), link.center().y),
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
        )
    );
    assert_eq!(text(&fixture, ui), "# 链接目标\n");
    fixture.open("notes.md", ui);
    assert_eq!(text(&fixture, ui), edited);
    fixture.manager.disable("markdown").unwrap();
    fixture.settle(ui);
    assert!(ui.debug_bounds("plugin-ui-preview-root").is_none());
    assert!(ui.debug_bounds("editor-source-toolbar").is_none());
    assert_eq!(text(&fixture, ui), edited);
}
