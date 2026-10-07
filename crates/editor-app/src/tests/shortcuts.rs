//! Exercises shortcuts through native application input, visible controls and document effects.

use crate::{EditorApp, theme, typography};
use editor_core::Workspace;
use gpui_kit::{
    AppContext as _, Entity, Focusable, KeyBinding, TestAppContext, VisualTestContext,
    component::Root, gpui, px, size,
};
use std::{path::Path, time::Duration};

/// One application fixture is shared by lookup, binding and real-package scenarios.
pub(crate) fn with_editor(
    cx: &mut TestAppContext,
    dark: bool,
    bindings: Vec<KeyBinding>,
    scenario: impl FnOnce(&mut VisualTestContext, Entity<EditorApp>, &Path),
) {
    let profile = tempfile::tempdir().unwrap();
    with_editor_profile(
        cx,
        dark,
        bindings,
        &profile.path().join("shortcuts.json"),
        scenario,
    );
}

/// Use the production loader with an explicit user profile to verify different workspaces.
pub(super) fn with_editor_profile(
    cx: &mut TestAppContext,
    dark: bool,
    bindings: Vec<KeyBinding>,
    profile: &Path,
    scenario: impl FnOnce(&mut VisualTestContext, Entity<EditorApp>, &Path),
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        theme::apply_theme(theme::builtin_theme(dark), cx);
        cx.set_reduce_motion(true);
        crate::app::shortcuts::init(cx);
        cx.bind_keys(bindings);
        crate::app::shortcuts::bootstrap::configure(cx, profile.to_path_buf()).unwrap();
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("shortcuts.txt");
    std::fs::write(&path, "original on disk").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let mut application = None;
    let document = path.clone();
    let (_, visual) = cx.add_window_view(|window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, Some(document), window, cx));
        application = Some(app.clone());
        Root::new(app, window, cx)
    });
    let application = application.unwrap();
    visual.simulate_resize(size(px(1100.), px(850.)));
    visual.update(|window, cx| {
        application
            .read(cx)
            .editor
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
        window.draw(cx).clear(cx);
    });
    scenario(visual, application, &path);
}

/// Refresh the rendered input tree after a user gesture.
pub(super) fn draw(visual: &mut VisualTestContext) {
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
}

/// Click the actual Base control hit region, including its focus and disabled behavior.
pub(super) fn click(visual: &mut VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    visual.simulate_click(bounds.center(), Default::default());
    draw(visual);
}

/// Opening from keys, the pointer, or the application menu retains the original editing target.
#[gpui::test]
fn shortcuts_open_and_restore_focus(cx: &mut TestAppContext) {
    with_editor(cx, false, vec![], |visual, app, _| {
        visual.simulate_keystrokes("ctrl-k");
        draw(visual);
        assert!(visual.debug_bounds("shortcuts-panel").is_some());
        assert_eq!(visual.update(|_, cx| cx.windows().len()), 1);
        // Base focus traversal remains available after the search field loses focus.
        click(visual, "shortcuts-tab-0");
        let tab_focus = visual.update(|window, cx| window.focused(cx));
        visual.simulate_keystrokes("tab");
        draw(visual);
        assert_ne!(visual.update(|window, cx| window.focused(cx)), tab_focus);
        click(visual, "shortcuts-tab-0");
        visual.simulate_keystrokes("shift-tab");
        draw(visual);
        assert_ne!(visual.update(|window, cx| window.focused(cx)), tab_focus);
        visual.simulate_keystrokes("escape");
        draw(visual);
        assert!(visual.debug_bounds("shortcuts-panel").is_none());
        assert!(visual.update(|window, cx| {
            app.read(cx)
                .editor
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        }));
        click(visual, "shortcuts-trigger");
        visual.simulate_input(&rust_i18n::t!("shortcuts.operation.Copy"));
        draw(visual);
        assert!(
            visual
                .debug_bounds("shortcut-operation-input::Copy")
                .is_some()
        );
        visual.simulate_keystrokes("escape");
        draw(visual);
        assert!(visual.update(|window, cx| {
            app.read(cx)
                .editor
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        }));
        click(visual, "shortcuts-menu-trigger");
        click(visual, "native-menu-item-shortcuts");
        visual.simulate_input(&rust_i18n::t!("shortcuts.operation.Copy"));
        draw(visual);
        assert!(
            visual
                .debug_bounds("shortcut-operation-input::Copy")
                .is_some()
        );
        visual.simulate_keystrokes("escape");
        draw(visual);
        assert!(visual.update(|window, cx| {
            app.read(cx)
                .editor
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        }));
    });
}

