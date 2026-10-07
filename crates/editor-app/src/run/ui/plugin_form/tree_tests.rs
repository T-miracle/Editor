//! End-to-end tree transactions use installed independent WASM providers and the real owned dialog.
use crate::extensions::native_configuration_tests::{
    Driver, click, click_at, edit, fixture, open_form,
};
use crate::*;
use gpui_kit::{TestAppContext, gpui};
use plugin_runtime::Manager;

/// Read the immutable observable draft, without exposing a test-only mutation seam in production.
fn draft(
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
) -> editor_core::RunConfigSet {
    cx.update(|_, cx| {
        app.read(cx)
            .run_form
            .as_ref()
            .unwrap()
            .read(cx)
            .plugin
            .as_ref()
            .unwrap()
            .draft
            .clone()
    })
}
fn chosen(app: &Entity<EditorApp>, cx: &mut gpui_kit::VisualTestContext) -> String {
    cx.update(|_, cx| {
        app.read(cx)
            .run_form
            .as_ref()
            .unwrap()
            .read(cx)
            .plugin
            .as_ref()
            .unwrap()
            .selected
            .clone()
            .unwrap()
    })
}
fn choose(app: &Entity<EditorApp>, id: &str, cx: &mut gpui_kit::VisualTestContext) {
    // GPUI's debug selectors require static strings; this bounded test window owns the leaked name.
    let selector: &'static str = format!("run-config-tree-{id}").leak();
    let point = cx.debug_bounds(selector).unwrap().center();
    click_at(cx, point);
    assert_eq!(chosen(app, cx), id);
}
fn add(
    driver: &mut Driver,
    manager: &mut Manager,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
) -> String {
    click(cx, "run-config-add");
    driver.wait(manager, app, cx, |cx| {
        cx.debug_bounds("run-template-configuration-alpha-program")
            .is_some()
    });
    click(cx, "run-template-configuration-alpha-program");
    driver.wait(manager, app, cx, |cx| {
        cx.debug_bounds("plugin-ui-name").is_some()
    });
    chosen(app, cx)
}
#[track_caller]
fn settle(
    driver: &mut Driver,
    manager: &mut Manager,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
) {
    driver.wait(manager, app, cx, |cx| {
        cx.update(|_, cx| {
            !app.read(cx)
                .run_form
                .as_ref()
                .unwrap()
                .read(cx)
                .plugin
                .as_ref()
                .unwrap()
                .busy()
        })
    });
    let error = cx.update(|_, cx| {
        app.read(cx)
            .run_form
            .as_ref()
            .unwrap()
            .read(cx)
            .plugin
            .as_ref()
            .unwrap()
            .error
            .clone()
    });
    assert!(error.is_none(), "native form error: {error:?}");
}

