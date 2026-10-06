//! Exercises diagnostic publication, keyboard navigation, and local hover presentation.

use super::*;
use gpui_base::input::{DiagnosticSeverity, Position};
use gpui_kit::{TestAppContext, component::Root, gpui, size};
use std::{cell::RefCell, rc::Rc};

/// Use a plain document so asynchronous language-server startup cannot affect UI assertions.
fn test_editor<'a>(
    cx: &'a mut TestAppContext,
    source: &str,
) -> (
    Entity<EditorApp>,
    &'a mut gpui_kit::VisualTestContext,
    tempfile::TempDir,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(theme::builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sample.txt");
    std::fs::write(&path, source).unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| EditorApp::new(workspace, Some(path), window, cx));
        *capture.borrow_mut() = Some(view.clone());
        Root::new(view, window, cx)
    });
    let view = slot.borrow_mut().take().unwrap();
    visual.update(|window, _| window.activate_window());
    visual.simulate_resize(size(px(1000.), px(500.)));
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    (view, visual, directory)
}

/// Publish through the revision gate instead of injecting markers directly into Base.
fn publish(app: &mut EditorApp, errors: Vec<Diagnostic>, cx: &mut Context<EditorApp>) {
    let tab = app.text_tab(0).unwrap();
    app.apply_syntax_diagnostics(
        tab.editor.entity_id(),
        tab.session.revision(),
        tab.diagnostics.generation,
        app.plugin_loading_generation,
        errors,
        cx,
    );
}

/// Diagnostics from an old edit or plugin cannot resurrect cleared errors.
#[gpui::test]
fn stale_diagnostics_are_discarded(cx: &mut TestAppContext) {
    let (view, visual, _directory) = test_editor(cx, "sample @");
    visual.update(|_, cx| {
        view.update(cx, |app, cx| {
            let error = Diagnostic::new(Position::new(0, 7)..Position::new(0, 8), "unexpected @")
                .with_severity(DiagnosticSeverity::Error);
            let tab = app.text_tab(0).unwrap();
            let editor_id = tab.editor.entity_id();
            let revision = tab.session.revision();
            let generation = tab.diagnostics.generation;
            let plugin_generation = app.plugin_loading_generation;
            publish(app, vec![error.clone()], cx);
            assert_eq!(app.editor.read(cx).diagnostics().unwrap().len(), 1);
            app.tabs[0].text.as_mut().unwrap().session.note_edit();
            app.refresh_syntax_diagnostics(editor_id, cx);
            app.apply_syntax_diagnostics(
                editor_id,
                revision,
                generation,
                plugin_generation,
                vec![error.clone()],
                cx,
            );
            assert!(app.editor.read(cx).diagnostics().unwrap().is_empty());
            let tab = app.text_tab(0).unwrap();
            let revision = tab.session.revision();
            let generation = tab.diagnostics.generation;
            app.plugin_loading_generation = app.plugin_loading_generation.wrapping_add(1);
            app.apply_syntax_diagnostics(
                editor_id,
                revision,
                generation,
                plugin_generation,
                vec![error],
                cx,
            );
            assert!(app.editor.read(cx).diagnostics().unwrap().is_empty());
        })
    });
}

/// Error navigation wraps and presents the local card at the selected marker.
#[gpui::test]
fn syntax_error_navigation_and_card(cx: &mut TestAppContext) {
    let (view, visual, _directory) = test_editor(cx, "a @\nb @\n");
    visual.update(|window, cx| {
        view.update(cx, |app, cx| {
            publish(
                app,
                vec![
                    Diagnostic::new(Position::new(0, 2)..Position::new(0, 3), "first error"),
                    Diagnostic::new(Position::new(1, 2)..Position::new(1, 3), "second error"),
                ],
                cx,
            );
            app.navigate_syntax_error(false, window, cx);
            assert_eq!(app.editor.read(cx).cursor(), 2);
            app.navigate_syntax_error(false, window, cx);
            assert_eq!(app.editor.read(cx).cursor(), 6);
            app.navigate_syntax_error(false, window, cx);
            assert_eq!(app.editor.read(cx).cursor(), 2);
            app.navigate_syntax_error(true, window, cx);
            assert_eq!(app.editor.read(cx).cursor(), 6);
        })
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("editor-diagnostic-card").is_some());
    assert!(visual.debug_bounds("syntax-error-indicator").is_some());
    assert!(visual.debug_bounds("editor-definition-card").is_none());
    visual.update(|_, cx| view.update(cx, |app, cx| app.dismiss_pointer_hover(cx)));
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("editor-diagnostic-card").is_none());
}

