//! Real SDK packages enter shortcut lookup through the existing manager/publication boundary.

use super::*;
use gpui_kit::{Focusable, TestAppContext, VisualTestContext, gpui};
use std::{
    io::{Cursor, Write},
    time::Duration,
};

/// Reuse the current independently built component under an identity unknown to the host.
/// Commands belong in the protocol manifest: resource-only TOML packages cannot declare them.
fn shortcut_package() -> Package {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/plugin-api-test/capability-example.zip");
    let mut files = Package::read(&source)
        .expect("build scripts/build-capability-example.ps1 before running this ignored test")
        .files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["id"] = serde_json::json!("shortcut-query-fixture");
    manifest["name"] = serde_json::json!("Shortcut query fixture");
    manifest["settings_hook"] = serde_json::json!(false);
    manifest["panels"][0]["default_visible"] = serde_json::json!(false);
    manifest["commands"] = serde_json::json!([
        {"id": "show-panel", "title": "Shortcut fixture show panel", "shortcut": "ctrl-alt-j", "menu": true},
        {"id": "read-selection", "title": "Shortcut fixture read selection", "menu": true}
    ]);
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        archive
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&archive.finish().unwrap().into_inner()).unwrap()
}

/// A rendered frame observes ordinary polling and lets the modal keep its real input tree.
fn draw(visual: &mut VisualTestContext) {
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
}

/// Published plugin commands are searchable, while both search modes suppress their execution.
#[gpui::test]
#[ignore = "build the real SDK fixture with scripts/build-capability-example.ps1 first"]
fn shortcuts_real_plugin_query_and_capture_do_not_execute(cx: &mut TestAppContext) {
    crate::tests::with_shortcut_editor(cx, false, vec![], |visual, app, path| {
        let package = shortcut_package();
        // The runtime has a separate temporary private root, outside the temporary workspace.
        let runtime = tempfile::tempdir().unwrap();
        let mut manager = plugin_runtime::Manager::open(
            runtime.path().to_path_buf(),
            protocol::Environment {
                workspace: path.parent().unwrap().display().to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
        let mut renderer = images::VectorRenderer::default();
        composable_tests::publish(&mut manager, &mut renderer, &app, visual);
        draw(visual);
        assert!(visual.debug_bounds("plugin-ui-welcome-text").is_none());
        visual.update(|window, cx| {
            app.read(cx)
                .editor
                .read(cx)
                .focus_handle(cx)
                .focus(window, cx);
            window.draw(cx).clear(cx);
        });

        visual.simulate_keystrokes("ctrl-k alt-right");
        draw(visual);
        visual.simulate_input("Shortcut fixture");
        draw(visual);
        assert!(
            visual
                .debug_bounds("shortcut-operation-shortcut-query-fixture/show-panel")
                .is_some()
        );
        assert!(
            visual
                .debug_bounds("shortcut-operation-shortcut-query-fixture/read-selection")
                .is_some()
        );

        // The raw plugin shortcut must also stay blocked while the ordinary search input has focus.
        visual.simulate_keystrokes("ctrl-alt-j");
        composable_tests::pump(&mut manager, &app, visual);
        composable_tests::publish(&mut manager, &mut renderer, &app, visual);
        draw(visual);
        assert!(visual.debug_bounds("shortcuts-panel").is_some());
        assert!(visual.debug_bounds("plugin-ui-welcome-text").is_none());
        let capture = visual.debug_bounds("shortcuts-capture").unwrap();
        visual.simulate_click(capture.center(), Default::default());
        visual.simulate_keystrokes("ctrl-alt-j");
        composable_tests::pump(&mut manager, &app, visual);
        composable_tests::publish(&mut manager, &mut renderer, &app, visual);
        draw(visual);
        assert!(
            visual
                .debug_bounds("shortcut-operation-shortcut-query-fixture/show-panel")
                .is_some()
        );
        assert!(
            visual
                .debug_bounds("shortcut-operation-shortcut-query-fixture/read-selection")
                .is_none()
        );
        assert!(visual.debug_bounds("plugin-ui-welcome-text").is_none());

        // Positive control: the same declared shortcut still invokes the actual component outside
        // the modal. Its normal shell route reveals the package's previously hidden native panel.
        visual.simulate_keystrokes("escape");
        draw(visual);
        visual.simulate_keystrokes("escape");
        draw(visual);
        visual.simulate_keystrokes("ctrl-alt-j");
        draw(visual);
        composable_tests::pump(&mut manager, &app, visual);
        composable_tests::publish(&mut manager, &mut renderer, &app, visual);
        draw(visual);
        assert!(visual.debug_bounds("shortcuts-panel").is_none());
        assert!(visual.debug_bounds("plugin-ui-welcome-text").is_some());
    });
}

/// Activate the same local Base buttons as pointer users, including their focus behavior.
fn click(visual: &mut VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    visual.simulate_click(bounds.center(), Default::default());
    draw(visual);
}

/// Start editing the command's current effective binding through its visible operation row.
fn edit_show_panel(visual: &mut VisualTestContext, keys: &str) {
    visual.simulate_keystrokes("ctrl-k alt-right");
    draw(visual);
    visual.simulate_input("Shortcut fixture show panel");
    draw(visual);
    click(
        visual,
        "shortcut-binding-shortcut-query-fixture/show-panel-0",
    );
    visual.simulate_keystrokes(keys);
    visual.executor().advance_clock(Duration::from_secs(2));
    draw(visual);
}

/// Reuse the real host close-panel route so the next command must visibly reopen its surface.
fn hide_and_focus_editor(visual: &mut VisualTestContext, app: &Entity<EditorApp>, panel: &str) {
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.hide_plugin_panel("shortcut-query-fixture", panel, cx);
            app.editor.read(cx).focus_handle(cx).focus(window, cx);
        });
        window.draw(cx).clear(cx);
    });
    draw(visual);
    assert!(visual.debug_bounds("plugin-ui-welcome-text").is_none());
}

