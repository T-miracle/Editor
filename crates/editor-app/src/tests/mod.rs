//! Exercises editor shell interactions through GPUI's test context.

mod editing_fixture;
mod language_editing;
mod language_fixture;
mod outline;
mod run_file_tabs;
mod tag_editing;
mod xml_formatting;
mod xml_language_tools;
pub(crate) use language_fixture::declared_language_service;

#[cfg(test)]
mod settings_dialog_tests {
    use crate::theme::{apply_theme, builtin_theme};
    use crate::{EditorApp, PANEL_HEADER_HEIGHT, typography};
    use editor_core::Workspace;
    use gpui_kit::{
        AppContext as _, TestAppContext, VisualTestContext, component::Root, gpui, px, size,
    };
    use std::{cell::RefCell, rc::Rc};

    /// Settings must use a separate modal window and close with Escape.
    #[gpui::test]
    fn settings_button_opens_modal_window(cx: &mut TestAppContext) {
        let app_cx = cx;
        app_cx.update(|cx| {
            gpui_kit::init(cx);
            typography::init(cx);
            apply_theme(builtin_theme(false), cx);
            cx.set_reduce_motion(true);
        });
        let directory = tempfile::tempdir().unwrap();
        // Row-height checks require an open document; empty workspaces render a canvas.
        let path = directory.path().join("settings.txt");
        std::fs::write(&path, "settings layout").unwrap();
        let workspace = Workspace::open(directory.path()).unwrap();
        let view_slot = Rc::new(RefCell::new(None));
        let capture = view_slot.clone();
        let (_, editor_cx) = app_cx.add_window_view(move |window, cx| {
            let view = cx.new(|cx| EditorApp::new(workspace, Some(path), window, cx));
            *capture.borrow_mut() = Some(view.clone());
            Root::new(view, window, cx)
        });
        let editor_view = view_slot.borrow_mut().take().unwrap();
        let editor_window = editor_cx.update(|window, _| window.window_handle());
        editor_cx.simulate_resize(size(px(1000.), px(800.)));
        editor_cx.update(|window, cx| window.draw(cx).clear(cx));
        // Opening a separate window must not change the editor's measured row height.
        let line_height_before = editor_cx.update(|_, cx| {
            editor_view
                .read(cx)
                .editor
                .read(cx)
                .line_height()
                .expect("editor layout should be measured before opening settings")
        });

        let button = editor_cx
            .debug_bounds("settings-trigger")
            .expect("settings button should be visible in the title bar");
        editor_cx.simulate_click(button.center(), Default::default());
        editor_cx.run_until_parked();
        editor_cx.update(|window, cx| window.draw(cx).clear(cx));
        let line_height_after = editor_cx.update(|_, cx| {
            editor_view
                .read(cx)
                .editor
                .read(cx)
                .line_height()
                .expect("editor layout should remain measured with settings open")
        });
        assert_eq!(line_height_after, line_height_before);

        assert!(editor_cx.debug_bounds("dialog-0").is_none());
        let windows = editor_cx.update(|_, cx| cx.windows());
        assert_eq!(windows.len(), 2);
        let dialog_window = windows
            .into_iter()
            .find(|handle| *handle != editor_window)
            .expect("settings must have a separate window");
        let dialog_cx = VisualTestContext::from_window(dialog_window, app_cx).into_mut();
        dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            dialog_cx.debug_bounds("dialog-0").is_some(),
            "the settings window must render its own content"
        );
        // The explorer preference starts disabled and its visible checkbox writes the saved choice.
        assert!(!dialog_cx.update(|_, cx| {
            editor_view
                .read(cx)
                .session_state
                .explorer_reveal_on_tab_switch
        }));
        let reveal = dialog_cx.debug_bounds("settings-explorer-reveal").unwrap();
        dialog_cx.simulate_click(reveal.center(), Default::default());
        dialog_cx.run_until_parked();
        dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(dialog_cx.update(|_, cx| {
            let app = editor_view.read(cx);
            crate::SessionState::load(app.workspace.root()).explorer_reveal_on_tab_switch
        }));
        // Toggling off must take effect without reopening either window.
        dialog_cx.simulate_click(reveal.center(), Default::default());
        dialog_cx.run_until_parked();
        dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(!dialog_cx.update(|_, cx| {
            editor_view
                .read(cx)
                .session_state
                .explorer_reveal_on_tab_switch
        }));
        let title = dialog_cx
            .debug_bounds("app-dialog-title-bar")
            .expect("the dialog must render a title bar");
        assert_eq!(title.size.height, px(PANEL_HEADER_HEIGHT));
        let keymap = dialog_cx
            .debug_bounds("settings-nav-keymap")
            .expect("the settings sidebar must show Keymap");
        dialog_cx.simulate_click(keymap.center(), Default::default());
        dialog_cx.run_until_parked();
        dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            dialog_cx.debug_bounds("settings-keymap-empty").is_some(),
            "selecting Keymap must replace the settings page"
        );
        let editor = dialog_cx
            .debug_bounds("settings-nav-editor")
            .expect("the settings sidebar must show Editor");
        dialog_cx.simulate_click(editor.center(), Default::default());
        dialog_cx.run_until_parked();
        dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            dialog_cx.debug_bounds("settings-font-size").is_some(),
            "selecting Editor must show the existing font size control"
        );
        dialog_cx.simulate_keystrokes("escape");
        dialog_cx.run_until_parked();
        assert_eq!(app_cx.windows().len(), 1);
        assert!(app_cx.read(|cx| editor_view.read(cx).dialog.is_none()));

        let editor_cx = VisualTestContext::from_window(editor_window, app_cx).into_mut();
        editor_cx.update(|window, cx| window.draw(cx).clear(cx));
        let button = editor_cx.debug_bounds("settings-trigger").unwrap();
        editor_cx.simulate_click(button.center(), Default::default());
        editor_cx.run_until_parked();
        assert_eq!(
            editor_cx.update(|_, cx| cx.windows().len()),
            2,
            "closing the modal window must allow it to reopen"
        );
    }

    /// Plugin management renders its sidebar, detail pane and underline tab strip in a modal.
    #[gpui::test]
    fn plugin_manager_renders_two_panes(cx: &mut TestAppContext) {
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
        let (_, editor_cx) = cx.add_window_view(move |window, cx| {
            let view = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
            *capture.borrow_mut() = Some(view.clone());
            Root::new(view, window, cx)
        });
        let editor = slot.borrow_mut().take().unwrap();
        let editor_window = editor_cx.update(|window, _| window.window_handle());
        editor_cx
            .update(|window, cx| editor.update(cx, |app, cx| app.toggle_extensions(window, cx)));
        let dialog_window = editor_cx
            .update(|_, cx| cx.windows())
            .into_iter()
            .find(|handle| *handle != editor_window)
            .unwrap();
        let dialog_cx = VisualTestContext::from_window(dialog_window, cx).into_mut();
        dialog_cx.run_until_parked();
        dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(dialog_cx.debug_bounds("dialog-0").is_some());
        assert!(dialog_cx.debug_bounds("runtime-plugin-manager").is_some());
        assert!(dialog_cx.debug_bounds("plugin-manager-sidebar").is_some());
        assert!(dialog_cx.debug_bounds("plugin-manager-detail").is_some());
        assert!(dialog_cx.debug_bounds("plugin-manager-tab-strip").is_some());
    }
}