/// Punctuation after multibyte text supports hover without an identifier or language server.
#[gpui::test]
fn syntax_error_hover_on_unicode_line(cx: &mut TestAppContext) {
    let (view, visual, _directory) = test_editor(cx, "中文😀 @\n");
    visual.update(|_, cx| {
        view.update(cx, |app, cx| {
            publish(
                app,
                vec![Diagnostic::new(
                    Position::new(0, 4)..Position::new(0, 5),
                    "unexpected @",
                )],
                cx,
            );
        })
    });
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let position = visual.update(|_, cx| {
        view.read(cx)
            .editor
            .read(cx)
            .range_to_bounds(&(11..12))
            .unwrap()
            .center()
    });
    visual.simulate_mouse_move(position, None, Default::default());
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("editor-diagnostic-card").is_some());
    visual.update(|_, cx| {
        assert_eq!(
            view.read(cx)
                .editor
                .read(cx)
                .diagnostic_popover()
                .unwrap()
                .message
                .as_str(),
            "unexpected @"
        );
    });
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    visual.simulate_mouse_move(position, None, Default::default());
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("editor-diagnostic-card").is_none());
}

/// A multiline recovery error can be explained when its first line has scrolled away.
#[gpui::test]
fn syntax_error_hover_after_scrolling(cx: &mut TestAppContext) {
    let (view, visual, _directory) = test_editor(cx, &"a @\n".repeat(100));
    visual.update(|_, cx| {
        view.update(cx, |app, cx| {
            publish(
                app,
                vec![Diagnostic::new(
                    Position::new(0, 2)..Position::new(90, 3),
                    "multiline error",
                )],
                cx,
            );
            app.editor.update(cx, |state, cx| {
                state.set_scroll_offset(point(px(0.), px(-900.)), cx)
            });
        })
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let (position, offset) = visual.update(|_, cx| {
        let state = view.read(cx).editor.read(cx);
        let row = state.visible_row_range().unwrap().start + 2;
        assert!(row > 0 && row < 90);
        let offset = state.text().line_start_offset(row) + 2;
        (
            state
                .range_to_bounds(&(offset..offset + 1))
                .unwrap()
                .center(),
            offset,
        )
    });
    visual.simulate_mouse_move(position, None, Default::default());
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("editor-diagnostic-card").is_some());
    visual.update(|_, cx| {
        let entry = view.read(cx).editor.read(cx).diagnostic_popover().unwrap();
        assert_eq!(entry.range, offset..offset + 1);
        assert_eq!(entry.diagnostic.range.start.line, 0);
    });
}

/// Server errors replace overlapping parser recovery messages without losing independent warnings.
#[gpui::test]
fn semantic_diagnostics_merge_and_clear(cx: &mut TestAppContext) {
    let (view, visual, _directory) = test_editor(cx, "missing() @");
    visual.update(|_, cx| {
        view.update(cx, |app, cx| {
            let editor_id = app.editor.entity_id();
            let syntax = Diagnostic::new(Position::new(0, 0)..Position::new(0, 7), "fallback")
                .with_severity(DiagnosticSeverity::Error);
            publish(app, vec![syntax], cx);
            app.apply_semantic_diagnostics(
                editor_id,
                vec![
                    Diagnostic::new(
                        Position::new(0, 0)..Position::new(0, 7),
                        "cannot find function missing",
                    )
                    .with_severity(DiagnosticSeverity::Error)
                    .with_code("E0425"),
                    Diagnostic::new(Position::new(0, 10)..Position::new(0, 11), "warning")
                        .with_severity(DiagnosticSeverity::Warning),
                ],
                cx,
            );
            let entries: Vec<_> = app.editor.read(cx).diagnostics().unwrap().iter().collect();
            assert_eq!(entries.len(), 2);
            assert_eq!(entries[0].message.as_str(), "cannot find function missing");
            assert_eq!(entries[1].severity, DiagnosticSeverity::Warning);
            app.apply_semantic_diagnostics(editor_id, Vec::new(), cx);
            assert_eq!(
                app.editor
                    .read(cx)
                    .diagnostics()
                    .unwrap()
                    .iter()
                    .next()
                    .unwrap()
                    .message
                    .as_str(),
                "fallback"
            );
        })
    });
}
