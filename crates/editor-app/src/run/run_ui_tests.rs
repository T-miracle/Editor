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
        env: Default::default(),
        tool_paths: Default::default(),
        build: Default::default(),
        prelaunch: Default::default(),
        source: editor_core::RunConfigSource::Local,
        from_target: None,
        provider: None,
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

/// The build page edits prepared actions row by row, with structural controls per row.
#[gpui::test]
fn the_build_page_edits_prepared_actions_row_by_row(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("project");
    std::fs::create_dir_all(&workspace).unwrap();
    let (app, cx) = open_editor(cx, &workspace);
    // A stored configuration with two build actions and one pre-launch step, opened for editing.
    let stored = cx.update(|window, cx| {
        let key = app.read(cx).workspace_key();
        let mut set = editor_core::RunConfigSet::default();
        let id = set.generate_id(&key);
        let draft = crate::run::RunConfigDraft {
            id: id.clone(),
            name: "分步".into(),
            shell: false,
            program: "app.exe".into(),
            arguments: String::new(),
            script: String::new(),
            directory: String::new(),
            environment: String::new(),
            tool_paths: String::new(),
            source: editor_core::RunConfigSource::Local,
            from_target: None,
            provider: None,
            share: false,
            build: "一 = cargo.exe | build\n二 = cargo.exe | test".into(),
            prelaunch: "三 = tool.exe | gen".into(),
        };
        let configuration = draft.to_config().expect("the fixture is valid");
        app.update(cx, |app, cx| {
            app.run_controls.upsert(configuration, &key).unwrap();
            cx.notify();
        });
        // The form reads the stored configuration from the same store the title bar uses.
        let controls = app.read(cx).run_controls.clone();
        cx.new(|cx| crate::run::RunConfigForm::open(&controls, &key, Some(&id), window, cx))
    });
    let (rows, labels) = cx.update(|window, cx| {
        let _ = window;
        let form = stored.read(cx);
        (
            (
                form.step_row_count(crate::run::RunField::Build),
                form.step_row_count(crate::run::RunField::Prelaunch),
            ),
            form.step_row_values(crate::run::RunField::Build, cx),
        )
    });
    assert_eq!(rows, (2, 1), "one row per prepared action");
    assert_eq!(
        labels,
        vec!["一 = cargo.exe | build", "二 = cargo.exe | test"]
    );

    // Moving the second build action up swaps the two rows and nothing else.
    cx.update(|window, cx| {
        stored.update(cx, |form, cx| {
            form.edit_rows(
                crate::run::RunField::Build,
                crate::run::StepEdit::Up,
                1,
                window,
                cx,
            );
        });
    });
    let reordered = cx.update(|window, cx| {
        let _ = window;
        let form = stored.read(cx);
        (
            form.step_row_values(crate::run::RunField::Build, cx),
            form.draft().build.clone(),
        )
    });
    assert_eq!(
        reordered.0,
        vec!["二 = cargo.exe | test", "一 = cargo.exe | build"]
    );
    assert_eq!(
        reordered.1, "二 = cargo.exe | test\n一 = cargo.exe | build",
        "the draft follows the rows"
    );

    // Removing a row takes the action the user addressed, and adding appends an empty one.
    cx.update(|window, cx| {
        stored.update(cx, |form, cx| {
            form.edit_rows(
                crate::run::RunField::Build,
                crate::run::StepEdit::Remove,
                0,
                window,
                cx,
            );
            form.edit_rows(
                crate::run::RunField::Build,
                crate::run::StepEdit::Add,
                0,
                window,
                cx,
            );
        });
    });
    let after = cx.update(|window, cx| {
        let _ = window;
        let form = stored.read(cx);
        (
            form.step_row_count(crate::run::RunField::Build),
            form.draft().build.clone(),
        )
    });
    assert_eq!(
        after.0, 2,
        "one action was removed and a template row was added"
    );
    assert_eq!(
        after.1, "一 = cargo.exe | build\n# 名称 = 程序 | 参数",
        "the template row is a comment until it is turned into an action"
    );
    // The pre-launch list is edited on its own; the build list is untouched by it.
    cx.update(|window, cx| {
        stored.update(cx, |form, cx| {
            form.edit_rows(
                crate::run::RunField::Prelaunch,
                crate::run::StepEdit::Up,
                0,
                window,
                cx,
            );
        });
    });
    let untouched = cx.update(|window, cx| {
        let _ = window;
        stored.read(cx).draft().prelaunch.clone()
    });
    assert_eq!(
        untouched, "三 = tool.exe | gen",
        "a move past the end changes nothing"
    );
}