#[cfg(test)]
mod file_highlight_tests {
    use crate::{
        EditorApp,
        theme::{apply_theme, builtin_theme},
        typography,
    };
    use editor_core::Workspace;
    use gpui_kit::{AppContext as _, TestAppContext, component::Root, gpui};
    use std::{cell::RefCell, rc::Rc};

    /// File contents must be visible before language highlighting begins after the first frame.
    #[gpui::test]
    fn highlights_only_after_loaded_file_is_rendered(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            typography::init(cx);
            apply_theme(builtin_theme(false), cx);
            cx.set_reduce_motion(true);
        });
        let directory = tempfile::tempdir().unwrap();
        // Install an independent declaration; editor defaults must never supply a hidden parser.
        let path = directory.path().join("example.novel");
        std::fs::write(&path, "answer = 42\n").unwrap();
        let workspace = Workspace::open(directory.path()).unwrap();
        let mut manager = plugin_runtime::Manager::open(
            workspace.root().join(".runtime-plugin-test"),
            plugin_runtime::plugin_protocol::Environment {
                workspace: workspace.root().display().to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        let package =
            crate::extensions::language_tests::packages::language_package("frame-fixture");
        manager.install(&package, Default::default()).unwrap();
        let view_slot = Rc::new(RefCell::new(None));
        let capture = view_slot.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let view = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
            *capture.borrow_mut() = Some(view.clone());
            Root::new(view, window, cx)
        });
        let view = view_slot.borrow_mut().take().unwrap();
        // Finish package startup before observing the separate file-open frame boundary.
        cx.run_until_parked();
        cx.update(|window, cx| view.update(cx, |app, cx| app.open_file(path, window, cx)));
        cx.update(|_, cx| {
            let app = view.read(cx);
            let editor = app.editor.read(cx);
            assert_eq!(editor.text().to_string(), "answer = 42\n");
            assert_eq!(editor.language_name().as_ref(), "text");
        });

        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(app.editor.read(cx).language_name().as_ref(), "text");
        });
        cx.update(|window, cx| window.simulate_next_frame(cx));
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(app.editor.read(cx).language_name().as_ref(), "novel");
        });
    }
}

