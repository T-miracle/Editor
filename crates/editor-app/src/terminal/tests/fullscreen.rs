//! Real Neovim enters the alternate screen, accepts Unicode input and survives native PTY resizes.

use super::*;

/// Requires an existing Neovim installation; no test installs tools or supplies a fake terminal app.
#[gpui::test]
#[ignore = "requires the existing nvim executable on PATH"]
fn native_terminal_neovim_unicode_and_resize(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("terminal-edit.txt");
    std::fs::write(&target, "native-TUI-marker\n").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    visual.simulate_resize(size(px(1100.), px(800.)));
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let toggle = visual.debug_bounds("terminal-toggle").unwrap();
    visual.simulate_click(toggle.center(), Default::default());
    wait(visual, &app, |visual| {
        painted(&app, visual).contains(if cfg!(windows) { "PS" } else { "$" })
    });
    // The command is intentional user input; the native launch itself always uses separate argv.
    visual.simulate_input(&format!(
        "nvim -u NONE -n '{}'",
        target.to_string_lossy().replace('\\', "/")
    ));
    visual.simulate_keystrokes("enter");
    wait(visual, &app, |visual| {
        painted(&app, visual).contains("native-TUI-marker")
    });
    visual.simulate_keystrokes("end a");
    visual.simulate_input("中文😀");
    wait(visual, &app, |visual| {
        painted(&app, visual).contains("中文😀")
    });
    for (width, height) in [(850., 610.), (1300., 900.), (930., 690.), (1100., 800.)] {
        visual.simulate_resize(size(px(width), px(height)));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
    }
    wait(visual, &app, |visual| {
        painted(&app, visual).contains("native-TUI-marker中文😀")
    });
    visual.simulate_keystrokes("escape");
    visual.simulate_input(":wq");
    visual.simulate_keystrokes("enter");
    wait(visual, &app, |_| {
        std::fs::read_to_string(&target)
            .unwrap()
            .contains("native-TUI-marker中文😀")
    });
    wait(visual, &app, |visual| {
        painted(&app, visual).contains(if cfg!(windows) { "PS" } else { "$" })
    });
}
