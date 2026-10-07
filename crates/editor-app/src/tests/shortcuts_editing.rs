//! User bindings are exercised through native controls, editor input and real saved documents.

use super::shortcuts::{click, draw, second_editor_window, with_editor_profile};
use crate::EditorApp;
use gpui_kit::{
    Entity, KeyBinding, KeyUpEvent, Keystroke, NoAction, TestAppContext, VisualTestContext, gpui,
    px,
};
use std::{path::Path, time::Duration};

/// Narrow the visible list without changing the original document focus captured by the modal.
fn search(visual: &mut VisualTestContext, title: &str) {
    click(visual, "shortcuts-search");
    visual.simulate_keystrokes("ctrl-a backspace");
    visual.simulate_input(title);
    draw(visual);
}

/// Open the global list from the real editor rather than dispatching an internal panel action.
fn open_global(visual: &mut VisualTestContext, title: &str) {
    visual.simulate_keystrokes("ctrl-k alt-right");
    draw(visual);
    search(visual, title);
}

/// A single stroke settles only after the same two-second capture deadline users observe.
fn capture(visual: &mut VisualTestContext, keys: &str) {
    visual.simulate_keystrokes(keys);
    visual.executor().advance_clock(Duration::from_secs(2));
    draw(visual);
}

/// Replace the document through EditorState's real input path, retaining its native undo/session.
fn enter_document(visual: &mut VisualTestContext, text: &str) {
    visual.simulate_keystrokes("ctrl-a");
    visual.simulate_input(text);
    draw(visual);
}

/// Observe dispatch by the durable document, not by a binding engine's private mapping.
fn saves(visual: &mut VisualTestContext, path: &Path, key: &str, text: &str) {
    enter_document(visual, text);
    visual.simulate_keystrokes(key);
    draw(visual);
    assert_eq!(std::fs::read_to_string(path).unwrap(), text);
}

/// Removed bindings cannot save, even though the editor accepts the new in-memory contents.
fn does_not_save(visual: &mut VisualTestContext, path: &Path, key: &str, text: &str) {
    let before = std::fs::read_to_string(path).unwrap();
    enter_document(visual, text);
    visual.simulate_keystrokes(key);
    draw(visual);
    assert_eq!(std::fs::read_to_string(path).unwrap(), before);
    assert_ne!(before, text);
}

/// Read only the public editor text, which is also the source rendered on screen.
fn editor_text(visual: &mut VisualTestContext, app: &Entity<EditorApp>) -> String {
    visual.update(|_, cx| app.read(cx).editor.read(cx).text().to_string())
}