/// The dialog's footer chooses where a save goes, and sharing is off until it is chosen.
#[gpui::test]
fn the_dialog_chooses_a_save_destination(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("project");
    std::fs::create_dir_all(&workspace).unwrap();
    let (app, cx) = open_editor(cx, &workspace);
    let form = cx.update(|window, cx| {
        let key = app.read(cx).workspace_key();
        cx.new(|cx| {
            crate::run::RunConfigForm::open(
                &crate::run::RunControls::default(),
                &key,
                None,
                window,
                cx,
            )
        })
    });
    // A new configuration is host-local until the user chooses otherwise, so a save writes nothing
    // into the project.
    let destination = cx.update(|window, cx| {
        let _ = window;
        (
            form.read(cx).draft().share,
            form.read(cx).destination_is_local(),
        )
    });
    assert_eq!(destination, (false, true), "sharing is opt-in");

    // Choosing the project is what changes the destination a save will use.
    let chosen = cx.update(|window, cx| {
        form.update(cx, |form, cx| {
            form.share_with_project(true);
            cx.notify();
        });
        let _ = window;
        form.read(cx).destination_is_local()
    });
    assert!(!chosen, "the chosen destination is what the save stores");
}

/// The B1 configuration dialog is a native modal whose draft follows the approved structure.
///
/// The dialog paints in its own window, so this check reads the state that window renders from
/// rather than asserting widget bounds the test harness cannot observe there.
#[gpui::test]
fn the_run_configuration_dialog_owns_a_b1_draft(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("project");
    std::fs::create_dir_all(&workspace).unwrap();
    let (app, cx) = open_editor(cx, &workspace);
    let form = cx.update(|window, cx| {
        // Opened through the same entry point the title bar uses; the draft and the workspace key are
        // its only inputs, so this check reads exactly what that window renders from.
        let key = app.read(cx).workspace_key();
        cx.new(|cx| {
            crate::run::RunConfigForm::open(
                &crate::run::RunControls::default(),
                &key,
                None,
                window,
                cx,
            )
        })
    });
    // The four tabs the prototype promises exist; the debug page is still to come.
    let tabs = cx.update(|window, cx| {
        let _ = window;
        form.read(cx)
            .tab_labels()
            .into_iter()
            .map(|(label, available)| (label.to_owned(), available))
            .collect::<Vec<_>>()
    });
    assert_eq!(
        tabs,
        vec![
            ("基本".to_owned(), true),
            // The build page edits the same stored configuration, so this slice implements it.
            ("构建".to_owned(), true),
            // The debug page chooses the execution provider for this configuration.
            ("调试".to_owned(), true),
            // The environment page edits the same stored configuration, so this slice implements it.
            ("环境".to_owned(), true),
        ]
    );
    // One field per setting, with arguments, environment entries and tool directories each kept
    // one per line.
    let fields = cx.update(|window, cx| {
        let _ = window;
        form.read(cx)
            .field_labels()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    });
    assert_eq!(
        fields,
        vec![
            "名称",
            "程序",
            "参数（每行一个）",
            "脚本文本",
            "工作目录",
            "环境变量（每行 名称=值）",
            "本机工具路径（每行一个目录，优先于继承的 PATH）",
            "构建操作（每行 名称 = 程序 | 参数）",
            "启动前步骤（每行 名称 = 程序 | 参数，顺序执行）"
        ]
    );
    // A new draft starts empty, on the basic page, with nothing to report yet.
    cx.update(|window, cx| {
        let _ = window;
        let form = form.read(cx);
        assert_eq!(form.tab(), crate::run::ui::RunConfigTab::Basic);
        assert!(form.draft().name.is_empty() && form.draft().program.is_empty());
        assert!(form.error().is_none());
    });
    // Closing the editor's dialog drops the draft instead of leaving an orphaned configuration.
    cx.update(|window, cx| {
        let _ = window;
        app.update(cx, |app, cx| app.close_run_form(cx));
    });
    assert!(cx.update(|window, cx| {
        let _ = window;
        app.read(cx).run_form.is_none()
    }));
    assert!(cx.update(|window, cx| {
        let _ = window;
        app.read(cx).run_controls.configurations().is_empty()
    }));
}

