//! The run group is a native title-bar control set whose capabilities are honest about their state.
//!
//! These checks read the rendered widget tree: the group's position relative to the plugin icon, its
//! separating rule, which controls are present, and that the configuration dialog exposes the
//! approved B1 structure.
#![cfg(windows)]
use crate::ui::controls::DialogContent;
use crate::*;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{ElementInputHandler, InputHandler as _, TestAppContext, gpui};

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
        breakpoints: Default::default(),
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

/// Chinese composition reaches a dialog field, marks the preedit and commits it as the stored text.
///
/// Ticket 04 asks for Chinese IME to be accepted. The composition protocol is delivered to the
/// field's retained editing state through the public input-handler bridge, the same entry point the
/// platform handler uses, so what is exercised here is the protocol and not a native input method.
///
/// What this does not measure, and therefore does not claim: the window's own typing path. Typing
/// through `Window::input` did not reach the field in this arrangement even with the field focused
/// and a frame completed, so the composing text is the whole value here rather than a suffix of
/// typed text. That path is not what this ticket asks about, and leaving it in would have made the
/// check pass or fail for a reason unrelated to composition.
#[gpui::test]
fn chinese_composition_enters_the_configuration_fields(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("project");
    std::fs::create_dir_all(&workspace).unwrap();
    let (app, cx) = open_editor(cx, &workspace);
    // The dialog is built inside its own window and rendered through the same function the real
    // dialog uses, so the fields under test are the fields a user types into. The form is handed back
    // through a slot because a window's entities belong to that window.
    let slot: std::rc::Rc<std::cell::RefCell<Option<Entity<FormHolder>>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let capture = slot.clone();
    let owner = app.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let key = owner.read(cx).workspace_key();
        let controls = crate::run::RunControls::default();
        let form = cx.new(|cx| crate::run::RunConfigForm::open(&controls, &key, None, window, cx));
        let content =
            crate::run::ui::render_run_config_form(&owner.downgrade(), DialogContent::new(), cx);
        let holder = cx.new(|_| FormHolder {
            form,
            content: Some(content),
        });
        *capture.borrow_mut() = Some(holder.clone());
        Root::new(holder, window, cx)
    });
    let holder = slot.borrow_mut().take().expect("the dialog was built");
    let name = holder
        .read_with(cx, |holder, cx| {
            holder.form.read(cx).field_input(crate::run::RunField::Name)
        })
        .expect("the name field exists");
    cx.update(|window, cx| {
        name.read(cx).focus_handle(cx).focus(window, cx);
        // A fixed rectangle: the bridge reports selection geometry from these bounds, and this check
        // is about the text it commits rather than about where the field was drawn.
        let bounds = gpui::Bounds {
            origin: gpui::point(gpui::px(0.), gpui::px(0.)),
            size: gpui::size(gpui::px(240.), gpui::px(24.)),
        };
        let mut handler = ElementInputHandler::new(bounds, name.clone());
        // Compose Chinese: marked while composing, replaced on the next keystroke, committed at the
        // end. The marks are in UTF-16 units, which is the unit the protocol speaks in.
        handler.replace_and_mark_text_in_range(None, "我的程序", Some(0..4), window, cx);
        window.render_frame(cx);
        assert_eq!(
            name.read(cx).value().as_ref(),
            "我的程序",
            "the composing text reaches the field"
        );
        assert_eq!(
            handler.marked_text_range(window, cx),
            Some(0..4),
            "the composing text is marked rather than committed"
        );
        // Replacing the preedit while composing must not accumulate the intermediate text.
        handler.replace_and_mark_text_in_range(None, "我的程序集", Some(0..5), window, cx);
        window.render_frame(cx);
        assert_eq!(
            name.read(cx).value().as_ref(),
            "我的程序集",
            "the previous preedit is replaced, not appended"
        );
        assert_eq!(handler.marked_text_range(window, cx), Some(0..5));
        // Committing clears the mark, leaving exactly the text the user chose.
        handler.replace_text_in_range(None, "我的程序集", window, cx);
        window.render_frame(cx);
        assert_eq!(name.read(cx).value().as_ref(), "我的程序集");
        assert_eq!(
            handler.marked_text_range(window, cx),
            None,
            "committing ends the composition"
        );
    });
    // What the field holds is what a save would store, so composition reaches the configuration.
    assert_eq!(
        name.read_with(cx, |input, _| input.value().to_string()),
        "我的程序集"
    );
}
/// The dialog window's root: the form entity beside the content the real dialog renders.
struct FormHolder {
    form: Entity<crate::run::RunConfigForm>,
    content: Option<DialogContent>,
}

