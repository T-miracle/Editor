//! Exercises pointer gestures through real tree rows rather than calling handlers directly.

use super::*;
use gpui_kit::component::WindowExt as _;
use gpui_kit::{TestAppContext, VisualTestContext, component::Root, gpui};
use std::cell::RefCell;

/// Mount the editor, optionally requesting an initial file instead of the normal session restore.
fn mount<'a>(
    cx: &'a mut TestAppContext,
    workspace: Workspace,
    initial: impl Into<Option<PathBuf>>,
) -> (Entity<EditorApp>, &'a mut VisualTestContext) {
    let initial = initial.into();
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, window_cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, initial, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    window_cx.simulate_resize(size(px(1000.), px(700.)));
    redraw(window_cx);
    (app, window_cx)
}

/// Flush subscription events and layout before looking up a row's bounds.
fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// The test platform's normal click helper always emits count one.
fn double_click(cx: &mut VisualTestContext, position: Point<Pixels>) {
    cx.simulate_event(MouseDownEvent {
        position,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
        click_count: 2,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
        click_count: 2,
    });
    redraw(cx);
}

/// Capture hidden nodes too so reopening an active document cannot silently expand them.
fn expansion_state(app: &EditorApp, cx: &App) -> Vec<(String, bool)> {
    fn visit(items: &[TreeItem], states: &mut Vec<(String, bool)>) {
        for item in items {
            if Path::new(item.id.as_str()).is_dir() {
                states.push((item.id.to_string(), item.is_expanded()));
            }
            visit(&item.children, states);
        }
    }
    let mut states = Vec::new();
    visit(&root_items(app.tree_state.read(cx)), &mut states);
    states
}

/// Save with a document inside a collapsed subtree, then exercise the real startup path.
fn assert_collapsed_tree_survives_restart(cx: &mut TestAppContext, collapse_root: bool) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("a-dir/b-sub")).unwrap();
    std::fs::write(directory.path().join("a-dir/b-sub/child.txt"), "active").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let root = workspace.root().to_path_buf();
    let initial = root.join("a-dir/b-sub/child.txt");
    let (app, visual) = mount(cx, workspace.clone(), initial.clone());
    let arrow = if collapse_root {
        "explorer-disclosure-0"
    } else {
        "explorer-disclosure-1"
    };
    let position = visual.debug_bounds(arrow).unwrap().center();
    visual.simulate_click(position, Modifiers::default());
    redraw(visual);
    let expected = visual.update(|_, cx| {
        app.update(cx, |app, cx| {
            // Restoration must preserve collapse even when normal tab switches are allowed to reveal.
            app.session_state.explorer_reveal_on_tab_switch = true;
            app.capture_explorer_state(cx);
            app.persist_session();
            expansion_state(app, cx)
        })
    });
    assert_eq!(
        SessionState::load(&root).explorer_root_expanded,
        !collapse_root
    );

    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let restored = slot.borrow_mut().take().unwrap();
    redraw(visual);
    visual.update(|_, cx| {
        let app = restored.read(cx);
        assert_eq!(app.active_path.as_ref(), Some(&initial));
        assert_eq!(app.editor.read(cx).value().to_string(), "active");
        assert_eq!(
            expansion_state(app, cx),
            expected,
            "restart must preserve collapsed directories"
        );
    });
    // A filesystem update after restart must not reveal the active file either.
    std::fs::write(directory.path().join("new.txt"), "added externally").unwrap();
    visual.update(|_, cx| restored.update(cx, |app, cx| app.refresh_files(cx)));
    redraw(visual);
    visual.update(|_, cx| {
        assert_eq!(expansion_state(restored.read(cx), cx), expected);
    });
}

#[gpui::test]
fn explorer_row_content_is_vertically_centered(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("文件.txt"), "content").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let (_, visual) = mount(cx, workspace, None);
    for dark in [false, true] {
        for font_size in [14., 20.] {
            visual.update(|_, cx| {
                apply_theme(builtin_theme(dark), cx);
                typography::set_font_size(cx, font_size);
            });
            redraw(visual);
            // Compare actual laid-out boxes for both folder and file rows at different text sizes.
            for (row_selector, parts) in [
                (
                    "explorer-row-0",
                    [
                        "explorer-disclosure-0",
                        "explorer-icon-0",
                        "explorer-label-0",
                    ],
                ),
                (
                    "explorer-row-1",
                    [
                        "explorer-disclosure-1",
                        "explorer-icon-1",
                        "explorer-label-1",
                    ],
                ),
            ] {
                let row = visual.debug_bounds(row_selector).unwrap();
                for part in parts {
                    let bounds = visual.debug_bounds(part).unwrap();
                    assert!(
                        (bounds.center().y - row.center().y).abs() < px(0.6),
                        "{part} must be centered in {row_selector}"
                    );
                }
                let icon = visual.debug_bounds(parts[1]).unwrap();
                assert_eq!(icon.size, size(px(16.), px(16.)));
            }
        }
    }
}

