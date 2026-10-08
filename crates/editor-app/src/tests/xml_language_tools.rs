//! A short acceptance scenario composes the delivered language, structure and independent SVG viewer.

use crate::extensions::composable_tests::{publish, pump};
use crate::ui::plugin::images::VectorRenderer;
use crate::*;
use gpui_kit::{SharedString, TestAppContext, VisualTestContext, gpui};
use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

/// Exercise the shared document at the installed-package seam without replaying the detailed matrices.
#[gpui::test]
#[ignore = "build current XML and SVG ZIPs with the current public SDK first"]
fn xml_language_tools_svg_combination_and_workspace_restore(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        bind_editor_shell_keys(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("drawing.svg");
    let original = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"160\" height=\"100\"><g id=\"中文🙂\"><rect width=\"160\" height=\"100\" fill=\"red\"/></g></svg>";
    std::fs::write(&path, original).unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let mut manager = plugin_runtime::Manager::open(
        workspace.root().join(".runtime-plugin-test"),
        plugin_runtime::plugin_protocol::Environment {
            os: std::env::consts::OS.into(),
            workspace: workspace.root().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    for id in ["svg", "xml"] {
        let package = plugin_runtime::Package::read(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../dist/plugins/{id}.zip")),
        )
        .unwrap();
        if id == "xml" {
            // The default private preparation is already covered; reuse its approved exact binary for this short composition.
            super::editing_fixture::install_xml(&mut manager, &package);
        } else {
            manager
                .install(&package, package.manifest.permissions.clone())
                .unwrap();
        }
    }
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    visual.simulate_resize(size(px(1400.), px(900.)));
    crate::extensions::lsp_tests::publish(&app, &mut manager, visual);
    visual.update(|window, cx| {
        window.activate_window();
        app.update(cx, |app, cx| app.open_file(path.clone(), window, cx));
    });
    let mut renderer = VectorRenderer::default();
    wait(
        "initial outline and preview",
        &mut manager,
        &mut renderer,
        &app,
        visual,
        |_, visual, _| {
            visual.debug_bounds("outline-node-/0/0").is_some()
                && visual.debug_bounds("plugin-ui-preview-canvas").is_some()
        },
    );
    let editor = visual.update(|_, cx| app.read(cx).editor.clone());
    let server = visual.update(|_, cx| app.read(cx).language_edits.formatters["xml"].clone());
    server.prepare_until_ready().unwrap();
    visual.update(|window, cx| {
        let guest_focus = app.read(cx).extensions.read(cx).focus_handle(cx);
        guest_focus.focus(window, cx);
        window.draw(cx).clear(cx);
        for action in [
            &FormatDocument as &dyn gpui::Action,
            &RenameSymbol,
            &SaveDocument,
        ] {
            assert!(
                window
                    .bindings_for_action_in(action, &guest_focus)
                    .is_empty(),
                "guest controls cannot acquire native document shortcuts"
            );
        }
    });
    visual.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.focus(window, cx));
        window.refresh();
        window.draw(cx).clear(cx);
        let source_focus = editor.focus_handle(cx);
        for action in [
            &FormatDocument as &dyn gpui::Action,
            &RenameSymbol,
            &SaveDocument,
        ] {
            assert!(
                !window
                    .bindings_for_action_in(action, &source_focus)
                    .is_empty(),
                "an embedded host source must retain native document shortcuts"
            );
        }
    });
    visual.simulate_keystrokes("shift-alt-f");
    wait(
        "manual formatting",
        &mut manager,
        &mut renderer,
        &app,
        visual,
        |app, visual, _| text(app, visual) != original,
    );
    let formatted = text(&app, visual);
    assert!(
        formatted.contains('\n'),
        "the native formatting command must reach the selected XML formatter"
    );

    // A single immediate input replaces one name and its semantic peer in the same native history entry.
    let name = formatted.find("<g ").unwrap() + 1;
    visual.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_selected_range(name..name + 1, cx);
            editor.focus(window, cx);
            // The public selection setter leaves painting to its caller; expose this caret to native input.
            cx.notify();
        })
    });
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.simulate_input("a");
    let renamed = formatted
        .replacen("<g ", "<a ", 1)
        .replacen("</g>", "</a>", 1);
    wait(
        "paired name and preview",
        &mut manager,
        &mut renderer,
        &app,
        visual,
        |app, visual, manager| {
            text(app, visual) == renamed
                && visual.debug_bounds("outline-node-/0/0").is_some()
                && outline_name(app, visual, "/0/0") == Some("a #中文🙂".into())
                && serde_json::to_string(manager.live["svg"].views["preview"].as_ref())
                    .unwrap()
                    .contains("<a id")
        },
    );
    let row = visual.debug_bounds("outline-node-/0/0").unwrap();
    visual.simulate_click(row.center(), Default::default());
    visual.run_until_parked();
    assert_eq!(
        visual.update(|_, cx| editor.read(cx).cursor()),
        renamed.find("<a ").unwrap() + 1
    );
    assert_eq!(visual.update(|_, cx| app.read(cx).editor.clone()), editor);
    assert!(
        serde_json::to_string(manager.live["svg"].views["preview"].as_ref())
            .unwrap()
            .contains("中文🙂")
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        original,
        "preview and outline must consume unsaved native content"
    );

    visual.simulate_keystrokes("ctrl-z");
    wait(
        "paired undo",
        &mut manager,
        &mut renderer,
        &app,
        visual,
        |app, visual, _| text(app, visual) == formatted,
    );
    visual.simulate_keystrokes("ctrl-y");
    wait(
        "paired redo",
        &mut manager,
        &mut renderer,
        &app,
        visual,
        |app, visual, _| text(app, visual) == renamed,
    );
    let scenes = manager.live["svg"]
        .views
        .iter()
        .map(|(id, view)| (format!("svg/{id}"), view.clone()))
        .collect();
    assert!(
        renderer
            .prepare(&scenes)
            .values()
            .any(|images| images.iter().any(Option::is_some))
    );

    // A new host view restores the recorded workspace split and shows both built-in peer panels.
    let saved = visual.update(|_, cx| {
        app.update(cx, |app, cx| {
            app.capture_dock_layout(cx);
            app.persist_session();
            app.session_state.dock_layout.clone().unwrap()
        })
    });
    let workspace = Workspace::open(directory.path()).unwrap();
    let restored_slot = Rc::new(RefCell::new(None));
    let capture = restored_slot.clone();
    let (_, restored_visual) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let restored = restored_slot.borrow_mut().take().unwrap();
    restored_visual.simulate_resize(size(px(1400.), px(900.)));
    crate::extensions::lsp_tests::publish(&restored, &mut manager, restored_visual);
    // Restored tabs start a fresh asynchronous structure request; one draw is not a completion signal.
    wait(
        "restored outline",
        &mut manager,
        &mut renderer,
        &restored,
        restored_visual,
        |app, visual, _| {
            visual.debug_bounds("outline-node-/0/0").is_some()
                && outline_name(app, visual, "/0/0") == Some("g #中文🙂".into())
        },
    );
    let actual = restored_visual.update(|_, cx| restored.read(cx).dock_area.read(cx).dump(cx));
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(saved).unwrap()
    );
    assert!(
        restored_visual.debug_bounds("outline-empty").is_some()
            || restored_visual.debug_bounds("outline-tree").is_some()
    );
    assert!(
        restored_visual
            .debug_bounds("explorer-reveal-active-file")
            .is_some()
    );
}

