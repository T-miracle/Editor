//! Native editor operations are the seam: real input reaches an owned Shell and painted grid.

use super::*;
use gpui_kit::{TestAppContext, VisualTestContext, gpui};
use std::cell::RefCell;
mod fullscreen;
mod lifecycle;
mod restore;

/// Observe the actual native Canvas projection rather than private parser flags or mocked output.
fn painted(app: &Entity<EditorApp>, visual: &mut VisualTestContext) -> String {
    visual.update(|_, cx| {
        let terminal = app.read(cx).terminal.read(cx);
        terminal
            .canvas
            .as_ref()
            .map(|canvas| {
                canvas
                    .read(cx)
                    .drawing
                    .paint
                    .iter()
                    .filter_map(|paint| {
                        if let protocol::Paint::Text { text, .. } = paint {
                            Some(text.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<String>()
            })
            .unwrap_or_default()
    })
}

/// Real processes use wall time; GPUI debounce and frame timers use the test dispatcher clock.
fn wait(
    visual: &mut VisualTestContext,
    app: &Entity<EditorApp>,
    mut ready: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.executor().advance_clock(Duration::from_millis(30));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        if ready(visual) {
            return;
        }
        if Instant::now() >= deadline {
            let state = visual.update(|_, cx| {
                let panel = app.read(cx).terminal.read(cx);
                (
                    panel.error.clone(),
                    panel
                        .sessions
                        .iter()
                        .map(|tab| (tab.id, tab.launched, tab.exited))
                        .collect::<Vec<_>>(),
                    panel.width,
                    panel.height,
                )
            });
            panic!(
                "native terminal timed out: {state:?}; painted={:?}",
                painted(app, visual)
            );
        }
        std::thread::sleep(Duration::from_millis(15));
    }
}

/// A second tab has unchanged pixel geometry: it still needs its own measured PTY launch.
#[gpui::test]
fn native_shell_tabs_input_output_clear_and_trust(cx: &mut TestAppContext) {
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
    visual.simulate_resize(size(px(1100.), px(800.)));
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let toggle = visual.debug_bounds("terminal-toggle").unwrap();
    visual.simulate_click(toggle.center(), Default::default());
    wait(visual, &app, |visual| {
        painted(&app, visual).contains(if cfg!(windows) { "PS" } else { "$" })
    });
    let output = visual.debug_bounds("native-terminal-output").unwrap();
    visual.simulate_click(output.center(), Default::default());
    visual.simulate_input(if cfg!(windows) {
        "Write-Output ('native-' + 'one')"
    } else {
        "printf 'native-one\\n'"
    });
    visual.simulate_keystrokes("enter");
    wait(visual, &app, |visual| {
        painted(&app, visual).contains("native-one")
    });

    let new_tab = visual.debug_bounds("native-terminal-new").unwrap();
    visual.simulate_click(new_tab.center(), Default::default());
    wait(visual, &app, |visual| {
        painted(&app, visual).contains(if cfg!(windows) { "PS" } else { "$" })
    });
    let output = visual.debug_bounds("native-terminal-output").unwrap();
    visual.simulate_click(output.center(), Default::default());
    visual.simulate_input(if cfg!(windows) {
        "Write-Output ('native-' + 'two')"
    } else {
        "printf 'native-two\\n'"
    });
    visual.simulate_keystrokes("enter");
    wait(visual, &app, |visual| {
        painted(&app, visual).contains("native-two")
    });
    assert!(!painted(&app, visual).contains("native-one"));
    visual.update(|_, cx| {
        let terminal = app.read(cx).terminal.read(cx);
        assert_eq!(terminal.sessions[0].name, terminal.sessions[1].name);
    });
    // The actual command output is selected by pointer coordinates from its painted row.
    let (row, cell_width, cell_height) = visual.update(|_, cx| {
        let terminal = app.read(cx).terminal.read(cx);
        let row = terminal
            .canvas
            .as_ref()
            .unwrap()
            .read(cx)
            .drawing
            .paint
            .iter()
            .filter_map(|paint| {
                if let protocol::Paint::Text { x, y, text, .. } = paint
                    && *x == 8.
                    && text == "n"
                {
                    Some(*y)
                } else {
                    None
                }
            })
            .next()
            .unwrap();
        (row, terminal.cell_width, terminal.cell_height)
    });
    let start = output.origin + point(px(8.), px(row + cell_height / 2.));
    let end = start + point(px(cell_width * 9.), px(0.));
    visual.simulate_mouse_down(start, MouseButton::Left, Default::default());
    visual.simulate_mouse_move(end, MouseButton::Left, Default::default());
    visual.simulate_mouse_up(end, MouseButton::Left, Default::default());
    visual.simulate_keystrokes("ctrl-c");
    assert_eq!(
        visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
        "native-two"
    );
    // Click empty padding: Ctrl+C must keep the clipboard and the right-click copy item is disabled.
    visual.simulate_click(output.center(), Default::default());
    visual.simulate_keystrokes("ctrl-c");
    assert_eq!(
        visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
        "native-two"
    );
    visual.simulate_mouse_down(output.center(), MouseButton::Right, Default::default());
    visual.simulate_mouse_up(output.center(), MouseButton::Right, Default::default());
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.update(|_, cx| {
        let terminal = app.read(cx).terminal.read(cx);
        assert!(
            terminal
                .menu_items(true)
                .iter()
                .find(|item| item.id == "copy")
                .unwrap()
                .disabled
        );
    });
    let clear = visual.debug_bounds("native-menu-clear").unwrap();
    visual.simulate_click(clear.center(), Default::default());
    wait(visual, &app, |visual| painted(&app, visual).is_empty());
    visual.update(|_, cx| app.update(cx, |app, cx| app.set_workspace_trusted(false, cx)));
    wait(visual, &app, |visual| {
        visual.update(|_, cx| {
            app.read(cx)
                .terminal
                .read(cx)
                .sessions
                .iter()
                .all(|tab| tab.exited)
        })
    });
}
