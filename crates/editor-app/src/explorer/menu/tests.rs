use super::*;
use crate::theme::{apply_theme, builtin_theme};
use editor_core::Workspace;
use gpui_kit::{AppContext as _, TestAppContext, VisualTestContext, component::Root, gpui};
use std::{cell::RefCell, rc::Rc};

/// Query the component's native accessibility facts instead of maintaining custom menu-row selectors.
trait NativeMenuTest {
    fn menu_bounds(&mut self, selector: &str) -> Option<Bounds<Pixels>>;
    fn menu_selected(&mut self, command: Command) -> bool;
}

impl NativeMenuTest for VisualTestContext {
    fn menu_bounds(&mut self, selector: &str) -> Option<Bounds<Pixels>> {
        let command = ROW_COMMANDS
            .iter()
            .chain(ROOT_COMMANDS)
            .chain(NEW_COMMANDS)
            .chain(SPECIAL_COPY_COMMANDS)
            .find(|command| command.id() == selector)
            .copied();
        self.update(|window, _| {
            gpui_base::test_support::snapshots(window).into_iter().find(|item| {
                if let Some(command) = command {
                    item.role() == Some(gpui_kit::Role::MenuItem)
                        && item.label() == Some(command.label().as_str())
                } else {
                    item.role() == Some(gpui_kit::Role::Menu)
                        && item.path().iter().any(|id| {
                            matches!(id, gpui_kit::ElementId::Name(name) if name.as_str() == "submenu")
                        })
                }
            }).map(|item| item.bounds())
        })
    }

    fn menu_selected(&mut self, command: Command) -> bool {
        self.update(|window, _| {
            gpui_base::test_support::snapshots(window)
                .into_iter()
                .any(|item| {
                    item.role() == Some(gpui_kit::Role::MenuItem)
                        && item.label() == Some(command.label().as_str())
                        && item.selected() == Some(true)
                })
        })
    }
}

#[test]
fn delete_follows_new_only_for_a_selected_entry() {
    assert_eq!(
        ROW_COMMANDS
            .iter()
            .position(|command| *command == Command::Delete),
        Some(4)
    );
    assert!(!ROOT_COMMANDS.contains(&Command::Delete));
}

#[test]
fn special_copy_follows_copy_and_uses_the_selected_path() {
    assert_eq!(ROW_COMMANDS[0], Command::Copy);
    assert_eq!(ROW_COMMANDS[1], Command::SpecialCopy);
    assert!(!ROOT_COMMANDS.contains(&Command::SpecialCopy));
    let root = Path::new("/project");
    let relative = Path::new("src").join("main.rs");
    let file = root.join(&relative);
    // Relative copying removes the workspace prefix and preserves the selected entry's suffix.
    assert_eq!(
        special_copy_text(Command::CopyFileName, &file, root).as_deref(),
        Some("main.rs")
    );
    assert_eq!(
        special_copy_text(Command::CopyAbsolutePath, &file, root),
        Some(file.to_string_lossy().into_owned())
    );
    assert_eq!(
        special_copy_text(Command::CopyProjectRoot, &file, root),
        Some(relative.to_string_lossy().into_owned())
    );
    assert_eq!(
        special_copy_text(Command::CopyProjectRoot, root, root).as_deref(),
        Some(".")
    );
    assert_eq!(
        special_copy_text(Command::CopyProjectRoot, Path::new("/other/file.rs"), root),
        None
    );
}

#[cfg(windows)]
#[test]
fn copied_windows_paths_omit_verbatim_prefixes() {
    // Canonicalized workspace paths use verbatim syntax, but users expect pasteable paths.
    assert_eq!(
        clipboard_path_text(Path::new(r"\\?\C:\project\src\main.rs")),
        r"C:\project\src\main.rs"
    );
    assert_eq!(
        clipboard_path_text(Path::new(r"\\?\UNC\server\share\main.rs")),
        r"\\server\share\main.rs"
    );
    // Match the reported nested Windows project path, including native separators.
    assert_eq!(
        special_copy_text(
            Command::CopyProjectRoot,
            Path::new(r"C:\Projects\RustProjects\Editor\plugins\rust\README.md"),
            Path::new(r"C:\Projects\RustProjects\Editor"),
        )
        .as_deref(),
        Some(r"plugins\rust\README.md")
    );
}

#[gpui::test]
fn builtin_themes_define_explorer_menu_colors(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        for dark in [false, true] {
            apply_theme(builtin_theme(dark), cx);
            let styles = component_styles(cx, ThemeComponent::ExplorerMenu);
            assert!(styles.base.background.is_some());
            assert!(styles.base.border.is_some());
            assert!(styles.hover.background.is_some());
        }
    });
}