/// Add, edit and delete take effect immediately; restore returns to the original default list.
#[gpui::test]
fn shortcuts_editing_add_edit_delete_restore_dispatch(cx: &mut TestAppContext) {
    let profile = tempfile::tempdir().unwrap();
    let config = profile.path().join("shortcuts.json");
    // A focused button must activate even when the ancestor dialog's Enter action is disabled.
    with_editor_profile(
        cx,
        false,
        vec![KeyBinding::new("enter", NoAction, Some("Dialog"))],
        &config,
        |visual, _, path| {
            let title = rust_i18n::t!("shortcuts.operation.SaveDocument").to_string();
            open_global(visual, &title);
            click(visual, "shortcut-add-me_editor::SaveDocument");
            capture(visual, "ctrl-alt-u");
            // A new draft has nothing stored to delete; the disabled action retains capture and storage.
            click(visual, "shortcuts-edit-delete");
            assert!(visual.debug_bounds("shortcuts-edit-capture").is_some());
            assert!(!config.exists());
            let description = visual
                .debug_bounds("shortcut-operation-me_editor::SaveDocument")
                .unwrap();
            let capture_bounds = visual.debug_bounds("shortcuts-edit-capture").unwrap();
            assert!(capture_bounds.left() > description.right());
            // Native hit regions must remain within the modal and above the fixed footer on small windows.
            visual.simulate_resize(gpui_kit::size(gpui_kit::px(580.), gpui_kit::px(430.)));
            draw(visual);
            let frame = visual.debug_bounds("shortcuts-panel").unwrap();
            let footer = visual.debug_bounds("shortcuts-footer").unwrap();
            for selector in [
                "shortcuts-edit-restore",
                "shortcuts-edit-delete",
                "shortcuts-edit-cancel",
                "shortcuts-edit-save",
            ] {
                let action = visual.debug_bounds(selector).unwrap();
                assert!(action.left() >= frame.left() && action.right() <= frame.right());
                assert!(action.bottom() <= footer.top());
            }
            visual.simulate_resize(gpui_kit::size(gpui_kit::px(1100.), gpui_kit::px(850.)));
            draw(visual);
            // Base buttons activate on release; simulate_keystrokes supplies only the key-down event.
            visual.simulate_keystrokes("enter");
            visual.simulate_event(KeyUpEvent {
                keystroke: Keystroke::parse("enter").unwrap(),
            });
            draw(visual);
            assert!(visual.debug_bounds("shortcuts-edit-capture").is_none());
            assert!(
                visual
                    .debug_bounds("shortcut-binding-me_editor::SaveDocument-1")
                    .is_some()
            );
            visual.simulate_keystrokes("escape");
            draw(visual);
            saves(visual, path, "ctrl-alt-u", "added binding saved");
            saves(visual, path, "ctrl-s", "default remains saved");

            open_global(visual, &title);
            click(visual, "shortcut-binding-me_editor::SaveDocument-1");
            capture(visual, "ctrl-alt-v");
            click(visual, "shortcuts-edit-save");
            visual.simulate_keystrokes("escape");
            draw(visual);
            does_not_save(visual, path, "ctrl-alt-u", "retired added key");
            saves(visual, path, "ctrl-alt-v", "edited binding saved");

            open_global(visual, &title);
            click(visual, "shortcut-binding-me_editor::SaveDocument-0");
            // Delete the original group even if the capture currently contains a different draft key.
            capture(visual, "ctrl-alt-b");
            click(visual, "shortcuts-edit-delete");
            visual.simulate_keystrokes("escape");
            draw(visual);
            does_not_save(visual, path, "ctrl-s", "deleted default key");
            saves(visual, path, "ctrl-alt-v", "sibling remains saved");

            open_global(visual, &title);
            click(visual, "shortcut-binding-me_editor::SaveDocument-0");
            click(visual, "shortcuts-edit-restore");
            visual.simulate_keystrokes("escape");
            draw(visual);
            saves(visual, path, "ctrl-s", "restored default saved");
            does_not_save(visual, path, "ctrl-alt-v", "restored removes override");
        },
    );
}