/// Pointer drag traverses actual painted hit regions and the production drop handlers.
fn drag_to(cx: &mut gpui_kit::VisualTestContext, source: &str, target: &str) {
    let source: &'static str = source.to_owned().leak();
    let target: &'static str = target.to_owned().leak();
    let start = cx.debug_bounds(source).unwrap().center();
    let end = cx.debug_bounds(target).unwrap().center();
    cx.simulate_event(gpui::MouseDownEvent {
        button: MouseButton::Left,
        position: start,
        modifiers: Default::default(),
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(gpui::MouseMoveEvent {
        position: point(start.x + px(10.), start.y + px(10.)),
        pressed_button: Some(MouseButton::Left),
        modifiers: Default::default(),
    });
    cx.run_until_parked();
    cx.simulate_event(gpui::MouseMoveEvent {
        position: end,
        pressed_button: Some(MouseButton::Left),
        modifiers: Default::default(),
    });
    cx.run_until_parked();
    cx.simulate_event(gpui::MouseUpEvent {
        button: MouseButton::Left,
        position: end,
        modifiers: Default::default(),
        click_count: 1,
    });
    cx.run_until_parked();
}

/// Moving folders, category order and root drops are verified through real pointer interaction.
#[gpui::test]
#[ignore = "build configuration examples and terminal package with the public SDK first"]
fn plugin_configuration_tree_native_drag_reorder_and_cycle_rejection(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (mut manager, app, cx) = fixture(cx, root.path());
    let mut driver = Driver::default();
    driver.frame(&mut manager, &app, cx);
    open_form(&app, cx);
    click(cx, "run-config-folder");
    let first = chosen(&app, cx);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    click(cx, "run-config-folder");
    let nested = chosen(&app, cx);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(
        draft(&app, cx).tree_parent(&nested).as_deref(),
        Some(first.as_str())
    );
    let configuration = add(&mut driver, &mut manager, &app, cx);
    settle(&mut driver, &mut manager, &app, cx);
    let before = draft(&app, cx).tree.clone();
    drag_to(
        cx,
        &format!("run-config-tree-{first}"),
        &format!("run-config-tree-{nested}"),
    );
    assert_eq!(
        draft(&app, cx).tree,
        before,
        "rejecting cycles keeps the tree intact"
    );
    // Root target is below the last row, away from any child drop handler.
    let start = cx
        .debug_bounds(format!("run-config-tree-{nested}").leak())
        .unwrap()
        .center();
    let bounds = cx.debug_bounds("run-config-tree-list").unwrap();
    let end = point(bounds.center().x, bounds.bottom() - px(10.));
    cx.simulate_event(gpui::MouseDownEvent {
        button: MouseButton::Left,
        position: start,
        modifiers: Default::default(),
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(gpui::MouseMoveEvent {
        position: point(start.x + px(10.), start.y + px(10.)),
        pressed_button: Some(MouseButton::Left),
        modifiers: Default::default(),
    });
    cx.run_until_parked();
    cx.simulate_event(gpui::MouseMoveEvent {
        position: end,
        pressed_button: Some(MouseButton::Left),
        modifiers: Default::default(),
    });
    cx.run_until_parked();
    cx.simulate_event(gpui::MouseUpEvent {
        button: MouseButton::Left,
        position: end,
        modifiers: Default::default(),
        click_count: 1,
    });
    cx.run_until_parked();
    assert!(draft(&app, cx).tree_parent(&nested).is_none());
    assert_eq!(
        draft(&app, cx).tree_parent(&configuration).as_deref(),
        Some(nested.as_str())
    );
    drag_to(
        cx,
        &format!("run-config-tree-{nested}"),
        &format!("run-config-before-{first}"),
    );
    assert_eq!(draft(&app, cx).tree_children(None), [nested, first]);
    assert!(driver.launches.is_empty());
    manager.shutdown();
}

/// Multi-record edits and virtual folders obey the Apply baseline, independent-copy and Cancel rules.
#[gpui::test]
#[ignore = "build configuration examples and terminal package with the public SDK first"]
fn plugin_configuration_tree_apply_copy_cancel_and_save_all(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (mut manager, app, cx) = fixture(cx, root.path());
    let mut driver = Driver::default();
    driver.frame(&mut manager, &app, cx);
    let parent = open_form(&app, cx);
    click(cx, "run-config-folder");
    let folder = chosen(&app, cx);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(
        cx.update(|_, cx| app.read(cx).run_form.is_some()),
        "renaming a folder must not submit the window"
    );
    let first = add(&mut driver, &mut manager, &app, cx);
    edit(cx, "plugin-ui-name", "first saved");
    settle(&mut driver, &mut manager, &app, cx);
    assert_eq!(
        draft(&app, cx).tree_parent(&first).as_deref(),
        Some(folder.as_str())
    );
    click(cx, "run-config-copy");
    let copy = chosen(&app, cx);
    settle(&mut driver, &mut manager, &app, cx);
    assert_ne!(first, copy);
    assert!(
        draft(&app, cx).plugin_configurations[&copy]
            .name
            .contains("副本")
            || draft(&app, cx).plugin_configurations[&copy]
                .name
                .contains("copy")
    );
    edit(cx, "plugin-ui-name", "copy draft");
    settle(&mut driver, &mut manager, &app, cx);
    choose(&app, &first, cx);
    assert_eq!(
        draft(&app, cx).plugin_configurations[&first].name,
        "first saved"
    );
    click(cx, "run-config-apply");
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.update(|_, cx| app.read(cx).run_controls.configurations().len() == 1)
    });
    let baseline = cx.update(|_, cx| app.read(cx).run_controls.configuration_set());
    assert_eq!(baseline.tree_ancestors(&first), [folder.clone()]);
    assert!(baseline.find(&copy).is_none());
    assert!(baseline.selected.is_none());
    edit(cx, "plugin-ui-name", "discard after apply");
    settle(&mut driver, &mut manager, &app, cx);
    click(cx, "run-config-cancel");
    *cx = gpui_kit::VisualTestContext::from_window(parent, &cx.cx);
    assert_eq!(
        cx.update(|_, cx| app
            .read(cx)
            .run_controls
            .configuration(&first)
            .unwrap()
            .name
            .clone()),
        "first saved"
    );
    let parent = open_form(&app, cx);
    choose(&app, &folder, cx);
    let second = add(&mut driver, &mut manager, &app, cx);
    edit(cx, "plugin-ui-name", "second saved");
    settle(&mut driver, &mut manager, &app, cx);
    choose(&app, &first, cx);
    settle(&mut driver, &mut manager, &app, cx);
    edit(cx, "plugin-ui-name", "first changed");
    settle(&mut driver, &mut manager, &app, cx);
    choose(&app, &second, cx);
    click(cx, "run-config-save");
    *cx = gpui_kit::VisualTestContext::from_window(parent, &cx.cx);
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.update(|_, cx| app.read(cx).run_form.is_none())
    });
    let saved = cx.update(|_, cx| app.read(cx).run_controls.configuration_set());
    assert_eq!(saved.plugin_configurations[&first].name, "first changed");
    assert_eq!(saved.plugin_configurations[&second].name, "second saved");
    assert_eq!(saved.selected.as_deref(), Some(second.as_str()));
    assert_eq!(saved.tree_children(Some(&folder)), [first, second]);
    assert!(!root.path().join("新建文件夹").exists());
    assert!(driver.launches.is_empty());
    manager.shutdown();
}