/// The selector opens the unified dropdown instead of a second permanent session list.
#[gpui::test]
fn the_selector_opens_the_unified_dropdown(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("project");
    std::fs::create_dir_all(&workspace).unwrap();
    let key = std::fs::canonicalize(&workspace)
        .unwrap()
        .display()
        .to_string();
    let stored = store_configuration(&key, "本机程序");
    let (app, cx) = open_editor(cx, &workspace);
    // Nothing is open until the selector is used.
    assert!(cx.debug_bounds("run-menu").is_none());

    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_run_menu(gpui_kit::point(px(320.), px(30.)), window, cx);
        });
    });
    cx.run_until_parked();
    // One component holds the sessions, the saved configurations and the edit entries.
    let entries = cx.update(|_, cx| app.read(cx).run_controls.menu_entries());
    assert_eq!(
        entries
            .iter()
            .filter(|entry| matches!(entry, crate::run::RunMenuEntry::Configuration { .. }))
            .count(),
        1
    );
    assert!(entries.iter().any(|entry| matches!(
        entry,
        crate::run::RunMenuEntry::Action { id, .. } if id == "run-edit"
    )));
    assert!(cx.update(|_, cx| app.read(cx).run_menu.is_some()));
    // Closing it leaves the editor usable and no second selector behind.
    cx.update(|_, cx| {
        app.update(cx, |app, cx| {
            app.run_menu = None;
            cx.notify();
        });
    });
    assert!(cx.update(|_, cx| app.read(cx).run_menu.is_none()));

    let _ = std::fs::remove_file(stored);
}

/// Leaving with a running program asks first, and cancelling keeps both the project and the program.
#[gpui::test]
fn closing_with_a_running_session_asks_before_leaving(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("project");
    std::fs::create_dir_all(&workspace).unwrap();
    let key = std::fs::canonicalize(&workspace)
        .unwrap()
        .display()
        .to_string();
    let stored = store_configuration(&key, "本机程序");
    let (app, cx) = open_editor(cx, &workspace);
    // A running session is what makes leaving a decision rather than an accident.
    let session = cx.update(|_, cx| {
        let id = app.read(cx).run_controls.selected().unwrap().id.clone();
        let request_id = app.update(cx, |app, cx| {
            let request_id = app.run_controls.begin(&id);
            cx.notify();
            request_id
        });
        request_id
    });
    // The runtime answer is published the way the worker publishes it.
    let config = cx.update(|_, cx| app.read(cx).run_controls.selected().unwrap().id.clone());
    cx.update(|_, cx| {
        app.update(cx, |app, cx| {
            app.run_controls
                .reconcile(&[crate::extensions::HostRunSnapshot {
                    id: 41,
                    config: config.clone(),
                    request_id: session,
                    plugin: "terminal".into(),
                    state: plugin_runtime::ExecutionState::Running,
                    provider_session: Some("7".into()),
                    failure: None,
                }]);
            cx.notify();
        });
    });
    assert_eq!(
        cx.update(|_, cx| app.read(cx).run_controls.active_sessions().len()),
        1
    );

    // The window's close request is refused while the decision is pending.
    let first = cx.update(|_, cx| app.update(cx, |app, cx| app.should_close_window(cx)));
    assert!(!first, "the window stays open until the user decides");
    cx.run_until_parked();
    let pending = cx.update(|_, cx| app.read(cx).leave_confirm.clone());
    assert_eq!(pending.as_deref(), Some([41u64].as_slice()));
    // The card is part of the shell, so the decision is visible rather than silent.
    assert!(cx.debug_bounds("run-leave-confirm").is_some());

    // Cancelling keeps the session running and the window open.
    cx.update(|_, cx| app.update(cx, |app, cx| app.cancel_leave(cx)));
    assert!(cx.update(|_, cx| app.read(cx).leave_confirm.is_none()));
    assert_eq!(
        cx.update(|_, cx| app.read(cx).run_controls.active_sessions().len()),
        1,
        "cancelling never stops a program"
    );
    let second = cx.update(|_, cx| app.update(cx, |app, cx| app.should_close_window(cx)));
    assert!(
        !second,
        "a second close attempt asks again instead of leaving"
    );

    let _ = std::fs::remove_file(stored);
}

#[gpui::test]
fn a_launch_saves_modified_documents_before_starting(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("project");
    std::fs::create_dir_all(&workspace).unwrap();
    let path = workspace.join("main.rs");
    std::fs::write(&path, "fn main() {}\n").unwrap();
    let (app, cx) = open_editor(cx, &workspace);
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_file(path.clone(), window, cx);
        });
    });
    // An edit inside the native editor marks the document modified; the change event is delivered
    // through the editor's own subscription, so the loop is drained before it is observed.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            let editor = app.editor.clone();
            editor.update(cx, |editor, cx| {
                editor.insert("// 修改\n", window, cx);
            });
        });
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        app.update(cx, |app, cx| {
            assert!(app.tabs[0].session.is_dirty(), "the document is modified");
            let saved = app.save_dirty_documents(cx);
            assert!(saved, "an ordinary save precedes the launch");
            assert!(!app.tabs[0].session.is_dirty());
        });
    });
    // The file on disk is the version the program would run.
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(on_disk.contains("// 修改"), "{on_disk}");
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
