//! Verify shutdown with an actual clipboard-created receipt and its owned private backup.
use super::*;
use gpui_kit::{TestAppContext, component::Root, gpui};
use std::cell::RefCell;

#[gpui::test]
fn explorer_transfer_window_close_cleans_only_session_backups(cx: &mut TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let unrelated = tempfile::tempdir().unwrap();
    let source = external.path().join("file.txt");
    std::fs::write(&source, "import").unwrap();
    std::fs::write(unrelated.path().join("keep.txt"), "unrelated").unwrap();
    let workspace = Workspace::open(project.path()).unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let slot = Rc::new(RefCell::new(None));
    let captured = slot.clone();
    let (_, ui) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *captured.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    ui.simulate_resize(size(px(1000.), px(700.)));
    ui.run_until_parked();
    ui.update(|window, cx| window.draw(cx).clear(cx));
    ui.update(|_, cx| {
        cx.write_to_clipboard(gpui_kit::ClipboardItem {
            entries: vec![gpui_kit::ClipboardEntry::ExternalPaths(
                gpui_kit::ExternalPaths(vec![source.clone()].into()),
            )],
        })
    });
    let position = ui.debug_bounds("explorer-row-0").unwrap().center();
    ui.simulate_click(position, Modifiers::default());
    ui.simulate_keystrokes("ctrl-v");
    ui.run_until_parked();
    let backup = ui.update(|_, cx| {
        app.read(cx)
            .file_transfers
            .undo
            .last()
            .unwrap()
            .backup
            .path()
            .to_path_buf()
    });
    assert!(backup.exists());
    // The normal native close gate keeps the window alive until cancellation and owned cleanup finish.
    ui.update(|window, cx| {
        app.update(cx, |app, cx| {
            assert!(!app.close_file_transfer_session(window, cx));
            assert!(
                !app.close_file_transfer_session(window, cx),
                "a repeated close cannot bypass pending cleanup"
            );
        })
    });
    ui.run_until_parked();
    assert!(!backup.exists());
    assert!(project.path().join("file.txt").exists());
    assert!(source.exists());
    assert_eq!(
        std::fs::read_to_string(unrelated.path().join("keep.txt")).unwrap(),
        "unrelated"
    );
}
