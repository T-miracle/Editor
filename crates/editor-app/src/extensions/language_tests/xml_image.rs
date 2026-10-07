//! Independent delivered language and display packages compose through one native SVG session.
use super::*;
use crate::extensions::composable_tests::{publish, pump};
use gpui_kit::VisualTestContext;

/// Install, revoke and reinstall each actual package without a plugin-to-plugin dependency.
#[gpui::test]
#[ignore = "build current xml and svg packages with scripts/build-plugins.ps1 first"]
fn xml_and_image_packages_share_unsaved_svg_and_independent_lifecycles(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("drawing.svg");
    let original = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"160\" height=\"100\"/>";
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
    let package = |id: &str| {
        Package::read(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../dist/plugins/{id}.zip")),
        )
        .unwrap()
    };
    let image = package("svg");
    let xml = package("xml");
    manager
        .install(&image, image.manifest.permissions.clone())
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
    visual.simulate_resize(size(px(1400.), px(900.)));
    let mut renderer = images::VectorRenderer::default();
    settle(&mut manager, &mut renderer, &app, visual);
    assert_eq!(language(&app, visual), "text");
    assert!(visual.debug_bounds("plugin-ui-preview-canvas").is_some());
    let editor = visual.update(|_, cx| app.read(cx).editor.clone());

    // Adding language support to the open display document preserves its only editor entity.
    manager
        .install(&xml, xml.manifest.permissions.clone())
        .unwrap();
    publish_languages(&app, &manager, visual);
    settle(&mut manager, &mut renderer, &app, visual);
    assert_eq!(language(&app, visual), "xml");
    assert_eq!(visual.update(|_, cx| app.read(cx).editor.clone()), editor);
    assert!(visual.debug_bounds("editor-source-pane").is_some());
    assert!(visual.debug_bounds("plugin-ui-preview-canvas").is_some());
    visual.update(|window, cx| editor.update(cx, |state, cx| state.focus(window, cx)));
    visual.simulate_keystrokes("ctrl-a");
    let unsaved = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"320\" height=\"100\"><!-- 未保存 中文 😀 --></svg>";
    visual.simulate_input(unsaved);
    visual.run_until_parked();
    settle(&mut manager, &mut renderer, &app, visual);
    assert_eq!(
        visual.update(|_, cx| editor.read(cx).value().to_string()),
        unsaved
    );
    assert!(
        serde_json::to_string(manager.live["svg"].views["preview"].as_ref())
            .unwrap()
            .contains("未保存 中文 😀")
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    visual.update(|_, cx| {
        let owner = app.read(cx).extensions.read(cx);
        let state = owner.worker.state.lock().unwrap();
        assert!(
            state.images["svg/preview/canvas/preview-canvas"]
                .iter()
                .any(Option::is_some)
        );
    });
    // The local controls retain visible source and drawing bounds under the other theme.
    visual.update(|_, cx| apply_theme(builtin_theme(true), cx));
    visual.run_until_parked();
    assert!(visual.debug_bounds("editor-source-pane").is_some());
    assert!(visual.debug_bounds("plugin-ui-preview-canvas").is_some());

    manager.disable("xml").unwrap();
    publish_languages(&app, &manager, visual);
    settle(&mut manager, &mut renderer, &app, visual);
    assert_eq!(language(&app, visual), "text");
    assert!(visual.debug_bounds("plugin-ui-preview-canvas").is_some());
    assert_eq!(
        visual.update(|_, cx| editor.read(cx).value().to_string()),
        unsaved
    );

    manager.enable("xml").unwrap();
    manager.disable("svg").unwrap();
    publish_languages(&app, &manager, visual);
    settle(&mut manager, &mut renderer, &app, visual);
    assert_eq!(language(&app, visual), "xml");
    assert!(visual.debug_bounds("plugin-ui-preview-canvas").is_none());
    assert_eq!(
        visual.update(|_, cx| editor.read(cx).value().to_string()),
        unsaved
    );

    manager.enable("svg").unwrap();
    settle(&mut manager, &mut renderer, &app, visual);
    assert!(visual.debug_bounds("plugin-ui-preview-canvas").is_some());
    assert!(
        serde_json::to_string(manager.live["svg"].views["preview"].as_ref())
            .unwrap()
            .contains("未保存 中文 😀")
    );
    for id in ["xml", "svg"] {
        manager.uninstall(id, false).unwrap();
    }
    publish_languages(&app, &manager, visual);
    settle(&mut manager, &mut renderer, &app, visual);
    assert_eq!(language(&app, visual), "text");
    assert!(visual.debug_bounds("plugin-ui-preview-canvas").is_none());
    assert_eq!(visual.update(|_, cx| app.read(cx).editor.clone()), editor);
    assert_eq!(
        visual.update(|_, cx| editor.read(cx).value().to_string()),
        unsaved
    );
}

/// Publication and preview events follow the existing production worker seam in both directions.
fn settle(
    manager: &mut plugin_runtime::Manager,
    renderer: &mut images::VectorRenderer,
    app: &Entity<EditorApp>,
    cx: &mut VisualTestContext,
) {
    for _ in 0..5 {
        pump(manager, app, cx);
        manager.poll();
        publish(manager, renderer, app, cx);
    }
}

/// Observe native editor recognition rather than a declaration or private registry field.
fn language(app: &Entity<EditorApp>, cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| app.read(cx).editor.read(cx).language_name().to_string())
}