#[gpui::test]
fn header_expansion_actions_include_hidden_and_empty_directories(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("nested/deep")).unwrap();
    std::fs::create_dir(directory.path().join("empty")).unwrap();
    std::fs::write(directory.path().join("nested/deep/active.txt"), "active").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let root = workspace.root().to_path_buf();
    let active = root.join("nested/deep/active.txt");
    let (app, visual) = mount(cx, workspace, active.clone());
    for dark in [false, true] {
        visual.update(|_, cx| apply_theme(builtin_theme(dark), cx));
        redraw(visual);
        for (selector, expanded) in [
            ("explorer-collapse-all", false),
            ("explorer-expand-all", true),
        ] {
            // Exercise the real header action with a selected file below multiple ancestors.
            let button = visual.debug_bounds(selector).unwrap().center();
            visual.simulate_click(button, Modifiers::default());
            redraw(visual);
            visual.update(|_, cx| {
                let app = app.read(cx);
                let states = expansion_state(app, cx);
                assert_eq!(
                    states.len(),
                    4,
                    "include root, deep directories and the empty folder"
                );
                assert!(states.iter().all(|(_, value)| *value == expanded));
                assert_eq!(app.active_path.as_ref(), Some(&active));
                assert_eq!(app.editor.read(cx).value().to_string(), "active");
                let saved = SessionState::load(&root);
                assert_eq!(saved.explorer_root_expanded, expanded);
                assert_eq!(
                    saved.expanded_directories.len(),
                    if expanded { 4 } else { 0 }
                );
                if !expanded {
                    assert!(app.tree_state.read(cx).entry(1).is_none());
                }
            });
        }
    }
}

#[gpui::test]
fn collapsed_project_root_survives_restart(cx: &mut TestAppContext) {
    assert_collapsed_tree_survives_restart(cx, true);
}

#[gpui::test]
fn reveal_header_button_expands_and_selects_active_file(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("a-dir/b-sub")).unwrap();
    std::fs::write(directory.path().join("a-dir/b-sub/child.txt"), "active").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let root = workspace.root().to_path_buf();
    let active = root.join("a-dir/b-sub/child.txt");
    let (app, visual) = mount(cx, workspace, active.clone());
    for dark in [false, true] {
        visual.update(|_, cx| {
            apply_theme(builtin_theme(dark), cx);
            app.update(cx, |app, cx| {
                // Manual reveal must work even with the project root collapsed and auto reveal disabled.
                assert!(!app.session_state.explorer_reveal_on_tab_switch);
                app.toggle_explorer_directory(&root, cx);
            });
        });
        redraw(visual);
        let button = visual
            .debug_bounds("explorer-reveal-active-file")
            .unwrap()
            .center();
        visual.simulate_click(button, Modifiers::default());
        redraw(visual);
        visual.update(|window, cx| {
            let app = app.read(cx);
            assert!(
                expansion_state(app, cx)
                    .iter()
                    .all(|(_, expanded)| *expanded)
            );
            assert_eq!(
                Path::new(app.tree_state.read(cx).selected_item().unwrap().id.as_str()),
                active
            );
            assert_eq!(app.active_path.as_ref(), Some(&active));
            assert_eq!(app.editor.read(cx).value().to_string(), "active");
            assert!(!window.has_active_dialog(cx));
        });
        assert!(visual.debug_bounds("explorer-row-3").is_some());
    }
}