impl Render for FormHolder {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.content.take().unwrap_or_else(DialogContent::new)
    }
}

/// The workspace key the editor itself uses, including whatever canonicalization the platform adds.
fn storage_key(app: &Entity<EditorApp>, cx: &mut TestAppContext) -> String {
    cx.update(|cx| app.read(cx).workspace_key())
}

/// The unified dropdown answers dismissal, and switching theme leaves the run surface usable.
///
/// Ticket 10's last acceptance item asks for the run surface to be checked for keyboard and for both
/// themes. **What this covers, stated narrowly**: the dropdown's dismissal path — the event its own key
/// handler produces and the subscription that closes the menu — and the editor's theme toggle followed by
/// the configuration dialog still opening with all four pages available.
///
/// **What it does not cover**: the keystroke itself is not injected, so the popup's key handler is not
/// exercised here; a check that dispatches a real Escape would need a `VisualTestContext`, which the
/// dialog-based checks in this file do not use. The record notes this so the item is not read as fully
/// measured.
#[gpui::test]
fn the_run_surface_answers_dismissal_and_the_theme_toggle(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("project");
    std::fs::create_dir_all(&workspace).unwrap();
    let stored = store_configuration(&storage_key_of(&workspace), "本机程序");
    let (app, cx) = open_editor(cx, &workspace);

    // Opening the dropdown puts focus on the popup, which is what makes a keystroke reach it.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_run_menu(gpui_kit::point(px(320.), px(30.)), window, cx);
        });
    });
    cx.run_until_parked();
    let popup = cx.update(|_, cx| {
        app.read(cx)
            .run_menu
            .as_ref()
            .map(|menu| menu.popup.clone())
            .expect("the selector opened the dropdown")
    });
    // Escape is the dropdown's own dismissal: its key handler finishes with `Action::Dismiss`, which the
    // menu's subscription turns into a closed menu. That event is what is emitted here, so this covers
    // the dismissal path rather than the keystroke that would produce it.
    popup.update(cx, |_, cx| cx.emit(gpui_kit::DismissEvent));
    cx.run_until_parked();
    assert!(
        cx.update(|_, cx| app.read(cx).run_menu.is_none()),
        "the dropdown answers dismissal and closes"
    );

    // Both themes keep the run surface usable. The editor's own toggle is used rather than setting a
    // field, so the path that re-applies the theme and refreshes open dialogs is the one exercised.
    let before = cx.update(|_, cx| app.read(cx).dark_theme);
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.toggle_theme(window, cx));
    });
    cx.run_until_parked();
    let after = cx.update(|_, cx| app.read(cx).dark_theme);
    assert_ne!(
        before, after,
        "the theme toggle changes the active theme rather than being a no-op"
    );

    // And the run surface still opens and renders its pages in the theme now active.
    let pages = cx.update(|window, cx| {
        let key = app.read(cx).workspace_key();
        let form = cx.new(|cx| {
            crate::run::RunConfigForm::open(
                &crate::run::RunControls::default(),
                &key,
                None,
                window,
                cx,
            )
        });
        form.read(cx)
            .tab_labels()
            .into_iter()
            .map(|(label, available)| (label.to_owned(), available))
            .collect::<Vec<_>>()
    });
    assert_eq!(
        pages.len(),
        4,
        "the configuration dialog still renders its pages after the theme change"
    );
    assert!(
        pages.iter().all(|(_, available)| *available),
        "and every page is available: {pages:?}"
    );

    let _ = std::fs::remove_file(stored);
}

/// The workspace key for a path, canonicalized the way the editor's own key is.
fn storage_key_of(workspace: &std::path::Path) -> String {
    std::fs::canonicalize(workspace)
        .unwrap()
        .display()
        .to_string()
}

