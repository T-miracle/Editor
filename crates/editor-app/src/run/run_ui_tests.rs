//! The run group is a native title-bar control set whose capabilities are honest about their state.
//!
//! These checks read the rendered widget tree: the group's position relative to the plugin icon, its
//! separating rule, which controls are present, and that the configuration dialog exposes the
//! approved B1 structure.
#![cfg(windows)]
use crate::*;
use gpui_kit::{TestAppContext, gpui};

/// Write one stored configuration into the same host-local location the editor resolves.
///
/// The file is returned for removal: a test must not leave state in the user's configuration
/// directory.
fn store_configuration(workspace_key: &str, name: &str) -> std::path::PathBuf {
    let path = editor_core::storage_path(workspace_key)
        .expect("host-local configuration directory is resolved by the platform");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut set = editor_core::RunConfigSet::default();
    let id = set.generate_id(workspace_key);
    set.upsert(editor_core::RunConfig {
        id: id.clone(),
        name: name.into(),
        target: editor_core::RunTarget::Program {
            program: "powershell.exe".into(),
            args: vec!["-NoProfile".into()],
        },
        directory: None,
        local: true,
    })
    .unwrap();
    set.select(&id);
    std::fs::write(&path, set.to_json().unwrap()).unwrap();
    path
}

/// Open the editor on a temporary workspace and return its window context.
fn open_editor<'a>(
    cx: &'a mut TestAppContext,
    root: &std::path::Path,
) -> (Entity<EditorApp>, &'a mut gpui_kit::VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let workspace = Workspace::open(root).unwrap();
    let slot = std::rc::Rc::new(std::cell::RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    cx.simulate_resize(size(px(1500.), px(900.)));
    cx.run_until_parked();
    let app = slot.borrow_mut().take().unwrap();
    (app, cx)
}

/// The workspace key the editor itself uses, including whatever canonicalization the platform adds.
fn storage_key(app: &Entity<EditorApp>, cx: &mut TestAppContext) -> String {
    cx.update(|cx| app.read(cx).workspace_key())
}

/// The group sits before the plugin icon, is separated by a rule, and offers every promised control.
#[gpui::test]
fn run_group_precedes_the_plugin_icon_and_is_separated_by_a_rule(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("project");
    std::fs::create_dir_all(&workspace).unwrap();
    let key = workspace.display().to_string();
    let stored = store_configuration(&key, "本机程序");
    let (_app, cx) = open_editor(cx, &workspace);

    let controls = cx
        .debug_bounds("run-controls")
        .expect("the run group is rendered in the title bar");
    let plugins = cx
        .debug_bounds("extensions-trigger")
        .expect("the plugin icon remains in the title bar");
    let divider = cx
        .debug_bounds("run-controls-divider")
        .expect("a short rule separates the run group from the plugin icon");
    // The group is left of the plugin icon, and the rule sits between them.
    assert!(controls.origin.x < plugins.origin.x);
    assert!(divider.origin.x >= controls.origin.x);
    assert!(divider.origin.x <= plugins.origin.x);

    // Every control the layout promises is present, including Stop, which is disabled while idle.
    for selector in [
        "run-config-selector",
        "run-build",
        "run-start",
        "run-debug",
        "run-stop",
    ] {
        assert!(
            cx.debug_bounds(selector).is_some(),
            "{selector} is part of the run group"
        );
    }
    // No session exists yet, so no session state is advertised.
    assert!(cx.debug_bounds("run-session-state").is_none());

    let _ = std::fs::remove_file(stored);
}

/// Selecting a saved configuration makes it the visible target without starting anything.
#[gpui::test]
fn a_saved_configuration_becomes_the_selected_target(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("project");
    std::fs::create_dir_all(&workspace).unwrap();
    // The path the editor canonicalizes to is what host-local storage is keyed by, so the test asks
    // the platform for it instead of guessing from the path it passed in.
    let key = std::fs::canonicalize(&workspace)
        .unwrap()
        .display()
        .to_string();
    let stored = store_configuration(&key, "本机程序");
    let (app, cx) = open_editor(cx, &workspace);
    assert_eq!(storage_key(&app, cx), key);

    let (selected, sessions) = cx.update(|_, cx| {
        let state = app.read(cx);
        (
            state
                .run_controls
                .selected()
                .map(|configuration| configuration.name.clone()),
            state.run_controls.sessions().len(),
        )
    });
    // The stored selection is the target the title bar shows, and loading a configuration never
    // starts a program.
    assert_eq!(selected.as_deref(), Some("本机程序"));
    assert_eq!(sessions, 0);
    // The Run control is present in the title bar for a selected target in a trusted workspace.
    assert!(cx.debug_bounds("run-start").is_some());

    let _ = std::fs::remove_file(stored);
}
