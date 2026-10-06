//! Verify empty-panel geometry and the document-to-canvas lifecycle through GPUI.

use crate::*;
use gpui_kit::{TestAppContext, VisualTestContext, component::Root, gpui};
use std::cell::RefCell;

/// Start with either a real document or an empty workspace, using the full dock layout.
fn canvas_window<'a>(
    cx: &'a mut TestAppContext,
    open_document: bool,
) -> (
    tempfile::TempDir,
    Entity<EditorApp>,
    &'a mut VisualTestContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        theme::apply_theme(theme::builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("canvas.txt");
    let initial = open_document.then(|| {
        std::fs::write(&path, "document text").unwrap();
        path
    });
    let workspace = editor_core::Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let captured = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| EditorApp::new(workspace, initial, window, cx));
        *captured.borrow_mut() = Some(view.clone());
        Root::new(view, window, cx)
    });
    let view = slot.borrow_mut().take().unwrap();
    cx.simulate_resize(size(px(1000.), px(700.)));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    (directory, view, cx)
}

/// The canvas occupies the whole editor panel and centers guidance on both axes.
fn assert_centered_canvas(cx: &mut VisualTestContext) {
    let panel = cx.debug_bounds("editor-panel-content").unwrap();
    let canvas = cx.debug_bounds("editor-empty-canvas").unwrap();
    let message = cx.debug_bounds("editor-empty-message").unwrap();
    assert_eq!(panel, canvas);
    assert!((canvas.center().x - message.center().x).abs() <= px(1.));
    assert!((canvas.center().y - message.center().y).abs() <= px(1.));
}

/// Closing the final tab removes document input; reopening restores the same disk text.
#[gpui::test]
fn closing_last_tab_shows_centered_canvas_and_reopens_document(cx: &mut TestAppContext) {
    let (directory, view, cx) = canvas_window(cx, true);
    let path = directory.path().join("canvas.txt");
    assert!(cx.debug_bounds("editor-empty-canvas").is_none());
    cx.update(|window, cx| {
        view.update(cx, |app, cx| {
            // Tabs store normalized Windows paths, just like the visible close control.
            let active = app.active_path.clone().unwrap();
            app.close_tab(active, window, cx);
            assert!(app.tabs.is_empty());
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_centered_canvas(cx);
    let canvas = cx.debug_bounds("editor-empty-canvas").unwrap();
    cx.simulate_click(canvas.center(), Modifiers::default());
    cx.simulate_input("must not become a document");
    cx.update(|_, cx| {
        let app = view.read(cx);
        assert!(app.tabs.is_empty());
        assert!(app.active_path.is_none());
        assert!(app.editor.read(cx).text().to_string().is_empty());
        assert!(app.editor.read(cx).line_height().is_none());
    });
    // A changed dock/window extent must retain true vertical and horizontal centering.
    cx.simulate_resize(size(px(1200.), px(850.)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_centered_canvas(cx);
    cx.update(|window, cx| {
        view.update(cx, |app, cx| app.open_file(path, window, cx));
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("editor-empty-canvas").is_none());
    cx.update(|_, cx| {
        assert_eq!(
            view.read(cx).editor.read(cx).text().to_string(),
            "document text"
        );
        assert!(view.read(cx).editor.read(cx).line_height().is_some());
    });
}

/// A workspace without restored or initial tabs uses the same blank canvas at startup.
#[gpui::test]
fn empty_workspace_starts_with_centered_canvas(cx: &mut TestAppContext) {
    let (_directory, view, cx) = canvas_window(cx, false);
    assert_centered_canvas(cx);
    cx.update(|_, cx| assert!(view.read(cx).tabs.is_empty()));
}

/// ASCII magic strings in notes must retain the native session and editable file bytes.
#[gpui::test]
fn image_like_text_prefixes_keep_text_editing(cx: &mut TestAppContext) {
    let (directory, view, cx) = canvas_window(cx, false);
    for (index, contents) in [
        "P2 bug: example",
        "BMI is a label",
        "GIF89a is a format",
        "GIF89a 是格式说明",
    ]
    .iter()
    .enumerate()
    {
        let path = directory.path().join(format!("notes-{index}.txt"));
        std::fs::write(&path, contents).unwrap();
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.open_file(path.clone(), window, cx);
                let tab = app
                    .text_tab(app.active_text_tab_index().expect("notes must be editable"))
                    .unwrap();
                assert_eq!(tab.editor.read(cx).text().to_string(), *contents);
            })
        });
    }
}

/// A missing viewer preserves the binary file tab and never sends input/save to a text editor.
#[gpui::test]
fn binary_file_keeps_its_tab_without_mounting_a_text_editor(cx: &mut TestAppContext) {
    let (directory, view, cx) = canvas_window(cx, false);
    let path = directory.path().join("picture.png");
    let bytes = b"\x89PNG\r\n\x1a\n\xff";
    std::fs::write(&path, bytes).unwrap();
    cx.update(|window, cx| view.update(cx, |app, cx| app.open_file(path.clone(), window, cx)));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("editor-tabs-container").is_some());
    assert!(cx.debug_bounds("editor-source-pane").is_none());
    assert!(cx.debug_bounds("file-view-unavailable").is_some());
    cx.simulate_input("must not edit an image");
    cx.update(|_, cx| view.update(cx, |app, cx| app.save_current(cx)));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    cx.update(|window, cx| {
        view.update(cx, |app, cx| {
            let active = app.active_path.clone().unwrap();
            app.close_tab(active, window, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_centered_canvas(cx);
}
