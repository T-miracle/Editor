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
    // A fresh workspace hides Outline while keeping its footer control available for explicit opening.
    visual.update(|_, cx| assert!(!app.read(cx).session_state.outline_visible));
    assert!(visual.debug_bounds("outline-hide").is_none());
    let toggle = visual.debug_bounds("outline-toggle").unwrap();
    visual.simulate_click(toggle.center(), Default::default());
    visual.run_until_parked();
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(app.read(cx).session_state.outline_visible);
    });
    // Removing the redundant locate control must not remove the independent header hide control.
    assert!(visual.debug_bounds("outline-follow").is_none());
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
    // The footer window controls must restore the panel after its header has been hidden.
    let group = visual.debug_bounds("plugin-windows-group").unwrap();
    let toggle = visual.debug_bounds("outline-toggle").unwrap();
    assert!(
        group.contains(&toggle.center()),
        "the outline toggle belongs inside the footer window control group"
    );
    visual.simulate_click(toggle.center(), Default::default());
    visual.run_until_parked();
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(app.read(cx).session_state.outline_visible);
    });
    assert!(visual.debug_bounds("outline-hide").is_some());
    assert!(visual.debug_bounds("outline-follow").is_none());
}