/// Native anchoring and keyboard actions must work without the removed host navigation state.
#[gpui::test]
fn component_menu_snaps_to_window_and_navigates_with_keyboard(cx: &mut TestAppContext) {
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
    visual.simulate_resize(size(px(800.), px(600.)));
    for dark in [false, true] {
        visual.update(|window, cx| {
            apply_theme(builtin_theme(dark), cx);
            app.update(cx, |app, cx| {
                app.open_explorer_menu(None, point(px(790.), px(590.)), window, cx);
            });
        });
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let bounds = visual.debug_bounds("explorer-context-menu").unwrap();
        assert!(bounds.origin.x >= px(0.) && bounds.origin.y >= px(0.));
        assert!(bounds.origin.x + bounds.size.width <= px(800.));
        assert!(bounds.origin.y + bounds.size.height <= px(600.));
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        assert!(visual.update(|_, cx| app.read(cx).explorer_menu.is_none()));
    }
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_explorer_menu(None, point(px(100.), px(100.)), window, cx);
        });
    });
    visual.update(|window, cx| window.draw(cx).clear(cx));
    for key in ["down", "down", "right"] {
        visual.simulate_keystrokes(key);
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
    }
    assert!(visual.menu_selected(Command::New));
    assert!(visual.menu_bounds("explorer-new-submenu").is_some());
    visual.simulate_keystrokes("enter");
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.update(|_, cx| app.read(cx).explorer_menu.is_none()));
    assert!(visual.update(|_, cx| app.read(cx).explorer_edit.is_some()));
    // Dismissal must not steal focus from the newly opened name input.
    visual.simulate_input("new-folder");
    visual.update(|_, cx| {
        assert_eq!(
            app.read(cx)
                .explorer_edit
                .as_ref()
                .unwrap()
                .input
                .read(cx)
                .value()
                .to_string(),
            "new-folder"
        );
    });
}

#[gpui::test]
fn empty_explorer_opens_project_menu_and_new_submenu(cx: &mut TestAppContext) {
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
    let (_, window_cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    window_cx.simulate_resize(size(px(800.), px(600.)));
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    // Even an empty workspace exposes its project root and the safe workspace menu.
    let position = window_cx.debug_bounds("explorer-row-0").unwrap().center();
    window_cx.simulate_mouse_down(position, MouseButton::Right, Default::default());
    window_cx.run_until_parked();
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(window_cx.update(|_, cx| app.read(cx).explorer_menu.is_some()));
    assert!(window_cx.debug_bounds("explorer-context-menu").is_some());
    assert!(window_cx.menu_bounds("explorer-menu-rename").is_none());
    assert!(window_cx.menu_bounds("explorer-menu-delete").is_none());
    let new = window_cx.menu_bounds("explorer-menu-new").unwrap();
    window_cx.simulate_mouse_move(new.center(), None, Default::default());
    window_cx.run_until_parked();
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    window_cx.simulate_click(new.center(), Default::default());
    window_cx.run_until_parked();
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(window_cx.menu_bounds("explorer-new-submenu").is_some());
    // Moving into the child menu must leave its parent visibly selected.
    let directory_item = window_cx.menu_bounds("explorer-menu-directory").unwrap();
    window_cx.simulate_mouse_move(
        directory_item.center(),
        MouseButton::Left,
        Default::default(),
    );
    window_cx.run_until_parked();
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(window_cx.menu_selected(Command::New));
    window_cx.simulate_click(directory_item.center(), Default::default());
    window_cx.run_until_parked();
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(window_cx.debug_bounds("explorer-edit-cancel").is_some());
    // Creating a directory opens a modal edit layer that blocks other panels.
    let title = window_cx.debug_bounds("title-bar-drag-region").unwrap();
    window_cx.simulate_mouse_down(title.center(), MouseButton::Left, Default::default());
    window_cx.run_until_parked();
    assert!(!window_cx.update(|_, cx| app.read(cx).titlebar_should_move));
}

#[gpui::test]
fn right_clicked_file_is_the_menu_target(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("note.txt");
    std::fs::write(&file, "note").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, window_cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    window_cx.simulate_resize(size(px(800.), px(600.)));
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    // The project node is row zero; the selected file is its first child.
    let row = window_cx.debug_bounds("explorer-row-1").unwrap();
    window_cx.simulate_mouse_down(row.center(), MouseButton::Right, Default::default());
    window_cx.run_until_parked();
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    let target = window_cx.update(|_, cx| {
        app.read(cx)
            .explorer_menu
            .as_ref()
            .and_then(|menu| menu.target.clone())
    });
    assert_eq!(target, Some(file.canonicalize().unwrap()));
    assert!(window_cx.menu_bounds("explorer-menu-rename").is_some());
    assert!(window_cx.menu_bounds("explorer-menu-delete").is_some());
    // The default tab is open, so deletion must preserve its backing file and editor buffer.
    let delete = window_cx.menu_bounds("explorer-menu-delete").unwrap();
    window_cx.simulate_click(delete.center(), Default::default());
    window_cx.run_until_parked();
    assert!(file.exists());
    assert!(window_cx.update(|_, cx| app.read(cx).explorer_delete.is_none()));
}

#[gpui::test]
fn special_copy_submenu_writes_text_to_clipboard(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("note.txt");
    std::fs::write(&file, "note").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let (_, window_cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        Root::new(app, window, cx)
    });
    window_cx.simulate_resize(size(px(800.), px(600.)));
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    let row = window_cx.debug_bounds("explorer-row-1").unwrap();
    let expected = [
        ("explorer-menu-copy-file-name", "note.txt".to_string()),
        (
            "explorer-menu-copy-absolute-path",
            file.to_string_lossy().into_owned(),
        ),
        ("explorer-menu-copy-project-root", "note.txt".to_string()),
    ];
    for (selector, text) in expected {
        // Reopen the menu for each action because a completed copy closes both levels.
        window_cx.simulate_mouse_down(row.center(), MouseButton::Right, Default::default());
        window_cx.run_until_parked();
        window_cx.update(|window, cx| window.draw(cx).clear(cx));
        let parent = window_cx.menu_bounds("explorer-menu-special-copy").unwrap();
        window_cx.simulate_mouse_move(parent.center(), None, Default::default());
        window_cx.run_until_parked();
        window_cx.update(|window, cx| window.draw(cx).clear(cx));
        window_cx.simulate_click(parent.center(), Default::default());
        window_cx.run_until_parked();
        window_cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            window_cx
                .menu_bounds("explorer-special-copy-submenu")
                .is_some()
        );
        let action = window_cx.menu_bounds(selector).unwrap();
        window_cx.simulate_click(action.center(), Default::default());
        window_cx.run_until_parked();
        assert_eq!(
            window_cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text())),
            Some(text)
        );
    }
}

