//! Real application startup imports old data before any guest can load, then restores native cells.
use super::*;
use plugin_runtime::{Installed, Manager};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

/// A hidden panel must still report broken migration data and protect startup layout auto-saves.
#[gpui::test]
fn corrupt_upgrade_is_visible_and_preserves_layout_during_startup(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    let native = persistence::directory(workspace.root());
    std::fs::create_dir_all(&native).unwrap();
    std::fs::write(native.join("upgrade-pending.json"), b"damaged journal").unwrap();
    let mut state = crate::app::session::SessionState::for_workspace(workspace.root());
    state.native_terminal_visible = false;
    state.save();
    let path = state.file_path().unwrap();
    let before = std::fs::read(&path).unwrap();
    let (app, visual) = restore::open(cx, workspace);
    visual.update(|_, cx| {
        let app = app.read(cx);
        assert!(!app.terminal.read(cx).visible());
        assert!(app.status.contains("damaged") || app.status.contains("expected value"));
        assert!(app.run_controls.error.is_some());
        app.session_state.save();
    });
    visual.simulate_resize(size(px(1350.), px(900.)));
    visual.run_until_parked();
    assert_eq!(std::fs::read(path).unwrap(), before);
}

/// A retired installation has no executable fixture: data import cannot run its guest or old task.
#[gpui::test]
fn legacy_upgrade_restores_native_shell_then_resizes_without_replaying_tasks(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let root = tempfile::tempdir().unwrap();
    let project = root
        .path()
        .join("restored-shell-directory-with-a-long-wrapped-command-prompt");
    std::fs::create_dir(&project).unwrap();
    let workspace = Workspace::open(&project).unwrap();
    let runtime = crate::extensions::runtime_root(workspace.root());
    let native = persistence::directory(workspace.root());
    let mut settings = Settings::default();
    settings.history = 50;
    settings.tab_position = protocol::ui::SideTabsPosition::Left;
    let cwd = workspace.root().display().to_string();
    let prompt = shell::default_prompt(&settings.profiles[0], &cwd).unwrap_or_else(|| "$ ".into());
    let manifest: protocol::Manifest = serde_json::from_value(json!({
        "id":"terminal","name":"Terminal","version":"0.12.2","protocol":7,
        "api":{"base":"^1"},"storage_limit":8388608,"component":"terminal.wasm",
        "permissions":[],"panels":[{"id":"terminal","title":"Terminal","position":"bottom"}]
    }))
    .unwrap();
    let installed = Installed {
        manifest: manifest.clone(),
        digest: "retired".into(),
        grants: BTreeSet::new(),
        enabled: true,
        project_enabled: BTreeSet::from([workspace.root().display().to_string()]),
        retired_ui_contract: false,
        global_enabled: None,
        error: None,
    };
    std::fs::create_dir_all(&runtime).unwrap();
    std::fs::write(
        runtime.join("registry.json"),
        serde_json::to_vec(&BTreeMap::from([("terminal", installed)])).unwrap(),
    )
    .unwrap();
    let data = Manager::persisted_data_directory(
        &runtime,
        &manifest,
        &workspace.root().display().to_string(),
    )
    .unwrap();
    std::fs::create_dir_all(&data).unwrap();
    let transcript = format!(
        "{prompt}\r\nOLD_UPGRADE_HISTORY\r\n{prompt}{}",
        "\r\n".repeat(9)
    );
    let snapshot = protocol::Snapshot {schema:2,data:json!({"tabs":[
        {"id":7,"name":"my-old-shell","profile":settings.profiles[0],"cwd":cwd,
            "output":transcript,"display":{"rows":12,"columns":160,"cursor":[2,prompt.chars().count()],"wrap_pending":false,"scrollback":0,"wrapped_lines":[],"soft_wraps":true}},
        {"id":8,"name":"finished-old-task","profile":{"name":"Service execution","program":"do-not-replay.exe","args":[]},"cwd":cwd,"output":"OLD_TASK_RESULT","exited":true}],
        "active":0,"next_id":8,"settings":settings,"tab_width":222.,"recovery_version":1}).to_string()};
    let source = serde_json::to_vec(&snapshot).unwrap();
    std::fs::write(data.parent().unwrap().join("state.json"), &source).unwrap();
    let (app, visual) = restore::open(cx, workspace);
    // Opening the real native panel must select the imported Shell, not create another default tab.
    let toggle = visual.debug_bounds("terminal-toggle").unwrap();
    visual.simulate_click(toggle.center(), Default::default());
    wait(visual, &app, |visual| {
        visual.update(|_, cx| app.read(cx).terminal.read(cx).sessions[0].launched)
    });
    visual.update(|_, cx| {
        let panel = app.read(cx).terminal.read(cx);
        assert_eq!(panel.sessions.len(), 2);
        assert_eq!(panel.sessions[0].name, "my-old-shell");
        assert!(panel.sessions[1].exited && panel.sessions[1].task.is_some());
        assert_eq!(panel.tab_width, 222.);
        assert_eq!(
            panel.settings.tab_position,
            protocol::ui::SideTabsPosition::Left
        );
    });
    let output = visual.debug_bounds("native-terminal-output").unwrap();
    visual.simulate_click(output.center(), Default::default());
    visual.simulate_input(if cfg!(windows) {
        "Write-Output ('continued-' + 'upgrade')"
    } else {
        "printf 'continued-upgrade\\n'"
    });
    visual.simulate_keystrokes("enter");
    wait(visual, &app, |visual| {
        painted(&app, visual).contains("continued-upgrade")
    });
    // Restored history may leave the viewport when a long prompt wraps. Compare logical cells,
    // including scrollback, instead of treating offscreen content as lost output.
    let transcript = |visual: &mut VisualTestContext| {
        visual.update(|_, cx| {
            let grid = app.read(cx).terminal.read(cx).sessions[0].engine.snapshot();
            grid.lines
                .iter()
                .flatten()
                .filter(|cell| {
                    !cell.flags.intersects(
                        alacritty_terminal::term::cell::Flags::WIDE_CHAR_SPACER
                            | alacritty_terminal::term::cell::Flags::LEADING_WIDE_CHAR_SPACER,
                    )
                })
                .map(|cell| cell.c)
                .collect::<String>()
        })
    };
    wait(visual, &app, |visual| {
        transcript(visual).matches("PS ").count() == 3
    });
    let before_resize = transcript(visual);
    assert_eq!(
        before_resize.matches("OLD_UPGRADE_HISTORY").count(),
        1,
        "before resize: {before_resize}"
    );
    for (width, height) in [(840., 600.), (1200., 950.), (900., 670.), (1100., 800.)] {
        visual.simulate_resize(size(px(width), px(height)));
        visual.executor().advance_clock(Duration::from_millis(180));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let settle = Instant::now() + Duration::from_millis(180);
        wait(visual, &app, |_| Instant::now() >= settle);
    }
    let deadline = Instant::now() + Duration::from_millis(700);
    wait(visual, &app, |_| Instant::now() >= deadline);
    assert_eq!(
        transcript(visual).matches("OLD_UPGRADE_HISTORY").count(),
        1,
        "before={before_resize}; after={}",
        transcript(visual)
    );
    let text = transcript(visual);
    assert_eq!(
        text.matches("PS ").count(),
        3,
        "resize must not redraw a restored prompt as new output: {text}"
    );
    assert_eq!(text.matches("continued-upgrade").count(), 1, "{text}");
    assert!(!painted(&app, visual).contains("restoredsession"));
    assert_eq!(
        std::fs::read(data.parent().unwrap().join("state.json")).unwrap(),
        source
    );
    assert!(native.join("upgrade-complete.json").exists());
    assert!(
        !Manager::read_registry(&runtime)
            .unwrap()
            .contains_key("terminal")
    );
}
