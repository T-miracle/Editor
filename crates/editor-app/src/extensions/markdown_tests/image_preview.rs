//! Delivered image declarations cross permission checks, real I/O, worker decoding and native layout.
use super::*;
use harness::NativeMarkdown;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    time::{Duration, Instant},
};

/// Wait for real background completion while still publishing and drawing through the native worker seam.
pub(super) fn complete(fixture: &mut NativeMarkdown, ui: &mut gpui_kit::VisualTestContext) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        fixture.settle(ui);
        let resources = fixture.manager.image_resources();
        if !resources.is_empty()
            && resources
                .values()
                .all(|image| !matches!(image.state, plugin_runtime::ImageState::Loading))
        {
            fixture.settle(ui);
            return;
        }
        assert!(Instant::now() < deadline, "image workers did not complete");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Content-encoded PNG bytes avoid fixtures that depend on a user's files or the real network.
pub(super) fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        width,
        height,
        image::Rgba([230, 40, 20, 255]),
    ));
    let mut output = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut output, image::ImageFormat::Png)
        .unwrap();
    output.into_inner()
}

/// Borrow the pixels produced by the real worker; retaining this Arc must not keep a retired GPU texture alive.
fn published_pixels(
    fixture: &NativeMarkdown,
    ui: &mut gpui_kit::VisualTestContext,
) -> std::sync::Arc<gpui_kit::RenderImage> {
    ui.update(|_, cx| {
        fixture
            .app
            .read(cx)
            .extensions
            .read(cx)
            .worker
            .state
            .lock()
            .unwrap()
            .images
            .photos
            .values()
            .find_map(|photo| {
                photo
                    .decoded
                    .as_ref()
                    .ok()
                    .and_then(Option::as_ref)
                    .map(|bitmap| bitmap.image.clone())
            })
            .expect("the delivered image was decoded by the worker")
    })
}

/// Retiring a delivered plugin must release the actual atlas entry, beyond dropping the worker's bytes.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_image_atlas_released_on_disable(cx: &mut TestAppContext) {
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", "![image](img.png)")]);
    std::fs::write(fixture.directory.path().join("img.png"), png(160, 120)).unwrap();
    fixture.open("notes.md", ui);
    complete(&mut fixture, ui);
    let pixels = published_pixels(&fixture, ui);
    assert!(ui.update(|window, _| window.has_image_atlas_entry(&pixels)));

    fixture.manager.disable("markdown").unwrap();
    fixture.settle(ui);
    assert!(
        !ui.update(|window, _| window.has_image_atlas_entry(&pixels)),
        "a disabled plugin's pixels must leave the sprite atlas"
    );
}

/// Native source replacement retires the old texture even while this test still owns its immutable pixels.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_image_atlas_retires_on_source_revision(cx: &mut TestAppContext) {
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", "![old](first.png)")]);
    for name in ["first.png", "second.png"] {
        std::fs::write(fixture.directory.path().join(name), png(160, 120)).unwrap();
    }
    fixture.open("notes.md", ui);
    complete(&mut fixture, ui);
    let old = published_pixels(&fixture, ui);
    assert!(ui.update(|window, _| window.has_image_atlas_entry(&old)));

    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-a");
    ui.simulate_input("![new](second.png)");
    ui.run_until_parked();
    complete(&mut fixture, ui);
    let current = published_pixels(&fixture, ui);
    assert_ne!(
        old.id, current.id,
        "the new version has new authorized pixels"
    );
    assert!(ui.update(|window, _| window.has_image_atlas_entry(&current)));
    assert!(
        !ui.update(|window, _| window.has_image_atlas_entry(&old)),
        "a source revision must evict pixels no longer projected by its tree"
    );
}

/// A toolbar/dialog projection can borrow the same worker pixels without owning the preview's atlas lifetime.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_image_atlas_survives_another_projection_release(cx: &mut TestAppContext) {
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", "![image](img.png)")]);
    std::fs::write(fixture.directory.path().join("img.png"), png(160, 120)).unwrap();
    fixture.open("notes.md", ui);
    complete(&mut fixture, ui);
    let pixels = published_pixels(&fixture, ui);
    let document = (*fixture.manager.live["markdown"].views["preview"]).clone();
    let images = ui.update(|_, cx| {
        fixture
            .app
            .read(cx)
            .extensions
            .read(cx)
            .worker
            .state
            .lock()
            .unwrap()
            .images
            .clone()
    });
    // Exercise the same generic constructor used by source toolbars, with an approved tree and
    // real authorized pixels. The original live preview remains visible throughout this release.
    let projection = ui.update(|window, cx| {
        let projection = cx.new(|cx| {
            crate::ui::plugin::PluginView::new(
                "markdown".into(),
                document,
                protocol::Environment::default(),
                |_, _| {},
                window,
                cx,
            )
            .content_sized()
        });
        projection.update(cx, |view, cx| {
            view.update_images("markdown/preview", &images, window, cx)
        });
        projection
    });
    ui.update(|_, _| drop(projection));
    ui.run_until_parked();
    assert!(
        ui.update(|window, _| window.has_image_atlas_entry(&pixels)),
        "releasing a projection must preserve pixels still owned by the live preview"
    );

    fixture.manager.disable("markdown").unwrap();
    fixture.settle(ui);
    assert!(!ui.update(|window, _| window.has_image_atlas_entry(&pixels)));
}