#[gpui::test]
fn reveal_external_file_shows_nonmodal_notification(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let external_directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("project.txt"), "project").unwrap();
    let external = external_directory.path().join("external.txt");
    std::fs::write(&external, "external").unwrap();
    let external = external.canonicalize().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let initial = workspace.root().join("project.txt");
    let (app, visual) = mount(cx, workspace, initial);
    visual.update(|window, cx| {
        app.update(cx, |app, cx| app.open_file(external.clone(), window, cx));
    });
    redraw(visual);
    let before = visual.update(|_, cx| expansion_state(app.read(cx), cx));
    let button = visual
        .debug_bounds("explorer-reveal-active-file")
        .unwrap()
        .center();
    visual.simulate_click(button, Modifiers::default());
    redraw(visual);
    assert!(visual.debug_bounds("local-notification").is_some());
    assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
    visual.update(|_, cx| assert_eq!(expansion_state(app.read(cx), cx), before));

    // The notification must allow normal interactions in the other panels.
    let arrow = visual
        .debug_bounds("explorer-disclosure-0")
        .unwrap()
        .center();
    visual.simulate_click(arrow, Modifiers::default());
    redraw(visual);
    visual.update(|_, cx| {
        let app = app.read(cx);
        assert_eq!(app.active_path.as_ref(), Some(&external));
        assert_ne!(expansion_state(app, cx), before);
    });
    let confirm = visual
        .debug_bounds("local-notification-close")
        .unwrap()
        .center();
    visual.simulate_click(confirm, Modifiers::default());
    redraw(visual);
    assert!(visual.debug_bounds("local-notification").is_none());
    assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));

    // A replacement card must survive the old card's timeout and expire on its own timer.
    visual.simulate_click(button, Modifiers::default());
    redraw(visual);
    visual
        .executor()
        .advance_clock(std::time::Duration::from_millis(600));
    redraw(visual);
    visual.simulate_click(button, Modifiers::default());
    redraw(visual);
    visual
        .executor()
        .advance_clock(std::time::Duration::from_millis(600));
    redraw(visual);
    assert!(visual.debug_bounds("local-notification").is_some());
    visual
        .executor()
        .advance_clock(std::time::Duration::from_millis(600));
    redraw(visual);
    assert!(visual.debug_bounds("local-notification").is_none());
}

#[gpui::test]
fn collapsed_active_file_ancestors_survive_restart(cx: &mut TestAppContext) {
    assert_collapsed_tree_survives_restart(cx, false);
}

#[gpui::test]
fn collapsed_root_survives_startup_without_saved_tabs(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("nested")).unwrap();
    std::fs::write(directory.path().join("nested/child.txt"), "fallback").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let mut saved = SessionState::for_workspace(workspace.root());
    saved.explorer_root_expanded = false;
    saved.save();
    // Opening the startup fallback file must honor the saved tree, just like restoring a tab.
    let (app, visual) = mount(cx, workspace, None);
    visual.update(|_, cx| {
        assert!(
            expansion_state(app.read(cx), cx)
                .iter()
                .all(|(_, expanded)| !expanded)
        );
    });
}

#[gpui::test]
fn exit_snapshot_includes_pending_directory_toggle(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("a-dir/b-sub")).unwrap();
    std::fs::write(directory.path().join("a-dir/b-sub/child.txt"), "active").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let root = workspace.root().to_path_buf();
    let initial = root.join("a-dir/b-sub/child.txt");
    let (app, visual) = mount(cx, workspace, initial);
    visual.update(|_, cx| {
        app.update(cx, |app, cx| {
            // Simulate closing immediately after a toggle, before its TreeEvent subscription runs.
            app.toggle_explorer_directory(&root, cx);
            app.capture_explorer_state(cx);
            app.persist_session();
            let saved = SessionState::load(&root);
            assert!(!saved.explorer_root_expanded);
            assert!(saved.expanded_directories.is_empty());
        });
    });
}

