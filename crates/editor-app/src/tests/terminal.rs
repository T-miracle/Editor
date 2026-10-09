//! Exercise the built-in terminal through the real editor footer and painted native view.

use crate::*;
use gpui_kit::{TestAppContext, gpui};

/// A fresh editor must expose its terminal without installing a terminal package.
#[gpui::test]
fn terminal_is_available_without_an_installed_package(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        Root::new(app, window, cx)
    });
    visual.simulate_resize(size(px(1100.), px(800.)));
    visual.run_until_parked();
    let button = visual
        .debug_bounds("terminal-toggle")
        .expect("the built-in terminal has a footer entry even without any installed package");
    visual.simulate_click(button.center(), Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("native-terminal-output").is_some());
}