/// Edited plugin keys run the real component; explicit conflicts preserve unrelated native keys.
#[gpui::test]
#[ignore = "build the real SDK fixture with scripts/build-capability-example.ps1 first"]
fn shortcuts_real_plugin_edit_dispatch_and_native_conflict(cx: &mut TestAppContext) {
    crate::tests::with_shortcut_editor(
        cx,
        false,
        vec![gpui_kit::KeyBinding::new(
            "ctrl-alt-u",
            crate::SaveDocument,
            Some("EditorShell && !PluginSurface"),
        )],
        |visual, app, path| {
            let package = shortcut_package();
            let panel_id = package.manifest.panels[0].id.clone();
            let runtime = tempfile::tempdir().unwrap();
            let mut manager = plugin_runtime::Manager::open(
                runtime.path().to_path_buf(),
                protocol::Environment {
                    workspace: path.parent().unwrap().display().to_string(),
                    ..Default::default()
                },
            )
            .unwrap();
            manager
                .install(&package, package.manifest.permissions.clone())
                .unwrap();
            let mut renderer = images::VectorRenderer::default();
            composable_tests::publish(&mut manager, &mut renderer, &app, visual);
            hide_and_focus_editor(visual, &app, &panel_id);

            edit_show_panel(visual, "ctrl-alt-v");
            // Running the actual worker pump while the draft exists must not execute its key.
            composable_tests::pump(&mut manager, &app, visual);
            composable_tests::publish(&mut manager, &mut renderer, &app, visual);
            draw(visual);
            assert!(visual.debug_bounds("shortcuts-edit-capture").is_some());
            assert!(visual.debug_bounds("plugin-ui-welcome-text").is_none());
            click(visual, "shortcuts-edit-save");
            visual.simulate_keystrokes("escape");
            draw(visual);
            visual.simulate_keystrokes("ctrl-alt-j");
            draw(visual);
            composable_tests::pump(&mut manager, &app, visual);
            composable_tests::publish(&mut manager, &mut renderer, &app, visual);
            draw(visual);
            assert!(visual.debug_bounds("plugin-ui-welcome-text").is_none());
            visual.simulate_keystrokes("ctrl-alt-v");
            draw(visual);
            composable_tests::pump(&mut manager, &app, visual);
            composable_tests::publish(&mut manager, &mut renderer, &app, visual);
            draw(visual);
            assert!(visual.debug_bounds("plugin-ui-welcome-text").is_some());

            hide_and_focus_editor(visual, &app, &panel_id);
            edit_show_panel(visual, "ctrl-j ctrl-v");
            composable_tests::pump(&mut manager, &app, visual);
            composable_tests::publish(&mut manager, &mut renderer, &app, visual);
            draw(visual);
            assert!(visual.debug_bounds("plugin-ui-welcome-text").is_none());
            click(visual, "shortcuts-edit-save");
            visual.simulate_keystrokes("escape");
            draw(visual);
            visual.simulate_keystrokes("ctrl-alt-v");
            draw(visual);
            composable_tests::pump(&mut manager, &app, visual);
            composable_tests::publish(&mut manager, &mut renderer, &app, visual);
            draw(visual);
            assert!(visual.debug_bounds("plugin-ui-welcome-text").is_none());
            visual.simulate_keystrokes("ctrl-j");
            draw(visual);
            assert!(visual.debug_bounds("shortcuts-pending").is_some());
            visual.simulate_keystrokes("ctrl-v");
            draw(visual);
            composable_tests::pump(&mut manager, &app, visual);
            composable_tests::publish(&mut manager, &mut renderer, &app, visual);
            draw(visual);
            assert!(visual.debug_bounds("shortcuts-pending").is_none());
            assert!(visual.debug_bounds("plugin-ui-welcome-text").is_some());

            hide_and_focus_editor(visual, &app, &panel_id);
            visual.simulate_keystrokes("ctrl-a");
            visual.simulate_input("native sibling will save");
            edit_show_panel(visual, "ctrl-s");
            click(visual, "shortcuts-edit-save");
            assert!(visual.debug_bounds("shortcuts-edit-confirmation").is_some());
            assert!(visual.debug_bounds("shortcuts-edit-replace").is_some());
            composable_tests::pump(&mut manager, &app, visual);
            composable_tests::publish(&mut manager, &mut renderer, &app, visual);
            draw(visual);
            assert!(visual.debug_bounds("plugin-ui-welcome-text").is_none());
            assert_eq!(std::fs::read_to_string(path).unwrap(), "original on disk");
            click(visual, "shortcuts-edit-replace");
            visual.simulate_keystrokes("escape");
            draw(visual);

            // Replacing Ctrl+S leaves SaveDocument's other binding intact and functional.
            visual.simulate_keystrokes("ctrl-alt-u");
            draw(visual);
            assert_eq!(
                std::fs::read_to_string(path).unwrap(),
                "native sibling will save"
            );
            visual.simulate_keystrokes("ctrl-a");
            visual.simulate_input("plugin command must not save this");
            visual.simulate_keystrokes("ctrl-s");
            draw(visual);
            composable_tests::pump(&mut manager, &app, visual);
            composable_tests::publish(&mut manager, &mut renderer, &app, visual);
            draw(visual);
            assert!(visual.debug_bounds("plugin-ui-welcome-text").is_some());
            assert_eq!(
                std::fs::read_to_string(path).unwrap(),
                "native sibling will save"
            );
        },
    );
}