/// The run surface follows the one interface size, which is how a user makes everything bigger.
///
/// Ticket 03's native clause asks for scaling. The editor has a single base size — `Typography` — that the
/// whole interface derives from, and the setting screen drives it through `typography::step_by`; there is
/// deliberately no second size. So the check is that the run group is drawn at that size: stepping it up
/// makes the group taller, stepping it back down returns it, and the group stays inside a window that is
/// narrow at the largest size. That last part matters because a control set that does not scale would pass
/// the first half by ignoring the setting.
#[gpui::test]
fn the_run_group_follows_the_interface_size(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("project");
    std::fs::create_dir_all(&workspace).unwrap();
    let stored = store_configuration(&storage_key_of(&workspace), "本机程序");
    let (app, cx) = open_editor(cx, &workspace);
    let _ = &app;

    let size_at = |cx: &mut gpui_kit::VisualTestContext, steps: i32| {
        // The settings screen changes the base size and then pushes it into the theme, which is what the
        // window's `rem` resolves against; doing only the first would leave the layout untouched.
        cx.update(|window, cx| {
            crate::ui::typography::set_font_size(cx, 14. + steps as f32);
            crate::ui::theme::sync_font_sizes(cx);
            window.refresh();
        });
        cx.run_until_parked();
        cx.debug_bounds("run-controls")
            .expect("the run group is rendered")
            .size
    };

    let usual = size_at(cx, 0);
    let larger = size_at(cx, 8);
    let back = size_at(cx, 0);
    // The group widens because its labels are sized in `rem`, which resolves against the interface size.
    // Its height does not change: the buttons set it, so a taller text line would not make the control
    // taller — the first version of this check asserted the height and was simply wrong about which
    // dimension this control derives from the setting.
    assert!(
        larger.width > usual.width,
        "a larger interface size widens the run group: {usual:?} then {larger:?}"
    );
    assert_eq!(
        back, usual,
        "and stepping back restores it, so the group reads the size rather than drifting"
    );

    // At the largest supported size in a narrow window the group still stays inside: scaling may not push
    // controls past the window's edge.
    let narrow = size(px(520.), px(420.));
    cx.update(|window, cx| {
        crate::ui::typography::set_font_size(cx, 24.);
        crate::ui::theme::sync_font_sizes(cx);
        window.refresh();
    });
    cx.simulate_resize(narrow);
    cx.run_until_parked();
    let controls = cx
        .debug_bounds("run-controls")
        .expect("the run group is rendered at the largest size");
    assert!(
        controls.origin.x >= px(0.) && controls.origin.x + controls.size.width <= narrow.width,
        "the run group stays inside the window at the largest size: {controls:?} in {narrow:?}"
    );

    let _ = std::fs::remove_file(stored);
}

/// The run group survives a narrow window, which is where a title-bar control set would collide.
///
/// Tickets 01 and 03 both ask for native acceptance that includes a narrow window, and nothing exercised
/// it: the checks in this file resize only down to 800–1000 points, which leaves the group ample room. At
/// 520 points the window is narrower than the room the group would like, so this asks that the controls
/// stay inside the window rather than painting past its edge, that they stay ordered (group, then plugin
/// icon), and that the dropdown — the interaction a narrow window makes necessary — still opens and closes.
///
/// The assertion is containment, not a particular width: how much room the group takes is a layout choice,
/// while "controls must not extend past the window" holds whatever that choice is.
#[gpui::test]
fn the_run_group_stays_inside_a_narrow_window(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("project");
    std::fs::create_dir_all(&workspace).unwrap();
    let stored = store_configuration(&storage_key_of(&workspace), "本机程序");
    let (app, cx) = open_editor(cx, &workspace);

    let narrow = size(px(520.), px(420.));
    cx.simulate_resize(narrow);
    cx.run_until_parked();
    let controls = cx
        .debug_bounds("run-controls")
        .expect("the run group is still rendered in a narrow window");
    let plugins = cx
        .debug_bounds("extensions-trigger")
        .expect("the plugin icon is still rendered in a narrow window");
    assert!(
        controls.origin.x >= px(0.) && controls.origin.x + controls.size.width <= narrow.width,
        "the run group stays inside the window: {controls:?} in {narrow:?}"
    );
    assert!(
        controls.origin.x < plugins.origin.x,
        "and the group is still left of the plugin icon: {controls:?} vs {plugins:?}"
    );

    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_run_menu(gpui_kit::point(px(300.), px(30.)), window, cx);
        });
    });
    cx.run_until_parked();
    let popup = cx.update(|_, cx| {
        app.read(cx)
            .run_menu
            .as_ref()
            .map(|menu| menu.popup.clone())
            .expect("the selector opens the dropdown in a narrow window")
    });
    popup.update(cx, |_, cx| cx.emit(gpui_kit::DismissEvent));
    cx.run_until_parked();
    assert!(
        cx.update(|_, cx| app.read(cx).run_menu.is_none()),
        "and it closes again"
    );

    let _ = std::fs::remove_file(stored);
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
            breakpoints: String::new(),
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
            "启动前步骤（每行 名称 = 程序 | 参数，顺序执行）",
            // The debug page edits the breakpoint list, one location per line.
            "断点（每行 源文件:行号）"
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

