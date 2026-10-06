//! Actual independent file layouts retire on faults and preserve text or binary recovery.
use super::package_ui_test_support::*;
use super::*;
use gpui_kit::{TestAppContext, VisualTestContext, gpui};
use plugin_runtime::Manager;

/// A real WASM trap withdraws the center and functions, with different recovery for text and binary files.
#[gpui::test]
#[ignore = "build current layout-example through scripts/build-layout-example.ps1 first"]
fn real_layout_fault_preserves_native_text_and_binary_retry(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let text_path = directory.path().join("draft.layout");
    let image_path = directory.path().join("picture.png");
    std::fs::write(&text_path, "original draft").unwrap();
    image::RgbaImage::from_pixel(3, 2, image::Rgba([20, 60, 100, 255]))
        .save(&image_path)
        .unwrap();
    let mut manager = Manager::open(
        data.path().into(),
        protocol::Environment {
            workspace: directory.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let package = package("fault-layout", "Fault Layout");
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    visual.simulate_resize(size(px(1100.), px(800.)));
    visual.update(|window, cx| {
        pump(&app, &mut manager, cx);
        app.update(cx, |app, cx| app.open_file(text_path, window, cx));
    });
    draw(&app, &mut manager, visual);
    let editor = visual.debug_bounds("editor-source-pane").unwrap();
    visual.simulate_click(editor.center(), Modifiers::default());
    visual.simulate_input("未保存");
    draw(&app, &mut manager, visual);
    let draft = visual.update(|_, cx| app.read(cx).editor.read(cx).text().to_string());
    let hide = visual
        .debug_bounds("plugin-ui-layout-content")
        .unwrap()
        .center();
    visual.simulate_click(hide, Modifiers::default());
    draw(&app, &mut manager, visual);
    assert!(visual.debug_bounds("editor-source-pane").is_none());
    trap_layout(&mut manager);
    draw(&app, &mut manager, visual);
    assert!(visual.debug_bounds("editor-plugin-layout").is_none());
    assert!(
        visual
            .debug_bounds("plugin-tool-fault-layout/layout/layout-content")
            .is_none()
    );
    let editor = visual
        .debug_bounds("editor-source-pane")
        .expect("text fallback must restore the same editor");
    assert_eq!(
        visual.update(|_, cx| app.read(cx).editor.read(cx).text().to_string()),
        draft
    );
    visual.simulate_click(editor.center(), Modifiers::default());
    visual.simulate_keystrokes("ctrl-z");
    draw(&app, &mut manager, visual);
    assert_eq!(
        visual.update(|_, cx| app.read(cx).editor.read(cx).text().to_string()),
        "original draft"
    );
    visual.simulate_keystrokes("ctrl-y");
    draw(&app, &mut manager, visual);
    assert_eq!(
        visual.update(|_, cx| app.read(cx).editor.read(cx).text().to_string()),
        draft
    );
    manager.restart_plugin("fault-layout").unwrap();
    draw(&app, &mut manager, visual);
    binary_fault_retains_tab_and_retries(&app, &mut manager, image_path, visual);
}

/// Binary recovery uses the visible retry control and the ordinary runtime lifecycle operation.
fn binary_fault_retains_tab_and_retries(
    app: &Entity<EditorApp>,
    manager: &mut Manager,
    image_path: PathBuf,
    visual: &mut VisualTestContext,
) {
    visual.update(|window, cx| app.update(cx, |app, cx| app.open_file(image_path, window, cx)));
    draw(app, manager, visual);
    assert!(visual.debug_bounds("plugin-file-image").is_some());
    trap_layout(manager);
    draw(app, manager, visual);
    assert!(visual.debug_bounds("file-view-unavailable").is_some());
    assert!(visual.debug_bounds("editor-tab-1").is_some());
    assert!(visual.debug_bounds("editor-source-pane").is_none());
    assert!(manager.image_resources().is_empty());
    let retry = visual
        .debug_bounds("file-view-retry")
        .expect("binary fault must offer native retry");
    visual.simulate_click(retry.center(), Modifiers::default());
    visual.run_until_parked();
    let restart = visual
        .update(|_, cx| {
            app.read(cx)
                .extensions
                .read(cx)
                .worker
                .recorded
                .lock()
                .unwrap()
                .try_iter()
                .find_map(|work| {
                    if let Work::Restart(id) = work {
                        Some(id)
                    } else {
                        None
                    }
                })
        })
        .expect("actual retry button must enqueue the standard lifecycle operation");
    assert_eq!(restart, "fault-layout");
    manager.restart_plugin(&restart).unwrap();
    draw(app, manager, visual);
    assert!(visual.debug_bounds("plugin-file-image").is_some());
    assert!(visual.debug_bounds("file-view-unavailable").is_none());
    assert!(visual.debug_bounds("editor-source-pane").is_none());
    manager.uninstall("fault-layout", true).unwrap();
    draw(app, manager, visual);
    assert!(manager.image_resources().is_empty());
    assert!(visual.debug_bounds("editor-tab-1").is_some());
}

/// A guest diagnostic trap passes through declared command dispatch and normal runtime fault retirement.
fn trap_layout(manager: &mut Manager) {
    let error = manager
        .invoke_command("fault-layout", "diagnostic-trap", serde_json::Value::Null)
        .expect_err("the explicit guest diagnostic must fail");
    assert!(
        manager.instance_id("fault-layout").is_none(),
        "a real guest fault must retire its resources: {error:#}"
    );
    assert!(manager.installed["fault-layout"].error.is_some());
}