/// Explicit replacement removes only a colliding sequence, preserving the other operation's keys.
#[gpui::test]
fn shortcuts_editing_conflict_requires_replace_and_preserves_siblings(cx: &mut TestAppContext) {
    let profile = tempfile::tempdir().unwrap();
    let config = profile.path().join("shortcuts.json");
    with_editor_profile(
        cx,
        false,
        vec![
            KeyBinding::new(
                "ctrl-alt-u",
                crate::SaveDocument,
                Some("EditorShell && !PluginSurface"),
            ),
            KeyBinding::new(
                "ctrl-alt-v",
                crate::SaveDocument,
                Some("EditorShell && !PluginSurface"),
            ),
        ],
        &config,
        |visual, app, path| {
            open_global(visual, &rust_i18n::t!("shortcuts.operation.ToggleTheme"));
            click(visual, "shortcut-binding-me_editor::ToggleTheme-0");
            capture(visual, "ctrl-alt-u");
            click(visual, "shortcuts-edit-save");
            assert!(visual.debug_bounds("shortcuts-edit-confirmation").is_some());
            click(visual, "shortcuts-edit-continue");
            assert!(visual.debug_bounds("shortcuts-edit-capture").is_some());
            assert!(visual.debug_bounds("shortcuts-edit-confirmation").is_none());
            click(visual, "shortcuts-edit-save");
            click(visual, "shortcuts-edit-replace");
            assert!(visual.debug_bounds("shortcuts-edit-confirmation").is_none());
            visual.simulate_keystrokes("escape");
            draw(visual);

            let initial_theme = visual.update(|_, cx| app.read(cx).dark_theme);
            does_not_save(visual, path, "ctrl-alt-u", "conflict now changes theme");
            assert_ne!(
                visual.update(|_, cx| app.read(cx).dark_theme),
                initial_theme
            );
            saves(visual, path, "ctrl-alt-v", "unrelated sibling saves");
            saves(visual, path, "ctrl-s", "original default still saves");

            // Inline restoration uses the original collision review, rather than stealing a default key.
            open_global(visual, &rust_i18n::t!("shortcuts.operation.SaveDocument"));
            click(visual, "shortcut-add-me_editor::SaveDocument");
            capture(visual, "ctrl-alt-t");
            click(visual, "shortcuts-edit-save");
            visual.simulate_keystrokes("escape");
            draw(visual);
            saves(
                visual,
                path,
                "ctrl-alt-t",
                "save now owns the theme default",
            );
            open_global(visual, &rust_i18n::t!("shortcuts.operation.ToggleTheme"));
            click(visual, "shortcut-binding-me_editor::ToggleTheme-0");
            let before_restore = std::fs::read(&config).unwrap();
            click(visual, "shortcuts-edit-restore");
            assert!(visual.debug_bounds("shortcuts-edit-confirmation").is_some());
            click(visual, "shortcuts-edit-continue");
            assert_eq!(std::fs::read(&config).unwrap(), before_restore);
            assert!(visual.debug_bounds("shortcuts-edit-capture").is_some());
            click(visual, "shortcuts-edit-restore");
            // A second window may save while the first reviews conflicts; old consent cannot
            // overwrite its newer binding, even if re-preview would otherwise permit restoration.
            let directory = tempfile::tempdir().unwrap();
            let (_, mut other) = second_editor_window(visual, directory.path());
            open_global(
                &mut other,
                &rust_i18n::t!("shortcuts.operation.ToggleTheme"),
            );
            click(&mut other, "shortcut-binding-me_editor::ToggleTheme-0");
            capture(&mut other, "ctrl-alt-b");
            click(&mut other, "shortcuts-edit-save");
            let concurrent_config = std::fs::read(&config).unwrap();
            click(visual, "shortcuts-edit-replace");
            assert_eq!(std::fs::read(&config).unwrap(), concurrent_config);
            assert!(visual.debug_bounds("shortcuts-edit-capture").is_some());
            assert!(visual.debug_bounds("shortcuts-edit-error").is_some());
            assert!(visual.debug_bounds("shortcuts-edit-confirmation").is_none());
            click(visual, "shortcuts-edit-cancel");
            click(visual, "shortcut-binding-me_editor::ToggleTheme-0");
            click(visual, "shortcuts-edit-restore");
            click(visual, "shortcuts-edit-replace");
            visual.simulate_keystrokes("escape");
            draw(visual);
            let previous_theme = visual.update(|_, cx| app.read(cx).dark_theme);
            does_not_save(
                visual,
                path,
                "ctrl-alt-t",
                "restored theme key does not save",
            );
            assert_ne!(
                visual.update(|_, cx| app.read(cx).dark_theme),
                previous_theme
            );
            saves(
                visual,
                path,
                "ctrl-alt-v",
                "restore replacement retains sibling save",
            );
        },
    );
}