/// Drive the existing worker publication and actual raster renderer until an observable native result.
fn wait(
    phase: &str,
    manager: &mut plugin_runtime::Manager,
    renderer: &mut VectorRenderer,
    app: &Entity<EditorApp>,
    visual: &mut VisualTestContext,
    mut ready: impl FnMut(&Entity<EditorApp>, &mut VisualTestContext, &plugin_runtime::Manager) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        // GPUI timers use the test clock; stdio services use wall time. Advance both while waiting.
        visual.executor().advance_clock(Duration::from_millis(10));
        pump(manager, app, visual);
        visual.update(|window, cx| {
            // Headless drawing alone does not run open_file's public next-frame grammar initialization.
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        });
        manager.poll();
        publish(manager, renderer, app, visual);
        if ready(app, visual, manager) {
            return;
        }
        if Instant::now() >= deadline {
            // Preserve the public native observables at the failing phase, rather than treating
            // an unavailable virtual row, a stale scene and a missing text edit as one failure.
            let roles = language::providers::rows()
                .into_iter()
                .map(|row| (row.key, row.selected, row.candidates))
                .collect::<Vec<_>>();
            let preview = serde_json::to_string(manager.live["svg"].views["preview"].as_ref())
                .unwrap()
                .chars()
                .take(1200)
                .collect::<String>();
            let status = visual.update(|_, cx| app.read(cx).status.clone());
            panic!(
                "combined native phase {phase} timed out; status={status:?}; text={:?}; root={:?}; child={:?}; loading={:?}; failed={:?}; preview={:?}; roles={roles:?}; scene={preview}",
                text(app, visual),
                visual.debug_bounds("outline-node-/0"),
                visual.debug_bounds("outline-node-/0/0"),
                visual.debug_bounds("outline-loading"),
                visual.debug_bounds("outline-failed"),
                visual.debug_bounds("plugin-ui-preview-canvas"),
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Read the sole native text owner, including unsaved formatting and history changes.
fn text(app: &Entity<EditorApp>, visual: &mut VisualTestContext) -> String {
    visual.update(|_, cx| app.read(cx).editor.read(cx).text().to_string())
}

/// A painted path can belong to a previous revision; require the plugin's current public tree label too.
fn outline_name(
    app: &Entity<EditorApp>,
    visual: &mut VisualTestContext,
    id: &str,
) -> Option<String> {
    visual.update(|_, cx| {
        let tree = app.read(cx).outline.tree.read(cx);
        let index = tree.index_of(&SharedString::from(id.to_owned()))?;
        Some(tree.entry(index)?.item().label.to_string())
    })
}
