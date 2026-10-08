//! Real XML suggestions must survive the native input, revision and menu application path.
use super::*;
use gpui_kit::VisualTestContext;
use std::time::{Duration, Instant};

/// A Chinese attribute typed after emoji selects a genuine installed guest proposal via Enter.
#[gpui::test]
#[ignore = "build current xml package with scripts/build-plugins.ps1 first"]
fn installed_xml_completion_applies_through_native_input(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("draft.xml");
    let original = "<!-- 中文🙂 -->\n<root><item 名称=\"old\"/><item ";
    std::fs::write(&path, original).unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let mut manager = plugin_runtime::Manager::open(
        workspace.root().join(".runtime-plugin-test"),
        protocol::Environment {
            os: std::env::consts::OS.into(),
            workspace: workspace.root().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let package = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/xml.zip"),
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    visual.update(|window, cx| {
        window.activate_window();
        app.update(cx, |app, cx| app.open_file(path.clone(), window, cx));
    });
    visual.simulate_resize(size(px(1200.), px(800.)));
    // Publish the actual manager's prepared service alongside its package resources.
    crate::extensions::lsp_tests::publish(&app, &mut manager, visual);
    let server = visual.update(|_, cx| app.read(cx).language_servers["xml"].clone());
    server.prepare_until_ready().unwrap();
    let editor = visual.update(|window, cx| {
        let editor = app.read(cx).editor.clone();
        editor.update(cx, |state, cx| state.focus(window, cx));
        editor
    });
    visual.simulate_keystrokes("ctrl-end");
    visual.simulate_input("名");
    await_attribute(&editor, visual);
    assert!(visual.debug_bounds("editor-completion-card").is_some());
    visual.update(|_, cx| apply_theme(builtin_theme(true), cx));
    visual.run_until_parked();
    visual.simulate_keystrokes("enter");
    visual.run_until_parked();
    assert_eq!(
        visual.update(|_, cx| editor.read(cx).text().to_string()),
        format!("{original}名称=\"\"")
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    assert!(visual.debug_bounds("editor-completion-card").is_none());
    manager.disable("xml").unwrap();
    crate::extensions::lsp_tests::publish(&app, &mut manager, visual);
    assert!(visual.update(|_, cx| editor.read(cx).lsp().completion_provider.is_none()));
    assert!(!server.is_active());
}

/// Pump native UI tasks while the real stdio service finishes, retaining an explicit wall-clock bound.
fn await_attribute(editor: &Entity<EditorState>, visual: &mut VisualTestContext) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        if visual.update(|_, cx| {
            let menu = editor.read(cx).completion_menu_state();
            menu.open && menu.items.iter().any(|item| item.label == "名称")
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "installed XML input never presented the live attribute suggestion"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