/// Definition navigation preserves explorer state unless tab-switch reveal is enabled.
#[gpui::test]
fn definition_jumps_follow_tab_reveal_preference(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("a-dir")).unwrap();
    std::fs::write(directory.path().join("a-dir/child.txt"), "child").unwrap();
    std::fs::write(directory.path().join("z.txt"), "initial").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let initial = workspace.root().join("z.txt");
    let nested = workspace.root().join("a-dir/child.txt");
    let (app, visual) = mount(cx, workspace, initial.clone());
    let uri = url::Url::from_file_path(&nested).unwrap();
    let uri = uri.as_str().parse::<lsp_types::Uri>().unwrap();
    let before = visual.update(|_, cx| expansion_state(app.read(cx), cx));
    // Check both opening a new target tab and returning to an already-open target.
    for _ in 0..2 {
        visual.update(|window, cx| {
            app.update(cx, |app, cx| {
                assert!(!app.session_state.explorer_reveal_on_tab_switch);
                assert!(app.open_definition_uri(&uri, None, window, cx));
                assert_eq!(app.active_path.as_ref(), Some(&nested));
                assert_eq!(expansion_state(app, cx), before);
                assert_eq!(
                    Path::new(app.tree_state.read(cx).selected_item().unwrap().id.as_str()),
                    initial
                );
                app.activate_tab(0, window, cx);
            });
        });
        redraw(visual);
    }
    // The saved setting is read for every jump, so enabling it works immediately.
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.session_state.explorer_reveal_on_tab_switch = true;
            assert!(app.open_definition_uri(&uri, None, window, cx));
            assert!(app.tree_state.read(cx).entry(1).unwrap().is_expanded());
            assert_eq!(
                Path::new(app.tree_state.read(cx).selected_item().unwrap().id.as_str()),
                nested
            );
        });
    });
}

#[gpui::test]
fn tab_clicks_reveal_only_when_enabled(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("a-dir")).unwrap();
    std::fs::write(directory.path().join("a-dir/child.txt"), "child").unwrap();
    std::fs::write(directory.path().join("z.txt"), "initial").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let initial = workspace.root().join("z.txt");
    let nested = workspace.root().join("a-dir/child.txt");
    let root = workspace.root().to_path_buf();
    let (app, visual) = mount(cx, workspace, initial);
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            assert!(!app.session_state.explorer_reveal_on_tab_switch);
            app.open_file(nested.clone(), window, cx);
        });
    });
    redraw(visual);
    let arrow = visual
        .debug_bounds("explorer-disclosure-1")
        .unwrap()
        .center();
    visual.simulate_click(arrow, Modifiers::default());
    redraw(visual);
    let before = visual.update(|_, cx| expansion_state(app.read(cx), cx));
    for selector in ["editor-tab-0", "editor-tab-1"] {
        let tab = visual.debug_bounds(selector).unwrap().center();
        visual.simulate_click(tab, Modifiers::default());
        redraw(visual);
    }
    visual.update(|_, cx| {
        let app = app.read(cx);
        assert_eq!(app.active_path.as_ref(), Some(&nested));
        assert_eq!(expansion_state(app, cx), before);
        assert_eq!(app.tree_state.read(cx).selected_index(), Some(1));
    });

    // Turning the preference on restores reveal behavior for actual tab-strip clicks.
    visual.update(|_, cx| {
        app.update(cx, |app, _| {
            app.session_state.explorer_reveal_on_tab_switch = true;
            app.persist_session();
        })
    });
    for selector in ["editor-tab-0", "editor-tab-1"] {
        let tab = visual.debug_bounds(selector).unwrap().center();
        visual.simulate_click(tab, Modifiers::default());
        redraw(visual);
    }
    visual.update(|_, cx| {
        let app = app.read(cx);
        assert!(app.tree_state.read(cx).entry(1).unwrap().is_expanded());
        assert_eq!(
            Path::new(app.tree_state.read(cx).selected_item().unwrap().id.as_str()),
            nested
        );
    });
    assert!(SessionState::load(&root).explorer_reveal_on_tab_switch);
}