/// Native pixels, image height and localized per-image failures leave the surrounding source intact.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_images_render_native_pixels_and_individual_errors(cx: &mut TestAppContext) {
    let text = "before **bold** ![中文图](img.png) after\n\n![缺图](missing.png)\n\n![坏图](broken.png)\n\n![越界](/outside.png)\n\nend";
    let (mut fixture, ui) =
        NativeMarkdown::mount(cx, &[("notes.md", text), ("broken.png", "not an image")]);
    std::fs::write(fixture.directory.path().join("img.png"), png(320, 720)).unwrap();
    fixture.open("notes.md", ui);
    let document = &fixture.manager.live["markdown"].views["preview"];
    let mut nodes = Vec::new();
    document.root.visit(&mut |node| {
        if let protocol::ui::Kind::Image { source, alt } = &node.kind {
            nodes.push((node.id.clone(), source.clone(), alt.clone()));
        }
    });
    assert_eq!(
        nodes.len(),
        4,
        "real guest must retain image declarations and prose"
    );
    complete(&mut fixture, ui);
    let loaded = nodes
        .iter()
        .find(|(_, source, _)| source == "img.png")
        .unwrap();
    // GPUI's test selector API takes static names; these few test-only IDs live for the test process.
    let selector: &'static str = Box::leak(format!("plugin-image-{}", loaded.0).into_boxed_str());
    let bounds = ui.debug_bounds(selector).expect("native pixel image");
    let preview = ui.debug_bounds("editor-preview-pane").unwrap();
    assert!(
        bounds.size.height >= px(700.),
        "image completion must change block height"
    );
    assert!(bounds.size.width <= preview.size.width);
    for (uri, key) in [
        ("missing.png", "preview.image_missing"),
        ("broken.png", "preview.image_failed"),
        ("/outside.png", "preview.image_path"),
    ] {
        let node = nodes.iter().find(|(_, source, _)| source == uri).unwrap();
        let status: &'static str =
            Box::leak(format!("plugin-image-status-{}-{key}", node.0).into_boxed_str());
        assert!(
            ui.debug_bounds(status).is_some(),
            "visible reason for {uri}"
        );
    }
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        text
    );
    assert_eq!(
        std::fs::read_to_string(fixture.directory.path().join("notes.md")).unwrap(),
        text
    );
    // Live locale/theme changes preserve the same decoded image and accessible caption layout.
    ui.update(|_, cx| apply_theme(builtin_theme(true), cx));
    fixture.settle(ui);
    assert!(ui.debug_bounds(selector).is_some());
    fixture.manager.disable("markdown").unwrap();
    fixture.settle(ui);
    assert!(ui.debug_bounds(selector).is_none());
    assert!(fixture.manager.image_resources().is_empty());
    assert!(ui.update(|_, cx| {
        fixture
            .app
            .read(cx)
            .extensions
            .read(cx)
            .worker
            .state
            .lock()
            .unwrap()
            .images
            .photos
            .is_empty()
    }));
}

/// A wide bitmap follows preview width while preserving its ratio before and after native divider dragging.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_wide_image_height_follows_the_native_preview_width(cx: &mut TestAppContext) {
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", "![宽图](wide.png)")]);
    std::fs::write(fixture.directory.path().join("wide.png"), png(2000, 1000)).unwrap();
    fixture.open("notes.md", ui);
    complete(&mut fixture, ui);
    let selector = "plugin-image-pixels-b-0-image";
    let before = ui.debug_bounds(selector).expect("wide native image");
    assert!((before.size.height * 2. - before.size.width).abs() < px(2.));
    let preview = ui.debug_bounds("editor-preview-pane").unwrap();
    assert!(before.size.width <= preview.size.width);
    let divider = ui.debug_bounds("editor-preview-divider").unwrap().center();
    let target = point(preview.right() - px(180.), divider.y);
    ui.simulate_mouse_down(divider, MouseButton::Left, Default::default());
    ui.run_until_parked();
    ui.simulate_mouse_move(
        point(divider.x + px(12.), divider.y),
        MouseButton::Left,
        Default::default(),
    );
    ui.run_until_parked();
    ui.simulate_mouse_move(target, MouseButton::Left, Default::default());
    ui.run_until_parked();
    ui.simulate_mouse_up(target, MouseButton::Left, Default::default());
    fixture.settle(ui);
    let after = ui.debug_bounds(selector).unwrap();
    assert!(
        after.size.width < before.size.width,
        "native image must respond to the narrower pane"
    );
    assert!((after.size.height * 2. - after.size.width).abs() < px(2.));
}

/// A held local HTTP response proves that image I/O neither blocks editing nor targets the next document.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_late_network_images_cannot_replace_a_new_document(cx: &mut TestAppContext) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (accepted_tx, accepted_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut request = [0; 2048];
        let _ = stream.read(&mut request);
        accepted_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let bytes = png(320, 720);
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        );
        let _ = stream.write_all(&bytes);
    });
    let text = format!("![慢图](http://{address}/slow.png)");
    let (mut fixture, ui) =
        NativeMarkdown::mount(cx, &[("notes.md", &text), ("other.md", "# New document")]);
    fixture.open("notes.md", ui);
    accepted_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("controlled image request");
    fixture.focus_editor(ui);
    let start = Instant::now();
    ui.simulate_keystrokes("ctrl-end");
    ui.simulate_input(" continued");
    ui.run_until_parked();
    fixture.open("other.md", ui);
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "held download blocked native input or navigation"
    );
    release_tx.send(()).unwrap();
    server.join().unwrap();
    for _ in 0..10 {
        fixture.settle(ui);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        "# New document"
    );
    assert!(fixture.manager.image_resources().is_empty());
    assert!(ui.update(|_, cx| {
        fixture
            .app
            .read(cx)
            .extensions
            .read(cx)
            .worker
            .state
            .lock()
            .unwrap()
            .images
            .photos
            .is_empty()
    }));
}
