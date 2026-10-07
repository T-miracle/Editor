//! Exercise persisted binding changes through GPUI's real keymap and resolver boundaries.

use super::*;
use crate::app::shortcuts::catalog;
use gpui_base::{
    actions::SelectDown,
    input::{Backspace, Copy, Cut, DeleteToPreviousWordStart, Enter, Escape, Paste},
};
use gpui_kit::{Action, Keystroke, TestAppContext, gpui};
use std::{fs, path::Path};

mod resolver;

/// Use native actions and the production catalog, retaining a small, explicit default keymap.
fn engine(cx: &mut App, path: &Path, defaults: Vec<KeyBinding>) -> BindingEngine {
    cx.clear_key_bindings();
    cx.bind_keys(defaults.clone());
    let mut engine = BindingEngine::load(path.to_owned(), defaults).unwrap();
    engine.register(catalog::all_operations(engine.defaults(), &[], cx));
    engine.apply(cx).unwrap();
    engine
}

/// Select an exact payload variant; two Enter variants must never share an operation identifier.
fn operation(engine: &BindingEngine, action: &dyn Action) -> String {
    engine.active_operations().find(|operation| {
        matches!(&operation.target, Target::Native { action: actual, .. } if actual.partial_eq(action))
    }).expect("registered native operation").id.clone()
}

/// Canonical input strings match the same parser used by real keyboard events.
fn bindings(sequences: &[&str]) -> Vec<Sequence> {
    sequences
        .iter()
        .map(|sequence| {
            sequence
                .split_whitespace()
                .map(|step| Keystroke::parse(step).unwrap().unparse())
                .collect()
        })
        .collect()
}

fn contexts(names: &[&str]) -> Vec<KeyContext> {
    names
        .iter()
        .map(|name| KeyContext::parse(name).unwrap())
        .collect()
}

/// Query dispatch candidates rather than inspecting the engine's private override map.
fn native(cx: &App, key: &str, path: &[&str]) -> Vec<KeyBinding> {
    cx.key_bindings()
        .borrow()
        .bindings_for_input(&[Keystroke::parse(key).unwrap()], &contexts(path))
        .0
        .into_iter()
        .collect()
}

/// Removing the current default cannot uncover a superseded binding at the same predicate.
#[gpui::test]
fn shortcut_engine_rebind_remove_restore_never_revives_shadowed_action(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        let mut engine = engine(
            cx,
            &directory.path().join("shortcuts.json"),
            vec![
                KeyBinding::new("ctrl-backspace", Backspace, Some("Input")),
                KeyBinding::new("ctrl-backspace", DeleteToPreviousWordStart, Some("Input")),
            ],
        );
        let id = operation(&engine, &DeleteToPreviousWordStart);
        engine
            .save(&id, &bindings(&["ctrl-shift-backspace"]), false, cx)
            .unwrap();
        assert!(native(cx, "ctrl-backspace", &["Root", "Input"]).is_empty());
        let actual = native(cx, "ctrl-shift-backspace", &["Root", "Input"]);
        assert_eq!(actual.len(), 1);
        assert!(actual[0].action().partial_eq(&DeleteToPreviousWordStart));
        engine.remove(&id, 0, cx).unwrap();
        assert!(native(cx, "ctrl-shift-backspace", &["Root", "Input"]).is_empty());
        assert!(native(cx, "ctrl-backspace", &["Root", "Input"]).is_empty());
        engine.restore(&id, false, cx).unwrap();
        let restored = native(cx, "ctrl-backspace", &["Root", "Input"]);
        assert_eq!(restored.len(), 1);
        assert!(restored[0].action().partial_eq(&DeleteToPreviousWordStart));
    });
}

/// Independent bindings and opaque action fields survive saving, deleting and restoring.
#[gpui::test]
fn shortcut_engine_multiple_bindings_preserve_exact_enter_payload(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        let primary = Enter {
            secondary: false,
            shift: false,
        };
        let secondary = Enter {
            secondary: true,
            shift: true,
        };
        let mut engine = engine(
            cx,
            &directory.path().join("shortcuts.json"),
            vec![
                KeyBinding::new("enter", primary.clone(), Some("Input")),
                KeyBinding::new("shift-enter", secondary.clone(), Some("Input")),
            ],
        );
        let id = operation(&engine, &primary);
        assert_ne!(id, operation(&engine, &secondary));
        engine
            .save(&id, &bindings(&["f5", "f6"]), false, cx)
            .unwrap();
        engine.remove(&id, 0, cx).unwrap();
        assert!(native(cx, "enter", &["Input"]).is_empty());
        assert!(native(cx, "f5", &["Input"]).is_empty());
        assert!(
            native(cx, "f6", &["Input"])[0]
                .action()
                .partial_eq(&primary)
        );
        assert!(
            native(cx, "shift-enter", &["Input"])[0]
                .action()
                .partial_eq(&secondary)
        );
        assert!(native(cx, "f6", &["Tree"]).is_empty());
        engine.restore(&id, false, cx).unwrap();
        assert!(
            native(cx, "enter", &["Input"])[0]
                .action()
                .partial_eq(&primary)
        );
        assert!(native(cx, "f6", &["Input"]).is_empty());
    });
}

