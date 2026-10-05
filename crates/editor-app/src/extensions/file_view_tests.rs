//! The actual Image component enters the native file container through the existing worker publication.
use super::*;
use gpui_kit::{TestAppContext, gpui};
use plugin_runtime::{ImageState, Manager, plugin_protocol::Environment};
use std::io::Cursor;

/// Feed the real manager's immutable publication to the deterministic native worker boundary.
fn publish(app: &Entity<EditorApp>, manager: &mut Manager, cx: &mut App) {
    let context = app
        .read(cx)
        .plugin_file_context(app.read(cx).active_tab_index().unwrap())
        .unwrap();
    manager
        .event(
            "svg",
            Some("preview".into()),
            PluginEvent::FilePreview {
                file: Some(context),
            },
        )
        .unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    let resources = loop {
        let resources = manager.image_resources();
        if resources
            .values()
            .all(|image| !matches!(image.state, ImageState::Loading))
        {
            break resources;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    };
    let views = manager.live["svg"]
        .views
        .iter()
        .map(|(panel, view)| (format!("svg/{panel}"), view.clone()))
        .collect();
    let prepared = images::VectorRenderer::default().prepare_resources(&views, &resources);
    let owner = app.read(cx).extensions.clone();
    owner.update(cx, |owner, cx| {
        let mut state = owner.worker.state.lock().unwrap();
        state.views = views;
        state.images = prepared;
        drop(state);
        owner.poll(cx);
    });
    let panel = app.read(cx).plugin_panels["svg/preview"].clone();
    panel.update(cx, |panel, cx| panel.poll(cx));
}

/// Native drawing, resizing, decode errors, retry, text switching and file closure use the shipped package.
#[gpui::test]
#[ignore = "build current Image package with scripts/build-plugins.ps1 -Packages svg first"]
fn image_package_draws_readonly_formats_in_the_native_file_container(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let package =
        Package::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/svg.zip"))
            .unwrap();
    let environment = Environment {
        workspace: directory.path().display().to_string(),
        ..Default::default()
    };
    let mut manager = Manager::open(data.path().to_path_buf(), environment).unwrap();
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
    visual.simulate_resize(size(px(1000.), px(700.)));
    visual.update(|_, cx| {
        let owner = app.read(cx).extensions.clone();
        owner.update(cx, |owner, cx| {
            // Native package assets use the same installed root as this real Manager.
            owner.root = data.path().to_path_buf();
            owner.worker.state.lock().unwrap().entries =
                manager.installed.values().cloned().collect();
            owner.poll(cx);
        });
    });
    for (extension, format) in [
        ("png", image::ImageFormat::Png),
        ("jpg", image::ImageFormat::Jpeg),
        ("gif", image::ImageFormat::Gif),
        ("webp", image::ImageFormat::WebP),
    ] {
        let mut bytes = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            100,
            50,
            image::Rgb([50, 120, 210]),
        ))
        .write_to(&mut bytes, format)
        .unwrap();
        let path = directory.path().join(format!("picture.{extension}"));
        std::fs::write(&path, bytes.get_ref()).unwrap();
        visual.update(|window, cx| {
            app.update(cx, |app, cx| app.open_file(path.clone(), window, cx));
            publish(&app, &mut manager, cx);
            window.draw(cx).clear(cx);
        });
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            visual.debug_bounds("plugin-file-image").is_some(),
            "native {extension} drawing"
        );
        assert!(visual.debug_bounds("editor-source-pane").is_none());
        assert!(
            visual
                .debug_bounds("editor-preview-mode-separator")
                .is_none(),
            "read-only files must not offer source-editing modes"
        );
        visual.simulate_input("images cannot receive text");
        visual.update(|_, cx| app.update(cx, |app, cx| app.save_current(cx)));
        assert_eq!(std::fs::read(path).unwrap(), *bytes.get_ref());
    }
    // File replacement advances authority, retires old pixels and exposes a working retry action.
    let path = directory
        .path()
        .join("picture.webp")
        .canonicalize()
        .unwrap();
    std::fs::write(&path, b"damaged image").unwrap();
    visual.update(|window, cx| {
        app.update(cx, |app, cx| app.retry_file_view(cx));
        publish(&app, &mut manager, cx);
        window.draw(cx).clear(cx);
    });
    assert!(visual.debug_bounds("plugin-file-image").is_none());
    assert!(
        visual
            .debug_bounds("plugin-file-image-status-preview.image_failed")
            .is_some()
    );
    visual.simulate_resize(size(px(700.), px(450.)));
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let text = directory.path().join("notes.txt");
    std::fs::write(&text, "ordinary text").unwrap();
    visual.update(|window, cx| app.update(cx, |app, cx| app.open_file(text, window, cx)));
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("editor-source-pane").is_some());
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            let active = app.active_path.clone().unwrap();
            app.close_tab(active, window, cx);
        })
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("editor-tabs-container").is_some());
    assert!(visual.debug_bounds("editor-source-pane").is_none());
}
