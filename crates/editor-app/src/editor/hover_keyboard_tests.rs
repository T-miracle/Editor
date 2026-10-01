//! Details close on document edits and keyboard navigation without stale reopening.

use super::*;
use gpui_kit::VisualTestContext;

/// Leave the suppressed symbol and hover it again through the real pointer path.
fn show_details(cx: &mut VisualTestContext, view: &Entity<EditorApp>, position: Point<Pixels>) {
    let blank = cx.update(|_, cx| {
        let bounds = view.read(cx).editor.read(cx).input_bounds();
        point(bounds.right() - px(8.), position.y)
    });
    cx.simulate_mouse_move(blank, None::<MouseButton>, Default::default());
    cx.simulate_mouse_move(position, None::<MouseButton>, Default::default());
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("editor-definition-card").is_some());
}

/// Exercise key routing and edit events while the pointer stays on one symbol.
#[gpui::test]
fn hover_closes_on_keyboard_navigation_and_edits(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        theme::apply_theme(theme::builtin_theme(false), cx);
        cx.set_reduce_motion(true);
        // Production installs this shell shortcut during application startup.
        cx.bind_keys([KeyBinding::new(
            "ctrl-i",
            ShowDefinitionDetails,
            Some("EditorShell && !PluginSurface"),
        )]);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("keyboard.rs");
    std::fs::write(&path, "alpha beta\nsecond_word\n").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| EditorApp::new(workspace, Some(path), window, cx));
        *capture.borrow_mut() = Some(view.clone());
        Root::new(view, window, cx)
    });
    let view = slot.borrow_mut().take().unwrap();
    cx.update(|window, _| window.activate_window());
    cx.simulate_resize(size(px(1000.), px(600.)));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let requests = Rc::new(Cell::new(0));
    let position = cx.update(|window, cx| {
        let editor = view.read(cx).editor.clone();
        editor.read(cx).focus_handle(cx).focus(window, cx);
        editor.update(cx, |state, cx| {
            state.set_cursor_position(lsp_types::Position::new(0, 0), window, cx);
            state.lsp_mut().hover_provider = Some(Rc::new(ReadyHover(requests.clone())));
        });
        editor.read(cx).range_to_bounds(&(0..5)).unwrap().center()
    });
    // Pointer details must remain hidden until the hover delay expires.
    cx.simulate_mouse_move(position, None::<MouseButton>, Default::default());
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_millis(999));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("editor-definition-card").is_none());
    // The shortcut bypasses that pending pointer timer without advancing time.
    cx.simulate_keystrokes("ctrl-i");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("editor-definition-card").is_some());
    let before = cx.update(|_, cx| view.read(cx).editor.read(cx).cursor());
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_ne!(
        cx.update(|_, cx| view.read(cx).editor.read(cx).cursor()),
        before
    );
    assert!(
        cx.debug_bounds("editor-definition-card").is_none(),
        "moving the caret must dismiss details"
    );
    let before_requests = requests.get();
    cx.simulate_mouse_move(
        position + point(px(1.), px(0.)),
        None::<MouseButton>,
        Default::default(),
    );
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-definition-card").is_none(),
        "cached details must not reopen on the same symbol"
    );
    assert_eq!(requests.get(), before_requests);

    show_details(cx, &view, position);
    // Copy does not move the caret or edit the document, so it retains details.
    cx.simulate_keystrokes("ctrl-c");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("editor-definition-card").is_some());
    cx.simulate_input("x");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-definition-card").is_none(),
        "typing must dismiss details"
    );
    cx.update(|_, cx| {
        let app = view.read(cx);
        assert!(app.editor.read(cx).text().to_string().starts_with("axlpha"));
        assert!(app.pointer_hover_cached.is_none());
        assert!(!app.pointer_hover_pending);
    });
    // Forward deletion edits text without moving the caret. It must be
    // covered by Change events, independently of the keyboard cursor check.
    show_details(cx, &view, position);
    let cursor = cx.update(|_, cx| view.read(cx).editor.read(cx).cursor());
    cx.simulate_keystrokes("delete");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(
        cx.update(|_, cx| view.read(cx).editor.read(cx).cursor()),
        cursor
    );
    assert!(cx.debug_bounds("editor-definition-card").is_none());
    cx.update(|_, cx| {
        assert!(
            view.read(cx)
                .editor
                .read(cx)
                .text()
                .to_string()
                .starts_with("axpha")
        );
    });
}
