//! Exercise filesystem reconciliation against real editor tabs and text entities.

use crate::*;
use editor_core::Workspace;
use gpui_kit::{TestAppContext, component::Root, gpui};
use std::{cell::RefCell, rc::Rc};

#[gpui::test]
fn external_edits_preserve_dirty_text_and_track_deletion_and_rename(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        theme::apply_theme(theme::builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("note.txt");
    std::fs::write(&original, "first").unwrap();
    let original = original.canonicalize().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let initial = original.clone();
    let (_, window_cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, Some(initial), window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();

    // Clean external changes replace the editor text without changing its dirty state.
    std::fs::write(&original, "second").unwrap();
    window_cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.apply_reconciliation(
                Reconciliation {
                    snapshot: Some(app.workspace.snapshot()),
                    documents: vec![(original.clone(), Ok("second".into()), Instant::now())],
                    renames: Vec::new(),
                    native: true,
                },
                window,
                cx,
            );
            assert_eq!(app.tabs[0].editor.read(cx).value().to_string(), "second");
            assert!(!app.tabs[0].session.is_dirty());
        });
    });

    // An unsaved buffer wins over the disk; the tab advertises the conflict.
    window_cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.tabs[0]
                .editor
                .update(cx, |editor, cx| editor.set_value("local", window, cx));
            // set_value is silent, so record the user revision represented by this text.
            app.tabs[0].session.note_edit();
            assert!(app.tabs[0].session.is_dirty());
        });
    });
    std::fs::write(&original, "external").unwrap();
    window_cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.apply_reconciliation(
                Reconciliation {
                    snapshot: Some(app.workspace.snapshot()),
                    documents: vec![(original.clone(), Ok("external".into()), Instant::now())],
                    renames: Vec::new(),
                    native: true,
                },
                window,
                cx,
            );
            assert_eq!(app.tabs[0].disk_state, DiskState::Conflict);
            assert_eq!(app.tabs[0].editor.read(cx).value().to_string(), "local");
        });
    });

    // A conflicted save requires a second explicit action before overwriting disk.
    window_cx.update(|_, cx| {
        app.update(cx, |app, cx| {
            app.save_current(cx);
            assert_eq!(std::fs::read_to_string(&original).unwrap(), "external");
            app.save_current(cx);
            assert_eq!(std::fs::read_to_string(&original).unwrap(), "local");
            assert_eq!(app.tabs[0].disk_state, DiskState::Synced);
        });
    });

    // Deleting a file leaves its tab and editor content available.
    std::fs::remove_file(&original).unwrap();
    window_cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.apply_reconciliation(
                Reconciliation {
                    snapshot: Some(app.workspace.snapshot()),
                    documents: vec![(
                        original.clone(),
                        Err(std::io::ErrorKind::NotFound.into()),
                        Instant::now(),
                    )],
                    renames: Vec::new(),
                    native: true,
                },
                window,
                cx,
            );
            assert_eq!(app.tabs[0].disk_state, DiskState::Deleted);
            assert_eq!(app.tabs[0].editor.read(cx).value().to_string(), "local");
        });
    });

    // A deleted file can be explicitly recreated from the retained editor text.
    window_cx.update(|_, cx| {
        app.update(cx, |app, cx| {
            app.save_current(cx);
            assert!(!original.exists());
            app.save_current(cx);
            assert_eq!(std::fs::read_to_string(&original).unwrap(), "local");
        });
    });

    // Only a verified rename pair transfers the existing tab to the new path.
    let renamed = directory.path().join("renamed.txt");
    std::fs::rename(&original, &renamed).unwrap();
    window_cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.apply_reconciliation(
                Reconciliation {
                    snapshot: Some(app.workspace.snapshot()),
                    documents: vec![(
                        original.clone(),
                        Err(std::io::ErrorKind::NotFound.into()),
                        Instant::now(),
                    )],
                    renames: vec![(original.clone(), renamed.clone())],
                    native: true,
                },
                window,
                cx,
            );
            assert_eq!(app.tabs[0].session.path(), renamed.as_path());
            assert_eq!(app.active_path.as_deref(), Some(renamed.as_path()));
            assert_eq!(app.tabs[0].editor.read(cx).value().to_string(), "local");
        });
    });
}