/// Both exact collisions and consumed prefixes require replacement, which preserves siblings.
#[gpui::test]
fn shortcut_engine_conflict_replacement_preserves_other_binding_and_restore_preview(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        let mut engine = engine(
            cx,
            &directory.path().join("shortcuts.json"),
            vec![
                KeyBinding::new("ctrl-c", Copy, Some("Input")),
                KeyBinding::new("ctrl-insert", Copy, Some("Input")),
                KeyBinding::new("ctrl-v", Paste, Some("Input")),
                KeyBinding::new("ctrl-x", Cut, Some("Input")),
            ],
        );
        let copy = operation(&engine, &Copy);
        let paste = operation(&engine, &Paste);
        let cut = operation(&engine, &Cut);
        assert_eq!(
            engine
                .validate(&paste, &bindings(&["ctrl-c"]))
                .unwrap()
                .len(),
            1
        );
        let chord = bindings(&["ctrl-c ctrl-u"]);
        assert!(matches!(
            engine.save(&paste, &chord, false, cx),
            Err(BindingError::Conflicts(_))
        ));
        assert!(
            native(cx, "ctrl-v", &["Input"])[0]
                .action()
                .partial_eq(&Paste)
        );
        engine.save(&paste, &chord, true, cx).unwrap();
        assert_eq!(engine.effective(&copy), bindings(&["ctrl-insert"]));
        assert!(
            native(cx, "ctrl-insert", &["Input"])[0]
                .action()
                .partial_eq(&Copy)
        );
        assert!(native(cx, "ctrl-v", &["Input"]).is_empty());
        let restore = engine.validate_restore(&copy).unwrap();
        assert_eq!(restore.len(), 1);
        assert_eq!(restore[0].operation, paste);
        assert!(matches!(
            engine.restore(&copy, false, cx),
            Err(BindingError::Conflicts(_))
        ));
        assert_eq!(engine.effective(&paste), chord);
        assert_eq!(
            engine.validate(&cut, &bindings(&["ctrl-c"])).unwrap()[0].operation,
            paste
        );
    });
}

/// Run-configuration renaming embeds Input in Tree; surface exclusion permits real reuse.
#[gpui::test]
fn shortcut_engine_conflicts_follow_reachable_input_tree_and_plugin_scopes(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        let mut engine = engine(
            cx,
            &directory.path().join("shortcuts.json"),
            vec![
                KeyBinding::new("ctrl-c", Copy, Some("Input")),
                KeyBinding::new("down", SelectDown, Some("Tree")),
                KeyBinding::new(
                    "ctrl-s",
                    crate::SaveDocument,
                    Some("EditorShell && !PluginSurface"),
                ),
                // An ancestor binding still reaches Paste's focused native Input handler.
                KeyBinding::new("ctrl-v", Paste, Some("PluginSurface")),
            ],
        );
        let down = operation(&engine, &SelectDown);
        assert_eq!(
            engine
                .validate(&down, &bindings(&["ctrl-c"]))
                .unwrap()
                .len(),
            1
        );
        let paste = operation(&engine, &Paste);
        assert!(
            engine
                .validate(&paste, &bindings(&["ctrl-s"]))
                .unwrap()
                .is_empty()
        );
        engine
            .save(&paste, &bindings(&["ctrl-s"]), false, cx)
            .unwrap();
        let ordinary = native(cx, "ctrl-s", &["Root", "EditorShell", "Input"]);
        assert_eq!(ordinary.len(), 1);
        assert!(ordinary[0].action().partial_eq(&crate::SaveDocument));
        let plugin = native(
            cx,
            "ctrl-s",
            &["Root", "EditorShell", "PluginSurface", "Input"],
        );
        assert_eq!(plugin.len(), 1);
        assert!(plugin[0].action().partial_eq(&Paste));
    });
}

