//! Exercise workspace layout persistence through the real editor save and startup paths.

use super::*;
use crate::{EditorApp, typography};
use editor_core::Workspace;
use gpui_base::dock::{DockAreaState, DockLayout};
use gpui_base::{
    Placement,
    dock::{DockPlacement, InsertTarget, PanelId},
};
use gpui_kit::{
    AppContext as _, Entity, TestAppContext, VisualTestContext, component::Root, gpui, px, size,
};
use plugin_runtime::{Installed, plugin_protocol::Manifest};
use std::{cell::RefCell, rc::Rc};

/// Remove only the isolated workspace's generated session file, including on assertion failure.
struct SessionFile(PathBuf);
impl Drop for SessionFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Ignore subpixel rounding from Base's measured split sizes while comparing all persisted fields.
fn assert_layout_eq(actual: DockAreaState, expected: DockAreaState) {
    fn rounded(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Number(number) => {
                *value = serde_json::json!((number.as_f64().unwrap() * 100.).round() / 100.);
            }
            serde_json::Value::Array(values) => values.iter_mut().for_each(rounded),
            serde_json::Value::Object(values) => values.values_mut().for_each(rounded),
            _ => {}
        }
    }
    let mut actual = serde_json::to_value(actual).unwrap();
    let mut expected = serde_json::to_value(expected).unwrap();
    rounded(&mut actual);
    rounded(&mut expected);
    assert_eq!(
        actual, expected,
        "restart must preserve the current split tree and sizes"
    );
}

/// Publish two distinct contributions of the same panel type through the host's regular sync.
fn publish_panels(app: &Entity<EditorApp>, installed: &Installed, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.extensions.update(cx, |owner, _| {
                owner.entries = vec![installed.clone()];
                owner.startup.clear();
            });
            app.sync_plugin_panels(window, cx);
        })
    });
}

/// A vertically rearranged center must survive saving and constructing a fresh editor.
#[gpui::test]
fn dock_layout_survives_editor_restart(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        crate::theme::apply_theme(crate::theme::builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let _session = SessionFile(
        SessionState::for_workspace(workspace.root())
            .file_path()
            .unwrap(),
    );
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let first_workspace = workspace.clone();
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(first_workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let first = slot.borrow_mut().take().unwrap();
    visual.update(|window, cx| {
        first.update(cx, |app, cx| {
            let editor = PanelId::from(app.editor_panel.entity_id());
            app.dock_area.update(cx, |area, cx| {
                let center = area.layout(DockPlacement::Center).unwrap();
                let explorer = center.panels().find(|id| *id != editor).unwrap();
                let node = center.find_panel_node(editor).unwrap();
                area.move_panel(
                    explorer,
                    InsertTarget::Split {
                        node,
                        placement: Placement::Bottom,
                        size: Some(px(260.)),
                    },
                    window,
                    cx,
                );
            });
        })
    });
    visual.run_until_parked();
    // Container resizing changes measured split sizes without a completed divider-drag event.
    visual.simulate_resize(size(px(1500.), px(900.)));
    let expected = visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        first.update(cx, |app, cx| {
            // Exit must capture measured sizes even without another finished drag event.
            app.shutdown_plugins(cx);
            app.dock_area.read(cx).dump(cx)
        })
    });
    let restored_slot = Rc::new(RefCell::new(None));
    let capture = restored_slot.clone();
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let restored = restored_slot.borrow_mut().take().unwrap();
    visual.simulate_resize(size(px(1500.), px(900.)));
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        let actual = restored.read(cx).dock_area.read(cx).dump(cx);
        assert_layout_eq(actual, expected);
    });
}