#[gpui::test]
fn clicking_outside_an_open_submenu_does_not_press_the_panel_beneath(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("note.txt"), "note").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, window_cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    window_cx.simulate_resize(size(px(800.), px(600.)));
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    let row = window_cx.debug_bounds("explorer-row-1").unwrap();
    window_cx.simulate_mouse_down(row.center(), MouseButton::Right, Default::default());
    window_cx.run_until_parked();
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    let parent = window_cx.menu_bounds("explorer-menu-special-copy").unwrap();
    window_cx.simulate_mouse_move(parent.center(), None, Default::default());
    window_cx.run_until_parked();
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    window_cx.simulate_click(parent.center(), Default::default());
    window_cx.run_until_parked();
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        window_cx
            .menu_bounds("explorer-special-copy-submenu")
            .is_some()
    );

    // The outside press dismisses the popup without arming the title bar's drag handler.
    let title = window_cx.debug_bounds("title-bar-drag-region").unwrap();
    window_cx.simulate_mouse_down(title.center(), MouseButton::Left, Default::default());
    window_cx.run_until_parked();
    assert!(!window_cx.update(|_, cx| app.read(cx).titlebar_should_move));
    assert!(window_cx.update(|_, cx| app.read(cx).explorer_menu.is_none()));
    // The same panel must become interactive again after the popup closes.
    window_cx.simulate_mouse_up(title.center(), MouseButton::Left, Default::default());
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    window_cx.simulate_mouse_down(title.center(), MouseButton::Left, Default::default());
    window_cx.run_until_parked();
    assert!(window_cx.update(|_, cx| app.read(cx).titlebar_should_move));
}

#[gpui::test]
fn deleting_a_file_requires_confirmation(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    // EditorApp opens the first discovered file automatically; keep that file distinct.
    std::fs::write(directory.path().join("a-keep.txt"), "open tab").unwrap();
    let file = directory.path().join("delete-me.txt");
    std::fs::write(&file, "keep until confirmed").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, window_cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    window_cx.simulate_resize(size(px(800.), px(600.)));
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    let row = window_cx.debug_bounds("explorer-row-2").unwrap();
    window_cx.simulate_mouse_down(row.center(), MouseButton::Right, Default::default());
    window_cx.run_until_parked();
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    let delete = window_cx.menu_bounds("explorer-menu-delete").unwrap();
    window_cx.simulate_click(delete.center(), Default::default());
    window_cx.run_until_parked();
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        file.exists(),
        "opening the preview must not remove the file"
    );
    assert!(window_cx.debug_bounds("explorer-delete-preview").is_some());
    // The modal mask must consume presses over other window panels.
    let title = window_cx.debug_bounds("title-bar-drag-region").unwrap();
    window_cx.simulate_mouse_down(title.center(), MouseButton::Left, Default::default());
    window_cx.run_until_parked();
    assert!(!window_cx.update(|_, cx| app.read(cx).titlebar_should_move));
    let cancel = window_cx.debug_bounds("explorer-delete-cancel").unwrap();
    window_cx.simulate_click(cancel.center(), Default::default());
    window_cx.run_until_parked();
    assert!(file.exists(), "cancel must preserve the file");

    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    let row = window_cx.debug_bounds("explorer-row-2").unwrap();
    window_cx.simulate_mouse_down(row.center(), MouseButton::Right, Default::default());
    window_cx.run_until_parked();
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    let delete = window_cx.menu_bounds("explorer-menu-delete").unwrap();
    window_cx.simulate_click(delete.center(), Default::default());
    window_cx.run_until_parked();
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    let confirm = window_cx.debug_bounds("explorer-delete-confirm").unwrap();
    window_cx.simulate_click(confirm.center(), Default::default());
    window_cx.run_until_parked();
    assert!(!file.exists(), "confirmation removes the selected file");
    assert!(window_cx.update(|_, cx| app.read(cx).explorer_delete.is_none()));
}