#[cfg(test)]
mod explorer_selection_tests {
    use crate::{
        EditorApp,
        theme::{apply_theme, builtin_theme},
        typography,
    };
    use editor_core::Workspace;
    use gpui_kit::{AppContext as _, TestAppContext, component::Root, gpui};
    use std::{cell::RefCell, rc::Rc};

    /// Selection uses the tree already displayed, even if the disk changes before refresh.
    #[gpui::test]
    fn selecting_a_file_does_not_rescan_the_workspace(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            typography::init(cx);
            apply_theme(builtin_theme(false), cx);
            cx.set_reduce_motion(true);
        });
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.txt");
        let second = directory.path().join("second.txt");
        std::fs::write(&first, "first").unwrap();
        std::fs::write(&second, "second").unwrap();
        let workspace = Workspace::open(directory.path()).unwrap();
        let first = first.canonicalize().unwrap();
        let second = second.canonicalize().unwrap();
        let initial_first = first.clone();
        let view_slot = Rc::new(RefCell::new(None));
        let capture = view_slot.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let view = cx.new(|cx| EditorApp::new(workspace, Some(initial_first), window, cx));
            *capture.borrow_mut() = Some(view.clone());
            Root::new(view, window, cx)
        });
        let view = view_slot.borrow_mut().take().unwrap();

        cx.update(|window, cx| {
            view.update(cx, |app, cx| app.open_file(second.clone(), window, cx));
            let app = view.read(cx);
            assert_eq!(app.active_path.as_deref(), Some(second.as_path()));
            assert_eq!(app.editor.read(cx).text().to_string(), "second");
            view.update(cx, |app, cx| app.select_file_in_tree(&first, cx));
        });
        std::fs::remove_file(&second).unwrap();
        cx.update(|_, cx| {
            view.update(cx, |app, cx| app.select_file_in_tree(&second, cx));
            let app = view.read(cx);
            assert_eq!(
                app.tree_state
                    .read(cx)
                    .selected_item()
                    .map(|item| item.id.as_str()),
                Some(second.to_str().unwrap())
            );
        });
    }

    /// Tab activation reveals project files and retains their selection for external tabs.
    #[gpui::test]
    fn tab_switches_follow_project_files_and_preserve_selection_for_external_files(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            typography::init(cx);
            apply_theme(builtin_theme(false), cx);
            cx.set_reduce_motion(true);
        });
        let directory = tempfile::tempdir().unwrap();
        let external_directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.txt");
        let nested = directory.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        let second = nested.join("second.txt");
        let external = external_directory.path().join("external.txt");
        std::fs::write(&first, "first").unwrap();
        std::fs::write(&second, "second").unwrap();
        std::fs::write(&external, "external").unwrap();
        let workspace = Workspace::open(directory.path()).unwrap();
        let first = first.canonicalize().unwrap();
        let second = second.canonicalize().unwrap();
        let external = external.canonicalize().unwrap();
        let initial_first = first.clone();
        let view_slot = Rc::new(RefCell::new(None));
        let capture = view_slot.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let view = cx.new(|cx| EditorApp::new(workspace, Some(initial_first), window, cx));
            *capture.borrow_mut() = Some(view.clone());
            Root::new(view, window, cx)
        });
        let view = view_slot.borrow_mut().take().unwrap();

        cx.update(|window, cx| {
            view.update(cx, |app, cx| app.open_file(second.clone(), window, cx));
            let app = view.read(cx);
            let tree = app.tree_state.read(cx);
            assert_eq!(
                tree.selected_item().map(|item| item.id.as_str()),
                second.to_str()
            );
            let selected_index = tree.selected_index().unwrap();
            assert_eq!(
                tree.scroll_handle()
                    .0
                    .borrow()
                    .deferred_scroll_to_item
                    .as_ref()
                    .map(|scroll| scroll.item_index),
                Some(selected_index)
            );
        });
        cx.update(|window, cx| {
            view.update(cx, |app, cx| app.open_file(external.clone(), window, cx));
            let app = view.read(cx);
            assert_eq!(app.active_path.as_deref(), Some(external.as_path()));
            assert_eq!(
                app.tree_state
                    .read(cx)
                    .selected_item()
                    .map(|item| item.id.as_str()),
                second.to_str()
            );
        });
        cx.update(|_, cx| {
            view.update(cx, |app, cx| app.refresh_files(cx));
            let app = view.read(cx);
            assert_eq!(
                app.tree_state
                    .read(cx)
                    .selected_item()
                    .map(|item| item.id.as_str()),
                second.to_str()
            );
        });
        cx.update(|window, cx| {
            // Reopening an existing tab must use the same tree synchronization path.
            view.update(cx, |app, cx| app.open_file(first.clone(), window, cx));
            let app = view.read(cx);
            assert_eq!(
                app.tree_state
                    .read(cx)
                    .selected_item()
                    .map(|item| item.id.as_str()),
                first.to_str()
            );
        });
    }
}
