//! Exercise native outline header callbacks without loading a language tool or WASM package.
use super::*;

/// Header buttons must update the current outline without re-entering the panel's active click callback.
#[gpui::test]
fn native_outline_header_buttons_keep_window_alive(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    visual.update(|window, cx| {
        window.activate_window();
        window.draw(cx).clear(cx);
    });
    let initial_follow = visual.update(|_, cx| app.read(cx).session_state.outline_follow_cursor);
    let follow = visual.debug_bounds("outline-follow").unwrap();
    visual.simulate_click(follow.center(), Default::default());
    visual.run_until_parked();
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert_eq!(
            app.read(cx).session_state.outline_follow_cursor,
            !initial_follow,
            "the actual follow button must toggle the user's preference"
        );
    });
    // Dispatch a genuine mouse click while OutlinePanel still owns its listener context.
    let hide = visual.debug_bounds("outline-hide").unwrap();
    visual.simulate_click(hide.center(), Default::default());
    visual.run_until_parked();
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(!app.read(cx).session_state.outline_visible);
    });
    assert!(
        visual.debug_bounds("outline-hide").is_none(),
        "hiding the panel must remove its actual title controls while the window remains usable"
    );
}
