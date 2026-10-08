//! Delivered icon artwork must occupy compact native buttons while retaining the public editing seam.
use super::*;
use harness::NativeMarkdown;

const TOOLS: &[&str] = &[
    "heading",
    "heading-2",
    "heading-3",
    "heading-4",
    "heading-5",
    "heading-6",
    "bold",
    "italic",
    "strike",
    "inline-code",
    "code-block",
    "quote",
    "unordered",
    "ordered",
    "task",
    "link",
    "image",
    "table",
];

/// Actual hit bounds prove the toolbar shrinks and all actions remain visible in both themes.
/// Serialized artwork is checked separately from native geometry so a missing icon cannot pass as blank space.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_icon_toolbar_is_compact_and_wraps_without_losing_actions(
    cx: &mut TestAppContext,
) {
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", "# 中文图标\n")]);
    fixture.open("notes.md", ui);
    for dark in [false, true] {
        ui.update(|_, cx| apply_theme(builtin_theme(dark), cx));
        ui.simulate_resize(size(px(1400.), px(900.)));
        fixture.settle(ui);
        let toolbar = ui.debug_bounds("editor-source-toolbar").unwrap();
        assert!(
            toolbar.size.height <= px(29.),
            "single-row toolbar: {toolbar:?}"
        );
        let document = &fixture.manager.live["markdown"].views["preview"];
        for tool in TOOLS {
            let id = format!("format-{tool}");
            let node = document.active_node(&id).unwrap();
            let wire = serde_json::to_value(node).unwrap();
            let svg = wire["button_icon"].as_str().expect("visible SVG artwork");
            assert!(svg.contains("viewBox=\"0 0 24 24\""));
            assert!(svg.contains("stroke-width=\"2\""));
            assert!(!node.tooltip.as_ref().unwrap().is_empty());
            let selector = Box::leak(format!("plugin-ui-{id}").into_boxed_str());
            let button = ui.debug_bounds(selector).expect("native icon hit target");
            assert_eq!(button.size, size(px(24.), px(24.)), "{tool}");
            assert!(button.left() >= toolbar.left() && button.right() <= toolbar.right());
            assert!(button.top() >= toolbar.top() && button.bottom() <= toolbar.bottom());
        }
        // A narrower real source pane must wrap every control instead of clipping the last group.
        ui.simulate_resize(size(px(700.), px(550.)));
        fixture.settle(ui);
        let toolbar = ui.debug_bounds("editor-source-toolbar").unwrap();
        assert!(toolbar.size.height > px(29.));
        for tool in TOOLS {
            let selector = Box::leak(format!("plugin-ui-format-{tool}").into_boxed_str());
            let button = ui.debug_bounds(selector).unwrap();
            assert!(button.right() <= toolbar.right() && button.bottom() <= toolbar.bottom());
        }
    }
}

