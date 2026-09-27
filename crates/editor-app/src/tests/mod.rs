//! Exercises editor shell interactions through GPUI's test context.

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
        let workspace = Workspace::open(directory.path()).unwrap();
        let view_slot = Rc::new(RefCell::new(None));
        let capture = view_slot.clone();
        let (_, editor_cx) = app_cx.add_window_view(move |window, cx| {
            let view = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
            *capture.borrow_mut() = Some(view.clone());
            Root::new(view, window, cx)
        });
        let editor_view = view_slot.borrow_mut().take().unwrap();
        let editor_window = editor_cx.update(|window, _| window.window_handle());
        editor_cx.simulate_resize(size(px(1000.), px(800.)));
        editor_cx.update(|window, cx| window.draw(cx).clear(cx));

        let button = editor_cx
            .debug_bounds("settings-trigger")
            .expect("settings button should be visible in the title bar");
        editor_cx.simulate_click(button.center(), Default::default());
        editor_cx.run_until_parked();
        editor_cx.update(|window, cx| window.draw(cx).clear(cx));

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
        let path = directory.path().join("example.js");
        std::fs::write(&path, "const loaded = true;\n").unwrap();
        let workspace = Workspace::open(directory.path()).unwrap();
        let view_slot = Rc::new(RefCell::new(None));
        let capture = view_slot.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let view = cx.new(|cx| EditorApp::new(workspace, Some(path), window, cx));
            *capture.borrow_mut() = Some(view.clone());
            Root::new(view, window, cx)
        });
        let view = view_slot.borrow_mut().take().unwrap();

        cx.update(|_, cx| {
            let app = view.read(cx);
            let editor = app.tabs[0].editor.read(cx);
            assert_eq!(editor.text().to_string(), "const loaded = true;\n");
            assert_eq!(editor.language_name().as_ref(), "text");
        });

        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(app.tabs[0].editor.read(cx).language_name().as_ref(), "text");
        });
        cx.update(|window, cx| window.simulate_next_frame(cx));
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(
                app.tabs[0].editor.read(cx).language_name().as_ref(),
                "javascript"
            );
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