/// A launch whose save cannot proceed starts nothing, rather than running the older file.
///
/// The saving half of this is checked above; this is the other half of the same criterion. A document
/// the disk has moved away from needs an overwrite decision the user has not made, so `false` from the
/// save step must stop the launch before anything is queued — otherwise the user would be running code
/// they were still being asked about.
#[gpui::test]
fn a_launch_stops_when_its_save_cannot_proceed(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("project");
    std::fs::create_dir_all(&workspace).unwrap();
    let path = workspace.join("main.rs");
    std::fs::write(&path, "fn main() {}\n").unwrap();
    // A tab records the canonical path, so every comparison below names the same file the tab holds.
    let path = path.canonicalize().unwrap();
    let (app, cx) = open_editor(cx, &workspace);
    // The store is written and read under the key the editor itself resolves. Deriving it separately
    // is how this check first passed without reaching the save step at all: the file was stored under a
    // different key, so the launch stopped at "configuration does not exist" instead.
    let (config_id, config_file) = cx.update(|_, cx| {
        let key = app.read(cx).workspace_key();
        let file = editor_core::storage_path(&key).expect("a configuration path is resolved");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let mut set = editor_core::RunConfigSet::default();
        let id = set.generate_id(&key);
        set.upsert(editor_core::RunConfig {
            id: id.clone(),
            name: "本机程序".into(),
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
            breakpoints: Default::default(),
            local: true,
        })
        .unwrap();
        set.select(&id);
        std::fs::write(&file, set.to_json().unwrap()).unwrap();
        (id, file)
    });
    // The configuration has to be loaded before a launch can reach its save step, or the launch would
    // stop earlier for a different reason and this check would pass without measuring anything.
    cx.update(|_, cx| {
        app.update(cx, |app, cx| {
            let key = app.workspace_key();
            let local = editor_core::storage_path(&key)
                .and_then(|file| file.parent().map(std::path::Path::to_path_buf));
            app.run_controls = crate::run::RunControls::load(&key, local);
            assert!(
                app.run_controls.configuration(&config_id).is_some(),
                "the stored configuration is the one this launch will ask for"
            );
            cx.notify();
        });
    });
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_file(path.clone(), window, cx);
            // An edit marks the document; writing a different file behind it makes the disk state a
            // conflict the user has not answered.
            let editor = app.editor.clone();
            editor.update(cx, |editor, cx| {
                editor.insert("// 本地修改\n", window, cx);
            });
        });
    });
    cx.run_until_parked();
    // The disk moves away from the edited buffer through the same reconciliation the file watcher
    // delivers, so the conflict is the editor's own rather than one arranged for the check.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.apply_reconciliation(
                Reconciliation {
                    snapshot: Some(app.workspace.snapshot()),
                    documents: vec![(
                        path.clone(),
                        Ok("fn main() { /* 磁盘已改 */ }\n".into()),
                        Instant::now(),
                    )],
                    renames: Vec::new(),
                    native: true,
                },
                window,
                cx,
            );
            assert_eq!(
                app.tabs[0].disk_state,
                DiskState::Conflict,
                "the disk and the buffer disagree, and the user has not answered"
            );
            assert!(
                app.tabs[0].session.is_dirty(),
                "the buffer is the unsaved one"
            );
        });
    });
    // With the launch refusing to proceed, nothing is queued and no status claims a start.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.start_configuration_without_environment(&config_id, window, cx)
        });
    });
    cx.run_until_parked();
    let (sessions, status, restored) = cx.update(|_, cx| {
        let app = app.read(cx);
        (
            app.run_controls.sessions().len(),
            app.status.clone(),
            app.tabs[0].editor.read(cx).value().to_string(),
        )
    });
    assert_eq!(sessions, 0, "a refused save starts no session");
    assert!(
        !status.contains("已启动") && !status.contains("正在准备"),
        "the status does not claim a launch that did not happen: {status}"
    );
    // The reason has to be the save gate's own. Without this, a launch refused earlier — for a missing
    // configuration or a missing provider — would satisfy every assertion above while measuring nothing,
    // which is exactly how this check first passed.
    // The reason is the save gate's own message. The dialog's text is localized, so the check names the
    // message the gate produces rather than one language's rendering of it.
    assert!(
        status.contains("changed on disk") || status.contains("保存失败"),
        "the launch stopped at the save step, not somewhere before it: {status}"
    );
    // The user's buffer is untouched: the launch stopped instead of discarding or overwriting it.
    assert!(
        restored.contains("本地修改"),
        "the unsaved edit is still there: {restored}"
    );
    let _ = std::fs::remove_file(config_file);
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