/// Search preserves its condition between scopes; recording never saves a dirty document.
#[gpui::test]
fn shortcuts_search_and_capture_without_running_commands(cx: &mut TestAppContext) {
    with_editor(cx, false, vec![], |visual, _, path| {
        visual.simulate_input("dirty ");
        visual.simulate_keystrokes("ctrl-k");
        draw(visual);
        let frame = visual.debug_bounds("shortcuts-panel").unwrap();
        assert!(visual.debug_bounds("shortcuts-search").is_some());
        visual.simulate_input(&rust_i18n::t!("shortcuts.operation.SaveDocument"));
        visual.simulate_keystrokes("alt-right");
        draw(visual);
        assert!(
            visual
                .debug_bounds("shortcut-operation-me_editor::SaveDocument")
                .is_some()
        );
        assert_eq!(
            visual.debug_bounds("shortcuts-panel").unwrap().size,
            frame.size
        );
        click(visual, "shortcuts-capture");
        visual.simulate_keystrokes("ctrl-s");
        draw(visual);
        assert!(
            visual
                .debug_bounds("shortcut-operation-me_editor::SaveDocument")
                .is_some()
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), "original on disk");
        visual.simulate_keystrokes("escape");
        draw(visual);
        assert!(visual.debug_bounds("shortcuts-panel").is_some());
        visual.simulate_keystrokes("escape");
        draw(visual);
        assert!(visual.debug_bounds("shortcuts-panel").is_none());
    });
}

/// Two-stroke descriptors filter by prefix/exact modifiers without moving the fixed chrome.
#[gpui::test]
fn shortcuts_two_stroke_query_and_fixed_layout(cx: &mut TestAppContext) {
    with_editor(
        cx,
        true,
        vec![
            KeyBinding::new("ctrl-j ctrl-s", crate::SaveDocument, Some("EditorShell")),
            KeyBinding::new("ctrl-j ctrl-t", crate::ToggleTheme, Some("EditorShell")),
        ],
        |visual, _, _| {
            visual.simulate_keystrokes("ctrl-k alt-right");
            draw(visual);
            click(visual, "shortcuts-capture");
            visual.simulate_keystrokes("ctrl-j");
            draw(visual);
            assert!(
                visual
                    .debug_bounds("shortcut-operation-me_editor::SaveDocument")
                    .is_some()
            );
            assert!(
                visual
                    .debug_bounds("shortcut-operation-me_editor::ToggleTheme")
                    .is_some()
            );
            visual.simulate_keystrokes("ctrl-s");
            draw(visual);
            assert!(
                visual
                    .debug_bounds("shortcut-operation-me_editor::SaveDocument")
                    .is_some()
            );
            assert!(
                visual
                    .debug_bounds("shortcut-operation-me_editor::ToggleTheme")
                    .is_none()
            );
            visual.simulate_keystrokes("ctrl-shift-j");
            draw(visual);
            assert!(visual.debug_bounds("shortcuts-empty").is_some());
            visual.simulate_keystrokes("escape alt-left");
            draw(visual);
            let header = visual.debug_bounds("shortcuts-tab-0").unwrap();
            let footer = visual.debug_bounds("shortcuts-footer").unwrap();
            let list = visual.debug_bounds("shortcuts-list").unwrap();
            visual.simulate_event(gpui_kit::ScrollWheelEvent {
                position: list.center(),
                delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-500.))),
                ..Default::default()
            });
            draw(visual);
            assert_eq!(visual.debug_bounds("shortcuts-tab-0").unwrap(), header);
            assert_eq!(visual.debug_bounds("shortcuts-footer").unwrap(), footer);
            visual.simulate_resize(size(px(580.), px(430.)));
            visual.simulate_scale_factor_change(1.5);
            draw(visual);
            let panel = visual.debug_bounds("shortcuts-panel").unwrap();
            assert!(panel.size.width <= px(548.) && panel.size.height <= px(398.));
            click(visual, "shortcuts-search");
            visual.simulate_input("不存在的中文操作");
            draw(visual);
            assert!(visual.debug_bounds("shortcuts-empty").is_some());
            visual.simulate_click(gpui_kit::point(px(4.), px(100.)), Default::default());
            draw(visual);
            assert!(visual.debug_bounds("shortcuts-panel").is_none());
        },
    );
}

