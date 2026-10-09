//! Session shortcuts and automatic selection must launch the correct restored native Shell.

use super::*;

/// Use the editor's actual shortcut/input entry points across a two-tab window restart.
#[gpui::test]
fn shortcuts_and_closing_active_restored_tab_keep_remaining_shell_usable(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let (app, visual) = super::restore::open(cx, Workspace::open(directory.path()).unwrap());
    let toggle = visual.debug_bounds("terminal-toggle").unwrap();
    visual.simulate_click(toggle.center(), Default::default());
    wait(visual, &app, |visual| {
        painted(&app, visual).contains(if cfg!(windows) { "PS" } else { "$" })
    });
    visual.simulate_keystrokes("ctrl-shift-t");
    wait(visual, &app, |visual| {
        visual.debug_bounds("side-tab-2").is_some()
    });
    wait(visual, &app, |visual| {
        painted(&app, visual).contains(if cfg!(windows) { "PS" } else { "$" })
    });
    let written_after = std::time::SystemTime::now();
    let path = visual.update(|_, cx| app.read(cx).terminal.read(cx).storage.join("state.json"));
    wait(visual, &app, |_| {
        std::fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .is_ok_and(|stamp| stamp >= written_after)
    });
    visual.update(|window, _| window.remove_window());
    drop(app);
    cx.run_until_parked();
    let (app, visual) = super::restore::open(cx, Workspace::open(directory.path()).unwrap());
    wait(visual, &app, |visual| {
        visual.debug_bounds("side-tab-2").is_some()
    });
    // A restored dock can be visible while editor focus remains elsewhere; shortcuts target its input.
    let output = visual.debug_bounds("native-terminal-output").unwrap();
    visual.simulate_click(output.center(), Default::default());
    visual.simulate_keystrokes("ctrl-shift-w");
    wait(visual, &app, |visual| {
        visual.debug_bounds("side-tab-2").is_none()
    });
    visual.simulate_input(if cfg!(windows) {
        "Write-Output ('remaining-' + 'shell')"
    } else {
        "printf 'remaining-shell\\n'"
    });
    visual.simulate_keystrokes("enter");
    wait(visual, &app, |visual| {
        painted(&app, visual).contains("remaining-shell")
    });
}
