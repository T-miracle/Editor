//! Actual source input geometry must fill the space beneath a package-owned toolbar.
use super::*;
use harness::NativeMarkdown;

/// A long delivered document exposes collapsed input even when the outer split and toolbar exist.
/// Native resize, mode clicks and wheel input exercise the same layout as the desktop window.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_source_fills_height_below_toolbar(cx: &mut TestAppContext) {
    let original = (0..160)
        .map(|index| format!("第 {index:03} 行：Markdown 编辑区应显示多行。\n"))
        .collect::<String>();
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", &original)]);
    fixture.open("notes.md", ui);

    for (width, height, dark) in [
        (1400., 900., false),
        (950., 700., true),
        (700., 550., false),
        (2100., 1160., true),
    ] {
        ui.simulate_resize(gpui_kit::size(px(width), px(height)));
        ui.update(|_, cx| apply_theme(builtin_theme(dark), cx));
        fixture.settle(ui);
        for mode in [
            "plugin-tool-markdown/preview/display-split",
            "plugin-tool-markdown/preview/display-source",
        ] {
            fixture.click(mode, ui);
            // Start each geometry from the top so every wheel check has content left to scroll.
            fixture.focus_editor(ui);
            ui.simulate_keystrokes("ctrl-home");
            ui.run_until_parked();
            fixture.settle(ui);
            let panel = ui.debug_bounds("editor-panel-content").unwrap();
            let source = ui.debug_bounds("editor-source-pane").unwrap();
            let toolbar = ui.debug_bounds("editor-source-toolbar").unwrap();
            let (input, visible) = ui.update(|_, cx| {
                let editor = fixture.app.read(cx).editor.read(cx);
                (editor.input_bounds(), editor.visible_row_range().unwrap())
            });
            // Check the native input, not only the container: a one-row input still has a toolbar.
            let available = panel.bottom() - toolbar.bottom();
            assert!(
                input.size.height >= available - px(24.),
                "source must fill the remaining height in {mode}: input={input:?}, source={source:?}, toolbar={toolbar:?}, panel={panel:?}, rows={visible:?}"
            );
            assert!(input.top() >= toolbar.bottom() - px(2.));
            assert!(input.bottom() <= panel.bottom() + px(2.));
            assert!(
                visible.len() > 10,
                "native viewport must reveal multiple rows"
            );

            // Click below the first line to prove that the lower area is part of the input target.
            let position = gpui_kit::point(input.left() + px(100.), input.bottom() - px(60.));
            ui.simulate_click(position, Default::default());
            ui.run_until_parked();
            fixture.settle(ui);
            assert!(
                ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).selected_range().start)
                    > original.find('\n').unwrap(),
                "pointer input in the lower editor must select a later row"
            );
            let before = ui.update(|_, cx| {
                fixture
                    .app
                    .read(cx)
                    .editor
                    .read(cx)
                    .visible_row_range()
                    .unwrap()
                    .start
            });
            ui.simulate_event(gpui_kit::ScrollWheelEvent {
                position: input.center(),
                delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-500.))),
                ..Default::default()
            });
            ui.run_until_parked();
            fixture.settle(ui);
            assert!(
                ui.update(|_, cx| {
                    fixture
                        .app
                        .read(cx)
                        .editor
                        .read(cx)
                        .visible_row_range()
                        .unwrap()
                        .start
                }) > before,
                "wheel input must move the visible source rows"
            );
        }
    }
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        original,
        "layout and scrolling must preserve the document"
    );
}

/// Empty Markdown and toolbar-free SVG must keep a full input target, as must ordinary text tabs.
/// The two delivered packages enter through Manager instead of a language-specific layout fixture.
#[gpui::test]
#[ignore = "build markdown and svg through scripts/build-plugins.ps1 first"]
fn delivered_source_height_survives_empty_documents_and_toolbar_free_previews(
    cx: &mut TestAppContext,
) {
    for (package_name, name, original, has_toolbar) in [
        ("markdown", "empty.md", "", true),
        (
            "svg",
            "image.svg",
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\"/>",
            false,
        ),
    ] {
        let package = Package::read(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../dist/plugins/{package_name}.zip")),
        )
        .unwrap();
        let (mut fixture, ui) = NativeMarkdown::mount_package(
            cx,
            &[(name, original), ("plain.txt", "ordinary text\n")],
            &package,
        );
        fixture.open(name, ui);
        for mode in [
            "plugin-tool-markdown/preview/display-split",
            "plugin-tool-markdown/preview/display-source",
        ] {
            fixture.click(mode, ui);
            let panel = ui.debug_bounds("editor-panel-content").unwrap();
            let source = ui.debug_bounds("editor-source-pane").unwrap();
            let toolbar = ui.debug_bounds("editor-source-toolbar");
            assert_eq!(toolbar.is_some(), has_toolbar);
            let top = toolbar.map_or(source.top(), |toolbar| toolbar.bottom());
            let input = ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).input_bounds());
            assert!(
                input.size.height >= panel.bottom() - top - px(24.),
                "{package_name} {mode}: actual input {input:?} must fill {panel:?} below {top:?}"
            );
            assert!(input.bottom() <= panel.bottom() + px(2.));
        }
        fixture.open("plain.txt", ui);
        assert!(ui.debug_bounds("editor-source-toolbar").is_none());
        assert!(ui.debug_bounds("plugin-ui-preview-root").is_none());
        let panel = ui.debug_bounds("editor-panel-content").unwrap();
        let source = ui.debug_bounds("editor-source-pane").unwrap();
        let input = ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).input_bounds());
        assert!(input.size.height >= panel.bottom() - source.top() - px(24.));
    }
}