/// New recordings retain their full interval; Alt keys are captured rather than navigating.
#[gpui::test]
fn shortcuts_capture_deadline_unbound_and_original_tree_context(cx: &mut TestAppContext) {
    with_editor(
        cx,
        false,
        vec![
            KeyBinding::new("ctrl-j ctrl-t", crate::ToggleTheme, Some("EditorShell")),
            KeyBinding::new("alt-left", crate::RefreshWorkspace, Some("EditorShell")),
        ],
        |visual, _, _| {
            visual.simulate_keystrokes("ctrl-k");
            visual.simulate_input(&rust_i18n::t!(
                "shortcuts.operation.DeleteToBeginningOfLine"
            ));
            draw(visual);
            assert!(
                visual
                    .debug_bounds("shortcut-operation-input::DeleteToBeginningOfLine")
                    .is_some()
            );
            // Parameterized Enter handlers remain independently discoverable by their labels.
            for variant in ["enter_primary", "enter_shift", "enter_secondary"] {
                visual.simulate_keystrokes("ctrl-a backspace");
                let key = format!("shortcuts.operation.{variant}");
                let title = rust_i18n::t!(key.as_str()).to_string();
                visual.simulate_input(&title);
                draw(visual);
                assert!(
                    visual
                        .debug_bounds("shortcut-operation-input::Enter")
                        .is_some()
                );
            }
            visual.simulate_keystrokes("ctrl-a backspace alt-right");
            draw(visual);
            click(visual, "shortcuts-capture");
            visual.simulate_keystrokes("ctrl-j");
            visual.executor().advance_clock(Duration::from_secs(1));
            visual.simulate_keystrokes("escape");
            draw(visual);
            click(visual, "shortcuts-capture");
            visual.simulate_keystrokes("ctrl-j");
            visual.executor().advance_clock(Duration::from_millis(1500));
            draw(visual);
            visual.simulate_keystrokes("ctrl-t");
            draw(visual);
            assert!(
                visual
                    .debug_bounds("shortcut-operation-me_editor::ToggleTheme")
                    .is_some()
            );
            visual.simulate_keystrokes("alt-left");
            draw(visual);
            assert!(
                visual
                    .debug_bounds("shortcut-operation-me_editor::RefreshWorkspace")
                    .is_some()
            );
            visual.executor().advance_clock(Duration::from_secs(2));
            draw(visual);
            visual.simulate_keystrokes("ctrl-t");
            draw(visual);
            assert!(visual.debug_bounds("shortcuts-empty").is_some());
            visual.simulate_keystrokes("escape escape");
            draw(visual);
            // Select the visible tree row through the same pointer path used by the user.
            click(visual, "explorer-row-0");
            visual.simulate_keystrokes("ctrl-k");
            draw(visual);
            assert!(
                visual
                    .debug_bounds("shortcut-operation-input::Copy")
                    .is_none()
            );
            // Querying the tree does not inherit Input merely because the modal has a search box.
            visual.simulate_input(&rust_i18n::t!("shortcuts.operation.Copy"));
            draw(visual);
            assert!(visual.debug_bounds("shortcuts-empty").is_some());
        },
    );
}
