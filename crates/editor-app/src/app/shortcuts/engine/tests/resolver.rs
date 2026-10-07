//! Verify two-step execution against real window/focus identities and the shared keymap.

use super::*;
use crate::app::shortcuts::resolver::{Resolution, Resolver};
use std::time::{Duration, Instant};

/// An unmatched continuation remains the same native event and does not create a fresh prefix.
#[gpui::test]
fn shortcut_resolver_timeout_and_unmatched_second_preserve_native_processing(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    let window = cx
        .add_empty_window()
        .update(|window, _| window.window_handle());
    cx.update(|cx| {
        let engine = engine(
            cx,
            &directory.path().join("shortcuts.json"),
            vec![
                KeyBinding::new("ctrl-k ctrl-c", Copy, Some("Input")),
                KeyBinding::new("ctrl-v", Paste, Some("Input")),
                KeyBinding::new("ctrl-v ctrl-x", Cut, Some("Input")),
            ],
        );
        let focus = cx.focus_handle();
        let path = contexts(&["Root", "EditorShell", "Input"]);
        let first = Keystroke::parse("ctrl-k").unwrap();
        let second = Keystroke::parse("ctrl-v").unwrap();
        let start = Instant::now();
        let mut resolver = Resolver::default();
        assert!(
            !cx.key_bindings()
                .borrow()
                .bindings_for_input(&[first.clone()], &path)
                .1,
            "native GPUI must not start a second pending timer for a managed chord"
        );
        let hint = resolver.resolve(
            &engine,
            &first,
            window,
            Some(focus.clone()),
            &path,
            start,
            |_| true,
        );
        let Resolution::Pending(hint) = hint else {
            panic!("first step must show a next-step hint")
        };
        assert_eq!(hint.deadline, start + Duration::from_secs(2));
        assert_eq!(hint.next[0].sequence, bindings(&["ctrl-k ctrl-c"])[0]);
        assert!(!hint.next[0].title.is_empty());
        assert!(matches!(
            resolver.resolve(
                &engine,
                &second,
                window,
                Some(focus.clone()),
                &path,
                start,
                |_| true
            ),
            Resolution::Pass
        ));
        assert!(resolver.pending().is_none());
        assert_eq!(second, Keystroke::parse("ctrl-v").unwrap());
        assert!(
            native(cx, &second.unparse(), &["Input"])[0]
                .action()
                .partial_eq(&Paste)
        );
        resolver.resolve(
            &engine,
            &first,
            window,
            Some(focus.clone()),
            &path,
            start,
            |_| true,
        );
        assert!(!resolver.expire(start + Duration::from_millis(1999)));
        assert!(resolver.expire(start + Duration::from_secs(2)));
        assert!(matches!(
            resolver.resolve(
                &engine,
                &Keystroke::parse("ctrl-c").unwrap(),
                window,
                Some(focus),
                &path,
                start + Duration::from_secs(2),
                |_| true
            ),
            Resolution::Pass
        ));
    });
}

/// A render preparation pass can remove a stale hint even when no second key ever arrives.
#[gpui::test]
fn shortcut_resolver_prepare_clears_changed_or_expired_hint_without_dispatch(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    let window = cx
        .add_empty_window()
        .update(|window, _| window.window_handle());
    cx.update(|cx| {
        let engine = engine(
            cx,
            &directory.path().join("shortcuts.json"),
            vec![KeyBinding::new("ctrl-k ctrl-c", Copy, Some("Input"))],
        );
        let focus = cx.focus_handle();
        let path = contexts(&["Root", "Input"]);
        let first = Keystroke::parse("ctrl-k").unwrap();
        let now = Instant::now();
        let mut resolver = Resolver::default();
        resolver.resolve(
            &engine,
            &first,
            window,
            Some(focus.clone()),
            &path,
            now,
            |_| true,
        );
        assert!(!resolver.invalidate_if_changed(
            engine.revision(),
            window,
            Some(focus.clone()),
            &path,
            now
        ));
        assert!(resolver.pending().is_some());
        assert!(resolver.invalidate_if_changed(engine.revision(), window, None, &path, now));
        assert!(resolver.pending().is_none());
        resolver.resolve(
            &engine,
            &first,
            window,
            Some(focus.clone()),
            &path,
            now,
            |_| true,
        );
        assert!(resolver.invalidate_if_changed(
            engine.revision(),
            window,
            Some(focus),
            &path,
            now + Duration::from_secs(2)
        ));
        assert!(resolver.pending().is_none());
    });
}

/// Pending sequences are invalid after focus/window/context/revision changes or revoked authority.
#[gpui::test]
fn shortcut_resolver_rechecks_focus_revision_and_availability_before_exact_dispatch(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    let window = cx
        .add_empty_window()
        .update(|window, _| window.window_handle());
    let other_window = cx
        .add_empty_window()
        .update(|window, _| window.window_handle());
    cx.update(|cx| {
        let expected = Enter {
            secondary: true,
            shift: true,
        };
        let mut engine = engine(
            cx,
            &directory.path().join("shortcuts.json"),
            vec![KeyBinding::new(
                "ctrl-k ctrl-c",
                expected.clone(),
                Some("Input"),
            )],
        );
        let focus = cx.focus_handle();
        let other_focus = cx.focus_handle();
        let path = contexts(&["Root", "EditorShell", "Input"]);
        let other_path = contexts(&["Root", "Tree", "Input"]);
        let first = Keystroke::parse("ctrl-k").unwrap();
        let second = Keystroke::parse("ctrl-c").unwrap();
        let start = Instant::now();
        let mut resolver = Resolver::default();
        for reason in 0..5 {
            assert!(matches!(
                resolver.resolve(
                    &engine,
                    &first,
                    window,
                    Some(focus.clone()),
                    &path,
                    start,
                    |_| true
                ),
                Resolution::Pending(_)
            ));
            // Instance replacement uses this same explicit owner-to-engine revision boundary.
            if reason == 2 {
                engine.invalidate_pending();
            }
            let outcome = resolver.resolve(
                &engine,
                &second,
                if reason == 1 { other_window } else { window },
                Some(if reason == 0 {
                    other_focus.clone()
                } else {
                    focus.clone()
                }),
                if reason == 3 { &other_path } else { &path },
                start,
                |_| reason != 4,
            );
            assert!(
                matches!(outcome, Resolution::Pass),
                "cancellation reason {reason}"
            );
            assert!(resolver.pending().is_none());
        }
        resolver.resolve(
            &engine,
            &first,
            window,
            Some(focus.clone()),
            &path,
            start,
            |_| true,
        );
        let outcome = resolver.resolve(
            &engine,
            &second,
            window,
            Some(focus.clone()),
            &path,
            start + Duration::from_millis(1999),
            |_| true,
        );
        let Resolution::Dispatch {
            target: Target::Native { action, .. },
            ..
        } = outcome
        else {
            panic!("matching second step must dispatch once")
        };
        assert!(action.partial_eq(&expected));
        assert!(resolver.pending().is_none());
        assert!(matches!(
            resolver.resolve(&engine, &second, window, Some(focus), &path, start, |_| {
                true
            }),
            Resolution::Pass
        ));
    });
}