/// Async plugin startup must retain nested splits, moved panels, closed docks and stable identities.
#[gpui::test]
fn plugin_dock_layout_survives_delayed_startup(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        crate::theme::apply_theme(crate::theme::builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let _session = SessionFile(
        SessionState::for_workspace(workspace.root())
            .file_path()
            .unwrap(),
    );
    let mut manifest: Manifest = serde_json::from_str(include_str!(
        "../../../../../plugins/terminal/manifest.json"
    ))
    .unwrap();
    manifest.panels[0].default_visible = true;
    let mut second_panel = manifest.panels[0].clone();
    second_panel.id = "tasks".into();
    manifest.panels.push(second_panel);
    let installed = Installed {
        grants: manifest.permissions.clone(),
        manifest,
        digest: "fixture".into(),
        enabled: true,
        project_enabled: Default::default(),
        global_enabled: None,
        error: None,
    };
    let registry = workspace.root().join(".runtime-plugin-test");
    fs::create_dir_all(&registry).unwrap();
    fs::write(
        registry.join("registry.json"),
        serde_json::to_vec(&std::collections::BTreeMap::from([(
            installed.manifest.id.clone(),
            installed.clone(),
        )]))
        .unwrap(),
    )
    .unwrap();
    let first_workspace = workspace.clone();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(first_workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let first = slot.borrow_mut().take().unwrap();
    publish_panels(&first, &installed, visual);
    visual.update(|window, cx| {
        first.update(cx, |app, cx| {
            let explorer = crate::dock::panel_handle(app.explorer_panel.clone());
            let editor = crate::dock::panel_handle(app.editor_panel.clone());
            let terminal =
                crate::dock::panel_handle(app.plugin_panels["terminal/terminal"].clone());
            let tasks = crate::dock::panel_handle(app.plugin_panels["terminal/tasks"].clone());
            app.dock_area.update(cx, |area, cx| {
                area.remove_dock(DockPlacement::Bottom, window, cx);
                area.set_center(
                    DockLayout::h_split()
                        .child(DockLayout::tabs().panel_view(explorer, cx), Some(px(240.)))
                        .child(
                            DockLayout::v_split()
                                .child(DockLayout::tabs().panel_view(editor, cx), Some(px(500.)))
                                .child(DockLayout::tabs().panel_view(tasks, cx), Some(px(220.))),
                            None,
                        ),
                    window,
                    cx,
                );
                area.set_dock(
                    DockPlacement::Right,
                    DockLayout::tabs().panel_view(terminal, cx),
                    window,
                    cx,
                );
                area.set_dock_size(DockPlacement::Right, px(317.), window, cx);
                area.toggle_dock(DockPlacement::Right, window, cx);
                assert!(!area.is_dock_open(DockPlacement::Right));
            });
        })
    });
    visual.run_until_parked();
    let expected = visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        first.update(cx, |app, cx| {
            app.shutdown_plugins(cx);
            app.dock_area.read(cx).dump(cx)
        })
    });
    // Load a pre-rename workspace through the normal editor entry point and restore its dock tree.
    let legacy_session = visual.update(|_, cx| first.read(cx).session_state.clone());
    let serialized = serde_json::to_string(&legacy_session)
        .unwrap()
        .replace("terminal/terminal", "me.terminal/terminal")
        .replace("terminal/tasks", "me.terminal/tasks");
    let mut legacy_session: SessionState = serde_json::from_str(&serialized).unwrap();
    legacy_session.disabled_plugins = vec!["me.uninstalled-test".into()];
    legacy_session.save();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let restored = slot.borrow_mut().take().unwrap();
    visual.update(|_, cx| {
        let app = restored.read(cx);
        assert_eq!(app.session_state.disabled_plugins, ["uninstalled-test"]);
        assert!(app.pending_dock_restore);
        assert_layout_eq(
            app.session_state.dock_layout.clone().unwrap(),
            expected.clone(),
        );
    });
    publish_panels(&restored, &installed, visual);
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        let app = restored.read(cx);
        assert!(!app.pending_dock_restore);
        assert_layout_eq(app.dock_area.read(cx).dump(cx), expected);
        for key in ["terminal/terminal", "terminal/tasks"] {
            let id = PanelId::from(app.plugin_panels[key].entity_id());
            assert!(
                app.dock_area.read(cx).panel(id).is_some(),
                "restore must bind the original plugin view"
            );
        }
    });
}
