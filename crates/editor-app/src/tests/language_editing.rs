//! Installed formatting roles and delayed linked services exercise observable native documents and files.
use super::{
    editing_fixture,
    tag_editing::{assert_text, await_condition, await_text, text},
};
use crate::*;
use gpui_kit::{TestAppContext, VisualTestContext, gpui};
mod formatting;
mod linked;
mod persisted;
mod queue;
mod semantic;
use plugin_runtime::{Manager, Package, plugin_protocol as protocol};
use std::{cell::RefCell, time::Duration};

/// Initialize an actual shell with an explicitly granted plugin manager and the production publication path.
pub(super) fn shell<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
    packages: Vec<Package>,
) -> (Entity<EditorApp>, &'a mut VisualTestContext, Manager) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::bind_editor_shell_keys(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let workspace = Workspace::open(root).unwrap();
    let mut manager = Manager::open(
        root.join(".runtime-plugin-test"),
        protocol::Environment {
            workspace: root.display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    for package in packages {
        if package.manifest.id == "xml" {
            editing_fixture::install_xml(&mut manager, &package);
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
    visual.simulate_resize(size(px(1200.), px(800.)));
    crate::extensions::lsp_tests::publish(&app, &mut manager, visual);
    (app, visual, manager)
}

/// Paint a real input handler, then each scenario can move and input with no semantic-cache wait.
pub(super) fn open(app: &Entity<EditorApp>, path: &Path, visual: &mut VisualTestContext) {
    visual.update(|window, cx| {
        window.activate_window();
        app.update(cx, |app, cx| app.open_file(path.into(), window, cx));
        window.draw(cx).clear(cx);
    });
    visual.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

/// Drawing installs input routing but does not wait for semantic results after the new caret.
fn immediate_caret(app: &Entity<EditorApp>, offset: usize, visual: &mut VisualTestContext) {
    // Visit non-name text in its own frame, so a previous valid group cannot warm this new caret.
    visual.update(|window, cx| {
        app.read(cx).editor.clone().update(cx, |editor, cx| {
            editor.set_selected_range(0..0, cx);
            // Public selection mutation leaves redraw to its caller, just as native gestures do.
            cx.notify();
        });
        window.draw(cx).clear(cx);
    });
    visual.run_until_parked();
    visual.update(|window, cx| {
        app.read(cx).editor.clone().update(cx, |editor, cx| {
            editor.set_selected_range(offset..offset, cx);
            editor.focus(window, cx);
            cx.notify();
        });
        window.draw(cx).clear(cx);
    });
}

/// Pump visible native frames for a bounded external response; all assertions observe public text or files.
fn pump(visual: &mut VisualTestContext, milliseconds: u64) {
    let deadline = std::time::Instant::now() + Duration::from_millis(milliseconds);
    while std::time::Instant::now() < deadline {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        std::thread::sleep(Duration::from_millis(10));
    }
}