/// A deterministic filesystem failure must leave both dispatch and displayed bindings intact.
#[gpui::test]
fn shortcut_engine_failed_save_preserves_running_configuration(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let parent = directory.path().join("profile");
    let path = parent.join("shortcuts.json");
    cx.update(|cx| {
        let mut engine = engine(
            cx,
            &path,
            vec![KeyBinding::new("ctrl-c", Copy, Some("Input"))],
        );
        let id = operation(&engine, &Copy);
        fs::write(&parent, "a file cannot contain the shortcut profile").unwrap();
        let revision = engine.revision();
        assert!(matches!(
            engine.save(&id, &bindings(&["ctrl-alt-c"]), false, cx),
            Err(BindingError::Storage(_))
        ));
        assert_eq!(engine.revision(), revision);
        assert_eq!(engine.effective(&id), bindings(&["ctrl-c"]));
        assert!(
            native(cx, "ctrl-c", &["Input"])[0]
                .action()
                .partial_eq(&Copy)
        );
        assert!(native(cx, "ctrl-alt-c", &["Input"]).is_empty());
        assert_eq!(
            fs::read_to_string(&parent).unwrap(),
            "a file cannot contain the shortcut profile"
        );
    });
}

/// A new engine reuses persisted overrides; malformed reload reports an error without erasing data.
#[gpui::test]
fn shortcut_engine_profile_reload_preserves_overrides_and_rejects_corruption(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("shortcuts.json");
    cx.update(|cx| {
        let defaults = vec![KeyBinding::new("ctrl-c", Copy, Some("Input"))];
        let mut first = engine(cx, &path, defaults.clone());
        let id = operation(&first, &Copy);
        first
            .save(&id, &bindings(&["ctrl-alt-c", "ctrl-insert"]), false, cx)
            .unwrap();
        drop(first);
        let mut restarted = engine(cx, &path, defaults);
        assert_eq!(
            restarted.effective(&id),
            bindings(&["ctrl-alt-c", "ctrl-insert"])
        );
        assert!(native(cx, "ctrl-c", &["Input"]).is_empty());
        assert!(
            native(cx, "ctrl-alt-c", &["Input"])[0]
                .action()
                .partial_eq(&Copy)
        );
        fs::write(&path, "{ broken profile").unwrap();
        assert!(matches!(
            restarted.reload(cx),
            Err(BindingError::Storage(_))
        ));
        assert!(
            native(cx, "ctrl-alt-c", &["Input"])[0]
                .action()
                .partial_eq(&Copy)
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ broken profile");
    });
}

/// Existing Tree letter defaults survive partial deletion and restart; new text drafts stay invalid.
#[gpui::test]
fn shortcut_engine_inherited_text_defaults_reload_but_new_text_is_rejected(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("shortcuts.json");
    cx.update(|cx| {
        let defaults = vec![
            KeyBinding::new("j", SelectDown, Some("Tree")),
            KeyBinding::new("down", SelectDown, Some("Tree")),
            KeyBinding::new("escape", Escape, Some("Input")),
        ];
        let mut first = engine(cx, &path, defaults.clone());
        let id = operation(&first, &SelectDown);
        // Digits are text on both steps too; a valid modified prefix cannot exempt its continuation.
        for draft in [
            bindings(&["k"]),
            bindings(&["j", "shift-k"]),
            bindings(&["8"]),
            bindings(&["shift-8"]),
            bindings(&["ctrl-k a"]),
            bindings(&["ctrl-k j"]),
            bindings(&["ctrl-k 8"]),
            bindings(&["f5 f6 f7"]),
        ] {
            assert!(matches!(
                first.save(&id, &draft, false, cx),
                Err(BindingError::Invalid(_))
            ));
        }
        for draft in [bindings(&["j", "escape"]), bindings(&["ctrl-k escape"])] {
            assert!(matches!(
                first.validate(&id, &draft),
                Err(BindingError::Invalid(InvalidBinding::ReservedEscape))
            ));
            assert!(matches!(
                first.save(&id, &draft, false, cx),
                Err(BindingError::Invalid(InvalidBinding::ReservedEscape))
            ));
        }
        assert!(!path.exists());
        let added = bindings(&["j", "down", "ctrl-j"]);
        assert!(first.validate(&id, &added).unwrap().is_empty());
        first.save(&id, &added, false, cx).unwrap();
        first.remove(&id, 1, cx).unwrap();
        let escape = operation(&first, &Escape);
        first
            .save(&escape, &bindings(&["escape", "f9"]), false, cx)
            .unwrap();
        let restarted = engine(cx, &path, defaults);
        assert_eq!(restarted.effective(&id), bindings(&["j", "ctrl-j"]));
        assert!(
            native(cx, "j", &["Tree"])[0]
                .action()
                .partial_eq(&SelectDown)
        );
        assert!(native(cx, "down", &["Tree"]).is_empty());
        assert!(
            native(cx, "ctrl-j", &["Tree"])[0]
                .action()
                .partial_eq(&SelectDown)
        );
        assert!(
            native(cx, "escape", &["Input"])[0]
                .action()
                .partial_eq(&Escape)
        );
        assert!(native(cx, "f9", &["Input"])[0].action().partial_eq(&Escape));
    });
}