/// The footer groups every preview control together, and non-Markdown documents show none of them.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_status_icon_follows_sync_and_toggles_only_toolbar(cx: &mut TestAppContext) {
    let (mut fixture, ui) = NativeMarkdown::mount(
        cx,
        &[
            ("notes.md", "# 底栏\n"),
            ("other.md", "# 另一个文件\n"),
            ("plain.txt", "普通文本"),
        ],
    );
    let caption = |fixture: &NativeMarkdown| {
        fixture.manager.live["markdown"].views["preview"]
            .tools
            .iter()
            .find(|tool| tool.id == "display-toolbar")
            .unwrap()
            .tooltip
            .zh_cn
            .clone()
    };
    fixture.open("notes.md", ui);
    assert!(matches!(
        caption(&fixture).as_str(),
        "隐藏/显示Markdown工具栏" | "Show/hide Markdown toolbar"
    ));
    let source = ui
        .debug_bounds("plugin-tool-markdown/preview/display-source")
        .unwrap();
    let split = ui
        .debug_bounds("plugin-tool-markdown/preview/display-split")
        .unwrap();
    let sync = ui
        .debug_bounds("plugin-tool-markdown/preview/display-sync")
        .unwrap();
    let toggle = ui
        .debug_bounds("plugin-tool-markdown/preview/display-toolbar")
        .expect("toolbar control stays in the preview group");
    assert!(source.right() <= split.left());
    assert!(split.right() <= sync.left());
    assert!(sync.right() <= toggle.left());
    assert_eq!(toggle.size, size(px(24.), px(24.)));
    // One group keeps one gap: the toolbar control is spaced like the mode buttons and the sync toggle.
    let preview_mode = ui
        .debug_bounds("plugin-tool-markdown/preview/display-preview")
        .unwrap();
    let mode_gap = split.left() - source.right();
    // One group keeps one gap: every adjacent pair, including the toolbar control, is spaced alike.
    for (left, right, label) in [
        (source, split, "source→split"),
        (split, preview_mode, "split→preview"),
        (preview_mode, sync, "preview→sync"),
        (sync, toggle, "sync→toolbar"),
    ] {
        let gap = right.left() - left.right();
        assert!(
            (gap - mode_gap).abs() <= px(1.),
            "{label} gap {gap:?} differs from the group gap {mode_gap:?}"
        );
    }
    fixture.click("plugin-tool-markdown/preview/display-toolbar", ui);
    assert!(ui.debug_bounds("editor-source-toolbar").is_none());
    assert!(ui.debug_bounds("plugin-ui-preview-root").is_some());
    assert!(
        ui.debug_bounds("plugin-tool-markdown/preview/display-sync")
            .is_some()
    );
    fixture.open("other.md", ui);
    assert!(ui.debug_bounds("editor-source-toolbar").is_none());
    assert!(ui.debug_bounds("plugin-ui-preview-root").is_some());
    // An ordinary document has no preview at all, so no preview control may remain in the footer.
    fixture.open("plain.txt", ui);
    assert!(
        ui.debug_bounds("plugin-tool-markdown/preview/display-toolbar")
            .is_none()
    );
    assert!(
        ui.debug_bounds("plugin-tool-markdown/preview/display-sync")
            .is_none()
    );
    assert!(
        ui.debug_bounds("plugin-tool-markdown/preview/display-source")
            .is_none()
    );
    assert!(
        ui.debug_bounds("plugin-panel-toggle-markdown/preview")
            .is_none(),
        "the toolbar control never degrades into a panel toggle"
    );
    fixture.open("notes.md", ui);
    assert!(
        ui.debug_bounds("plugin-tool-markdown/preview/display-toolbar")
            .is_some(),
        "the stored toolbar choice survives a non-Markdown file"
    );
    assert!(ui.debug_bounds("editor-source-toolbar").is_none());
    fixture.click("plugin-tool-markdown/preview/display-toolbar", ui);
    assert!(ui.debug_bounds("plugin-ui-preview-root").is_some());
    assert!(ui.debug_bounds("editor-source-toolbar").is_some());
    // The footer stays focused and can be activated repeatedly, including after its toolbar is gone.
    fixture.click("plugin-tool-markdown/preview/display-toolbar", ui);
    assert!(ui.debug_bounds("editor-source-toolbar").is_none());
    let keystroke = gpui_kit::Keystroke::parse("space").unwrap();
    ui.simulate_event(gpui_kit::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    ui.simulate_event(gpui_kit::KeyUpEvent { keystroke });
    fixture.settle(ui);
    assert!(ui.debug_bounds("editor-source-toolbar").is_some());
    ui.update(|_, cx| apply_theme(builtin_theme(true), cx));
    fixture.settle(ui);
    let sync = ui
        .debug_bounds("plugin-tool-markdown/preview/display-sync")
        .unwrap();
    let toggle = ui
        .debug_bounds("plugin-tool-markdown/preview/display-toolbar")
        .unwrap();
    assert!(sync.right() <= toggle.left());
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        "# 底栏\n"
    );
}