/// Navigation asks before discarding a draft, while Escape cancels the current decision layer.
#[gpui::test]
fn shortcuts_editing_unsaved_navigation_and_escape_do_not_persist(cx: &mut TestAppContext) {
    let profile = tempfile::tempdir().unwrap();
    let config = profile.path().join("shortcuts.json");
    with_editor_profile(cx, false, vec![], &config, |visual, _, _| {
        let save = rust_i18n::t!("shortcuts.operation.SaveDocument").to_string();
        open_global(visual, &save);
        click(visual, "shortcut-add-me_editor::SaveDocument");
        capture(visual, "ctrl-alt-u");
        click(visual, "shortcuts-tab-0");
        assert!(visual.debug_bounds("shortcuts-edit-confirmation").is_some());
        click(visual, "shortcuts-edit-continue");
        assert!(visual.debug_bounds("shortcuts-edit-capture").is_some());

        // A backdrop close is guarded just like tab navigation; Esc dismisses the prompt first.
        visual.simulate_click(
            gpui_kit::point(gpui_kit::px(4.), gpui_kit::px(100.)),
            Default::default(),
        );
        draw(visual);
        assert!(visual.debug_bounds("shortcuts-edit-confirmation").is_some());
        visual.simulate_keystrokes("escape");
        draw(visual);
        assert!(visual.debug_bounds("shortcuts-edit-confirmation").is_none());
        assert!(visual.debug_bounds("shortcuts-edit-capture").is_some());

        search(visual, &rust_i18n::t!("shortcuts.operation.ToggleTheme"));
        click(visual, "shortcut-add-me_editor::ToggleTheme");
        assert!(visual.debug_bounds("shortcuts-edit-confirmation").is_some());
        click(visual, "shortcuts-edit-discard");
        assert!(visual.debug_bounds("shortcuts-edit-capture").is_some());
        visual.simulate_keystrokes("escape");
        draw(visual);
        assert!(visual.debug_bounds("shortcuts-edit-capture").is_none());
        assert!(visual.debug_bounds("shortcuts-panel").is_some());
        visual.simulate_keystrokes("escape");
        draw(visual);
        assert!(visual.debug_bounds("shortcuts-panel").is_none());
        assert!(
            !config.exists(),
            "discarded drafts must not create a user override"
        );
    });
}

/// A real two-step binding shows waiting state, preserves unmatched typing and crosses workspaces.
#[gpui::test]
fn shortcuts_editing_two_step_timeout_typing_and_shared_profile(cx: &mut TestAppContext) {
    let profile = tempfile::tempdir().unwrap();
    let config = profile.path().join("shortcuts.json");
    with_editor_profile(cx, false, vec![], &config, |visual, app, path| {
        open_global(visual, &rust_i18n::t!("shortcuts.operation.SaveDocument"));
        click(visual, "shortcut-binding-me_editor::SaveDocument-0");
        capture(visual, "ctrl-alt-j ctrl-alt-s");
        // Wrapped keycaps must grow the capture and keep the four actions below its painted area.
        let recording = visual.debug_bounds("shortcuts-edit-capture").unwrap();
        let restore = visual.debug_bounds("shortcuts-edit-restore").unwrap();
        assert!(recording.size.height >= px(56.));
        assert!(recording.bottom() <= restore.top());
        click(visual, "shortcuts-edit-save");
        let binding = visual
            .debug_bounds("shortcut-binding-me_editor::SaveDocument-0")
            .unwrap();
        assert!(binding.size.height >= px(56.));
        visual.simulate_keystrokes("escape");
        draw(visual);
        assert!(config.is_file());
        does_not_save(visual, path, "ctrl-s", "single step is gone");

        enter_document(visual, "prefix then typed ");
        visual.simulate_keystrokes("ctrl-alt-j");
        draw(visual);
        assert!(visual.debug_bounds("shortcuts-pending").is_some());
        visual.simulate_keystrokes("x");
        draw(visual);
        assert!(visual.debug_bounds("shortcuts-pending").is_none());
        assert_eq!(editor_text(visual, &app), "prefix then typed x");

        let before = std::fs::read_to_string(path).unwrap();
        visual.simulate_keystrokes("ctrl-alt-j");
        draw(visual);
        assert!(visual.debug_bounds("shortcuts-pending").is_some());
        visual.executor().advance_clock(Duration::from_secs(2));
        draw(visual);
        assert!(visual.debug_bounds("shortcuts-pending").is_none());
        visual.simulate_keystrokes("ctrl-alt-s");
        draw(visual);
        assert_eq!(std::fs::read_to_string(path).unwrap(), before);
        saves(
            visual,
            path,
            "ctrl-alt-j ctrl-alt-s",
            "completed sequence saves",
        );
    });

    // This opens a second workspace through normal startup with the same user profile. A separate
    // native process restart is covered by desktop acceptance rather than fabricated engine state.
    with_editor_profile(cx, false, vec![], &config, |visual, _, path| {
        does_not_save(visual, path, "ctrl-s", "second workspace old key");
        saves(
            visual,
            path,
            "ctrl-alt-j ctrl-alt-s",
            "second workspace shared binding",
        );
    });
}
