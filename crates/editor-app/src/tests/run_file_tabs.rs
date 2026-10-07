//! Verify run preparation and debugger navigation against real text and file-only tabs together.

use crate::*;
use gpui_kit::{TestAppContext, gpui};

/// Opening an image must not lend it the previous text session or create a save obligation.
#[gpui::test]
fn run_save_gate_preserves_file_only_tabs_and_saves_background_text(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.txt");
    std::fs::write(&source, "original\n").unwrap();
    let image = directory.path().join("picture.png");
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        2,
        2,
        image::Rgb([50, 120, 210]),
    ))
    .write_to(&mut bytes, image::ImageFormat::Png)
    .unwrap();
    std::fs::write(&image, bytes.get_ref()).unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = std::rc::Rc::new(std::cell::RefCell::new(None));
    let capture = slot.clone();
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, Some(source), window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    visual.simulate_resize(size(px(1000.), px(700.)));
    visual.run_until_parked();
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.editor.update(cx, |editor, cx| {
                editor.insert("edited ", window, cx);
            });
            app.open_file(image.clone(), window, cx);
        });
    });
    visual.run_until_parked();
    let expected_text = visual.update(|_, cx| {
        let state = app.read(cx);
        assert!(state.active_text_tab_index().is_none());
        let text = state.text_tab(0).unwrap();
        assert!(text.session.is_dirty());
        text.editor.read(cx).value().to_string()
    });
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            assert!(app.save_dirty_documents(cx));
            // A debugger path must resolve to text capability rather than borrow the background editor.
            assert!(!app.open_debug_location(image.to_str().unwrap(), 1, window, cx));
            assert!(app.active_text_tab_index().is_none());
        });
    });
    assert_eq!(
        std::fs::read_to_string(directory.path().join("source.txt")).unwrap(),
        expected_text
    );
    assert_eq!(std::fs::read(&image).unwrap(), *bytes.get_ref());
}
