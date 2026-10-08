//! Input latency includes native dispatch and synchronous document observers, not only a later draw.
use super::*;
use harness::NativeMarkdown;

/// Compare identical mixed Markdown as plain text and as a highlighted source document.
/// Keep background draining separate: its wall time is diagnostic, not a claimed input-thread stall.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_input_to_frame_latency(cx: &mut TestAppContext) {
    let source = include_str!("../../../../../website/src/content/docs/en/sdk/ui.md")
        .lines()
        .take(120)
        .collect::<Vec<_>>()
        .join("\n");
    let (mut fixture, ui) =
        NativeMarkdown::mount(cx, &[("notes.md", &source), ("plain.txt", &source)]);
    language_tests::publish_languages(&fixture.app, &fixture.manager, ui);
    let mut measurements = Vec::new();
    for (file, mode) in [
        ("plain.txt", "plain"),
        ("notes.md", "source"),
        ("notes.md", "split"),
        ("notes.md", "source-no-toolbar"),
        ("notes.md", "split-no-toolbar"),
    ] {
        fixture.open(file, ui);
        if mode != "plain" {
            fixture.click(
                if mode.starts_with("source") {
                    "plugin-tool-markdown/preview/display-source"
                } else {
                    "plugin-tool-markdown/preview/display-split"
                },
                ui,
            );
            if mode == "source-no-toolbar" {
                fixture.click("plugin-tool-markdown/preview/display-toolbar", ui);
            }
            // Exercise the actual installed WASM grammar, including its inline injection provider.
            ui.update(|_, cx| {
                fixture
                    .app
                    .read(cx)
                    .editor
                    .clone()
                    .update(cx, |editor, cx| {
                        editor.set_highlighter("markdown", cx);
                    })
            });
        }
        fixture.focus_editor(ui);
        ui.simulate_keystrokes("ctrl-home");
        fixture.settle(ui);
        let mut samples = Vec::new();
        let mut dispatch_samples = Vec::new();
        for key in ["a", "b", "中", "文", "x", "y", "测", "试"] {
            let start = std::time::Instant::now();
            let body = ui.update(|window, cx| {
                let body = std::time::Instant::now();
                window.dispatch_keystroke(gpui_kit::Keystroke::parse(key).unwrap(), cx);
                body.elapsed()
            });
            let dispatch = start.elapsed();
            ui.update(|window, cx| window.draw(cx).clear(cx));
            let visible = start.elapsed();
            let background = std::time::Instant::now();
            ui.run_until_parked();
            eprintln!(
                "input-latency {mode} {key}: dispatch_body={body:?} dispatch_effects={dispatch:?} input_to_draw={visible:?} drain={:?}",
                background.elapsed()
            );
            samples.push(visible);
            dispatch_samples.push(dispatch);
        }
        samples.sort();
        dispatch_samples.sort();
        measurements.push((mode, dispatch_samples[4], samples[4]));
    }
    eprintln!("input-to-frame medians: {measurements:?}");
    assert!(
        measurements.iter().all(
            |(_, dispatch, elapsed)| *dispatch < Duration::from_millis(60)
                && *elapsed < Duration::from_millis(100)
        ),
        "ordinary typing must reach a frame without a perceptible synchronous stall: {measurements:?}"
    );
}