#[gpui::test]
fn file_single_click_selects_and_double_click_opens(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("a.txt"), "initial").unwrap();
    std::fs::write(directory.path().join("b.txt"), "target").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let initial = workspace.root().join("a.txt");
    let target = workspace.root().join("b.txt");
    let (app, window_cx) = mount(cx, workspace, initial.clone());
    let target_position = window_cx.debug_bounds("explorer-row-2").unwrap().center();

    // Moving over another file must leave both the tree selection and active tab alone.
    window_cx.simulate_mouse_move(target_position, None, Modifiers::default());
    window_cx.update(|_, cx| {
        let app = app.read(cx);
        assert_eq!(app.active_path.as_ref(), Some(&initial));
        assert_eq!(
            Path::new(app.tree_state.read(cx).selected_item().unwrap().id.as_str()),
            initial
        );
    });
    window_cx.simulate_click(target_position, Modifiers::default());
    redraw(window_cx);
    window_cx.update(|_, cx| {
        let app = app.read(cx);
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.active_path.as_ref(), Some(&initial));
        assert_eq!(app.editor.read(cx).value().to_string(), "initial");
        assert_eq!(
            Path::new(app.tree_state.read(cx).selected_item().unwrap().id.as_str()),
            target
        );
    });

    // A changed directory snapshot must preserve selection independently of the open document.
    std::fs::write(directory.path().join("c.txt"), "new file").unwrap();
    window_cx.update(|_, cx| app.update(cx, |app, cx| app.refresh_files(cx)));
    redraw(window_cx);
    window_cx.update(|_, cx| {
        assert_eq!(
            Path::new(
                app.read(cx)
                    .tree_state
                    .read(cx)
                    .selected_item()
                    .unwrap()
                    .id
                    .as_str()
            ),
            target
        );
    });
    double_click(window_cx, target_position);
    window_cx.update(|_, cx| {
        let app = app.read(cx);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active_path.as_ref(), Some(&target));
        assert_eq!(app.editor.read(cx).value().to_string(), "target");
    });

    // Context menu selection also highlights the clicked row without activating its document.
    let initial_position = window_cx.debug_bounds("explorer-row-1").unwrap().center();
    window_cx.simulate_mouse_down(initial_position, MouseButton::Right, Modifiers::default());
    redraw(window_cx);
    window_cx.update(|_, cx| {
        let app = app.read(cx);
        assert!(app.explorer_menu.is_some());
        assert_eq!(app.active_path.as_ref(), Some(&target));
        assert_eq!(
            Path::new(app.tree_state.read(cx).selected_item().unwrap().id.as_str()),
            initial
        );
    });
}

#[gpui::test]
fn folder_gestures_collapse_all_descendants(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("a-dir/b-sub")).unwrap();
    std::fs::write(directory.path().join("a-dir/b-sub/child.txt"), "child").unwrap();
    std::fs::write(directory.path().join("z.txt"), "initial").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let initial = workspace.root().join("z.txt");
    let parent = workspace.root().join("a-dir");
    let child = parent.join("b-sub");
    let (app, window_cx) = mount(cx, workspace, initial);
    let parent_position = window_cx.debug_bounds("explorer-row-1").unwrap().center();
    window_cx.simulate_click(parent_position, Modifiers::default());
    redraw(window_cx);
    window_cx.update(|_, cx| {
        assert!(
            !app.read(cx)
                .tree_state
                .read(cx)
                .entry(1)
                .unwrap()
                .is_expanded()
        );
    });
    double_click(window_cx, parent_position);
    window_cx.update(|_, cx| {
        assert!(
            app.read(cx)
                .tree_state
                .read(cx)
                .entry(1)
                .unwrap()
                .is_expanded()
        );
    });
    let child_arrow = window_cx
        .debug_bounds("explorer-disclosure-2")
        .unwrap()
        .center();
    window_cx.simulate_click(child_arrow, Modifiers::default());
    redraw(window_cx);
    window_cx.update(|_, cx| {
        assert!(
            app.read(cx)
                .tree_state
                .read(cx)
                .entry(2)
                .unwrap()
                .is_expanded()
        );
    });

    // Closing an ancestor resets hidden children and their saved expansion state.
    let parent_arrow = window_cx
        .debug_bounds("explorer-disclosure-1")
        .unwrap()
        .center();
    window_cx.simulate_click(parent_arrow, Modifiers::default());
    redraw(window_cx);
    window_cx.update(|_, cx| {
        let app = app.read(cx);
        let roots = root_items(app.tree_state.read(cx));
        assert!(!find_tree_item(&roots, &parent).unwrap().is_expanded());
        assert!(!find_tree_item(&roots, &child).unwrap().is_expanded());
        assert!(
            !app.session_state
                .expanded_directories
                .iter()
                .any(|path| { Path::new(path).starts_with(&parent) })
        );
    });
    window_cx.simulate_click(parent_arrow, Modifiers::default());
    redraw(window_cx);
    window_cx.update(|_, cx| {
        let app = app.read(cx);
        assert!(app.tree_state.read(cx).entry(1).unwrap().is_expanded());
        assert!(!app.tree_state.read(cx).entry(2).unwrap().is_expanded());
        assert_eq!(app.tabs.len(), 1);
    });
    // A double click on the label uses the same recursive-collapse rule as the arrow.
    double_click(window_cx, parent_position);
    window_cx.update(|_, cx| {
        assert!(
            !app.read(cx)
                .tree_state
                .read(cx)
                .entry(1)
                .unwrap()
                .is_expanded()
        );
    });
}