/// Deletion remains staged, and X/Escape offer Save, Discard and Continue instead of silently losing edits.
#[gpui::test]
#[ignore = "build configuration examples and terminal package with the public SDK first"]
fn plugin_configuration_tree_delete_and_three_way_close(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (mut manager, app, cx) = fixture(cx, root.path());
    let mut driver = Driver::default();
    driver.frame(&mut manager, &app, cx);
    let parent = open_form(&app, cx);
    click(cx, "run-config-folder");
    let folder = chosen(&app, cx);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let configuration = add(&mut driver, &mut manager, &app, cx);
    settle(&mut driver, &mut manager, &app, cx);
    click(cx, "run-config-save");
    *cx = gpui_kit::VisualTestContext::from_window(parent, &cx.cx);
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.update(|_, cx| app.read(cx).run_form.is_none())
    });
    let parent = open_form(&app, cx);
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.debug_bounds("plugin-ui-name").is_some()
    });
    choose(&app, &folder, cx);
    click(cx, "run-config-delete");
    assert!(cx.debug_bounds("run-config-confirm-delete").is_some());
    click(cx, "run-config-confirm-delete");
    assert!(draft(&app, cx).configurations.is_empty());
    assert_eq!(
        cx.update(|_, cx| app.read(cx).run_controls.configurations().len()),
        1
    );
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    click(cx, "run-config-close-keep");
    assert!(cx.debug_bounds("run-config-form").is_some());
    click(cx, "app-dialog-close");
    click(cx, "run-config-close-discard");
    *cx = gpui_kit::VisualTestContext::from_window(parent, &cx.cx);
    assert!(cx.update(|_, cx| {
        app.read(cx)
            .run_controls
            .configuration(&configuration)
            .is_some()
    }));
    let parent = open_form(&app, cx);
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.debug_bounds("plugin-ui-name").is_some()
    });
    edit(cx, "plugin-ui-name", "save when closing");
    settle(&mut driver, &mut manager, &app, cx);
    click(cx, "app-dialog-close");
    click(cx, "run-config-close-save");
    *cx = gpui_kit::VisualTestContext::from_window(parent, &cx.cx);
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.update(|_, cx| app.read(cx).run_form.is_none())
    });
    assert_eq!(
        cx.update(|_, cx| app
            .read(cx)
            .run_controls
            .configuration(&configuration)
            .unwrap()
            .name
            .clone()),
        "save when closing"
    );
    assert!(driver.launches.is_empty());
    manager.shutdown();
}
