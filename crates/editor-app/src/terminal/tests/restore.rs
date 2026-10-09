//! Closing a native window persists its logical cells; reopening starts a fresh owned Shell.

use super::*;

/// A late validation error must neither partially import tabs nor overwrite the user's source.
#[gpui::test]
fn native_terminal_rejected_snapshot_preserves_original(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let storage = persistence::directory(workspace.root());
    std::fs::create_dir_all(&storage).unwrap();
    let settings = Settings::default();
    let grid = Engine::new(
        GridSize {
            columns: 80,
            rows: 12,
        },
        settings.history,
    )
    .snapshot();
    let mut invalid = serde_json::to_value(&grid).unwrap();
    invalid["rows"] = 1001.into();
    let source = serde_json::to_vec(&serde_json::json!({
        "version": 1, "settings": settings, "active": 1, "next_id": 2, "tab_width": 180.,
        "sessions": [
            { "id": 1, "name": "partial-import", "profile": settings.profiles[0],
              "cwd": workspace.root(), "exited": false, "grid": grid },
            { "id": 2, "name": "invalid", "profile": settings.profiles[0],
              "cwd": workspace.root(), "exited": false, "grid": invalid }
        ]
    }))
    .unwrap();
    let path = storage.join("state.json");
    std::fs::write(&path, &source).unwrap();
    let (app, visual) = open(cx, workspace);
    visual.update(|_, cx| {
        let panel = app.read(cx).terminal.read(cx);
        assert!(panel.error.is_some());
        assert!(
            panel.sessions.is_empty(),
            "a rejected file cannot leave partial tabs"
        );
    });
    let toggle = visual.debug_bounds("terminal-toggle").unwrap();
    visual.simulate_click(toggle.center(), Default::default());
    let deadline = Instant::now() + Duration::from_millis(2100);
    wait(visual, &app, |_| Instant::now() >= deadline);
    assert_eq!(
        std::fs::read(path).unwrap(),
        source,
        "autosave must preserve rejected data"
    );
}

/// Keep the editor entry point and native frame creation identical across both windows.
pub(super) fn open(
    cx: &mut TestAppContext,
    workspace: Workspace,
) -> (Entity<EditorApp>, &mut VisualTestContext) {
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    visual.simulate_resize(size(px(1100.), px(800.)));
    visual.update(|window, cx| window.draw(cx).clear(cx));
    (slot.borrow_mut().take().unwrap(), visual)
}

/// Restoration and repeated shrink/grow must not insert banners, padding lines or repeated output.
#[gpui::test]
fn native_terminal_reopen_preserves_history_and_resize(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let (app, visual) = open(cx, Workspace::open(directory.path()).unwrap());
    let toggle = visual.debug_bounds("terminal-toggle").unwrap();
    visual.simulate_click(toggle.center(), Default::default());
    wait(visual, &app, |visual| {
        painted(&app, visual).contains(if cfg!(windows) { "PS" } else { "$" })
    });
    assert!(
        visual.update(|window, cx| app
            .read(cx)
            .terminal
            .read(cx)
            .canvas
            .as_ref()
            .unwrap()
            .read(cx)
            .focus_handle()
            .is_focused(window)),
        "opening the native terminal must focus its input"
    );
    let written_after = std::time::SystemTime::now();
    visual.simulate_input(if cfg!(windows) {
        "Write-Output ('preserved-' + 'history')"
    } else {
        "printf 'preserved-history\\n'"
    });
    visual.simulate_keystrokes("enter");
    wait(visual, &app, |visual| {
        painted(&app, visual).contains("preserved-history")
    });
    let path = visual.update(|_, cx| app.read(cx).terminal.read(cx).storage.join("state.json"));
    // Observe the public durable writer before reopening; test windows defer entity retirement.
    wait(visual, &app, |_| {
        std::fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .is_ok_and(|stamp| stamp >= written_after)
    });
    // Output and the next prompt can arrive in separate PTY chunks; compare the state actually saved.
    let before = painted(&app, visual);
    let before_last = visual.update(|_, cx| {
        app.read(cx).terminal.read(cx).sessions[0]
            .engine
            .snapshot()
            .lines
            .iter()
            .rposition(|row| row.iter().any(|cell| cell.c != ' '))
            .unwrap()
    });
    // Remove the native root after its normal autosave has committed the visible content.
    visual.update(|window, _| window.remove_window());
    drop(app);
    cx.run_until_parked();
    let (app, visual) = open(cx, Workspace::open(directory.path()).unwrap());
    wait(visual, &app, |visual| {
        painted(&app, visual).contains("preserved-history")
    });
    let after = painted(&app, visual);
    assert!(
        after.contains(&before),
        "restored cells must retain the original prompt and output: {after}"
    );
    assert!(!after.contains("restoredsession"));
    for (width, height) in [(840., 600.), (1200., 950.), (900., 670.), (1100., 800.)] {
        visual.simulate_resize(size(px(width), px(height)));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
    }
    // The original old output must stay single even after ConPTY's coalesced resize redraw.
    let deadline = Instant::now() + Duration::from_millis(400);
    wait(visual, &app, |_| Instant::now() >= deadline);
    assert_eq!(
        painted(&app, visual).matches("preserved-history").count(),
        1
    );
    visual.update(|_, cx| {
        let panel = app.read(cx).terminal.read(cx);
        let saved = panel.sessions[0].engine.snapshot();
        let last_printed = saved
            .lines
            .iter()
            .rposition(|row| row.iter().any(|cell| cell.c != ' '))
            .unwrap();
        assert_eq!(
            last_printed, before_last,
            "restore/resize must not create spacer rows or extra prompts"
        );
    });
    // Closing the only Shell through the native sidebar hides the whole panel; reopening creates one.
    let tab = visual.debug_bounds("side-tab-1").unwrap();
    visual.simulate_mouse_down(tab.center(), MouseButton::Middle, Default::default());
    visual.simulate_mouse_up(tab.center(), MouseButton::Middle, Default::default());
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("native-terminal-output").is_none());
    let toggle = visual.debug_bounds("terminal-toggle").unwrap();
    visual.simulate_click(toggle.center(), Default::default());
    wait(visual, &app, |visual| {
        painted(&app, visual).contains(if cfg!(windows) { "PS" } else { "$" })
    });
}
