//! Real clipboard and external file-drop input must create sibling images and one undoable source edit.
use super::*;
use gpui_kit::{ClipboardItem, ExternalPaths, FileDropEvent, Image, ImageFormat, PlatformInput};
use harness::NativeMarkdown;
mod safety;

/// Encoded fixture data lets tests distinguish the real format from a misleading source file suffix.
fn png() -> Vec<u8> {
    let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        32,
        24,
        image::Rgba([30, 170, 200, 255]),
    ));
    let mut encoded = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    encoded.into_inner()
}

/// Two native Paste actions before foreground preparation finishes must give an explicit retry outcome.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_image_busy_input_preserves_the_first_gesture(cx: &mut TestAppContext) {
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", "source")]);
    fixture.open("notes.md", ui);
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-end");
    let image = Image::from_bytes(ImageFormat::Png, png());
    let external = tempfile::tempdir().unwrap();
    let file = external.path().join("busy.png");
    std::fs::write(&file, png()).unwrap();
    let app = fixture.app.clone();
    let position = ui.update(|_, cx| {
        app.read(cx)
            .editor
            .read(cx)
            .range_to_bounds(&(0..0))
            .unwrap()
            .center()
    });
    ui.update(|window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_image(&image));
        window.dispatch_action(Box::new(gpui_base::input::Paste), cx);
        window.dispatch_action(Box::new(gpui_base::input::Paste), cx);
        let handle = window.window_handle();
        // Dispatch after those native actions but before the foreground preparation task can finish.
        cx.defer(move |cx| {
            handle
                .update(cx, |_, window, cx| {
                    assert!(app.read(cx).image_input_preparing);
                    window.dispatch_event(
                        PlatformInput::FileDrop(FileDropEvent::Entered {
                            position,
                            paths: ExternalPaths([file].into_iter().collect()),
                        }),
                        cx,
                    );
                    window.draw(cx).clear(cx);
                    window.dispatch_event(
                        PlatformInput::FileDrop(FileDropEvent::Submit { position }),
                        cx,
                    );
                    assert_eq!(
                        app.read(cx).editor.read(cx).selected_range(),
                        6..6,
                        "a busy drop must keep the first paste's captured caret"
                    );
                })
                .unwrap()
        });
    });
    ui.run_until_parked();
    let status = ui.update(|_, cx| fixture.app.read(cx).status.clone());
    assert!(
        status.contains("重试") || status.contains("retry"),
        "busy native input needs a visible retry outcome: {status}"
    );
    fixture.settle(ui);
    assert!(
        fixture.directory.path().join("img.png").exists(),
        "the first gesture must remain valid"
    );
    assert!(
        !fixture.directory.path().join("img1.png").exists(),
        "the rejected second gesture creates no file"
    );
    let text = ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string());
    assert!(text.starts_with("source!") && text.contains("](img.png)"));
}

/// Native Paste consumes image bytes, keeps existing files, and Undo only removes the inserted reference.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_image_paste_creates_sibling_without_overwrite_and_undo_keeps_file(
    cx: &mut TestAppContext,
) {
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", "before\n")]);
    let existing = fixture.directory.path().join("img.png");
    std::fs::write(&existing, b"existing user file").unwrap();
    fixture.open("notes.md", ui);
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-end");
    let bytes = png();
    let image = Image::from_bytes(ImageFormat::Png, bytes.clone());
    ui.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_image(&image)));
    ui.simulate_keystrokes("ctrl-v");
    ui.run_until_parked();
    fixture.settle(ui);
    let created = fixture.directory.path().join("img1.png");
    assert!(
        created.exists(),
        "the native paste must create the next sibling image"
    );
    assert_eq!(std::fs::read(&created).unwrap(), bytes);
    assert_eq!(std::fs::read(&existing).unwrap(), b"existing user file");
    let text = ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string());
    assert!(
        text.contains("](img1.png)"),
        "a saved image needs its relative source reference: {text}"
    );
    let receipt = fixture.manager.live["markdown"].views["preview"]
        .active_node("format-error")
        .expect("separate save/edit receipt");
    assert!(
        matches!(&receipt.kind, protocol::ui::Kind::Text { text }
        if text.contains("图片已保存并插入引用") || text.contains("Saved images and inserted references")),
        "a completed native source edit must be confirmed rather than reported as pending"
    );
    assert!(!fixture.directory.path().join("assets").exists());
    // A later independent paste searches actual filenames again and receives its own undo step.
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-v");
    ui.run_until_parked();
    fixture.settle(ui);
    let second = fixture.directory.path().join("img2.png");
    assert_eq!(std::fs::read(&second).unwrap(), bytes);
    ui.simulate_keystrokes("ctrl-z");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        text
    );
    ui.simulate_keystrokes("ctrl-z");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        "before\n"
    );
    assert!(
        created.exists(),
        "text undo must preserve the complete saved image"
    );
    assert!(second.exists());
}

/// A real platform drop imports only offered files, chooses actual encoding suffixes and preserves order.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_image_drop_imports_multiple_formats_and_previews_saved_files(
    cx: &mut TestAppContext,
) {
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", "中文\n")]);
    let external = tempfile::tempdir().unwrap();
    let misleading = external.path().join("first.jpeg");
    std::fs::write(&misleading, png()).unwrap();
    let jpeg_path = external.path().join("second.png");
    let jpeg = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        40,
        30,
        image::Rgb([90, 80, 70]),
    ));
    let mut encoded = std::io::Cursor::new(Vec::new());
    jpeg.write_to(&mut encoded, image::ImageFormat::Jpeg)
        .unwrap();
    let jpeg = encoded.into_inner();
    std::fs::write(&jpeg_path, &jpeg).unwrap();
    fixture.open("notes.md", ui);
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-a");
    let toolbar = ui.debug_bounds("editor-source-toolbar").unwrap().center();
    ui.update(|window, cx| {
        window.dispatch_event(
            PlatformInput::FileDrop(FileDropEvent::Entered {
                position: toolbar,
                paths: ExternalPaths([misleading.clone()].into_iter().collect()),
            }),
            cx,
        )
    });
    ui.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    ui.update(|window, cx| {
        window.dispatch_event(
            PlatformInput::FileDrop(FileDropEvent::Submit { position: toolbar }),
            cx,
        )
    });
    ui.run_until_parked();
    fixture.settle(ui);
    assert!(
        !fixture.directory.path().join("img.png").exists(),
        "dropping on a toolbar must not consume the old selected source"
    );
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        "中文\n"
    );
    let position = ui.update(|_, cx| {
        fixture
            .app
            .read(cx)
            .editor
            .read(cx)
            .range_to_bounds(&(0..0))
            .unwrap()
            .center()
    });
    assert!(
        ui.update(|window, cx| crate::editor::caret_offset_at(
            fixture.app.read(cx).editor.read(cx),
            position,
            window,
            cx
        ))
        .is_some(),
        "the platform fixture must drop inside real source geometry"
    );
    ui.update(|window, cx| {
        window.dispatch_event(
            PlatformInput::FileDrop(FileDropEvent::Entered {
                position,
                paths: ExternalPaths([misleading, jpeg_path].into_iter().collect()),
            }),
            cx,
        )
    });
    assert!(
        ui.update(|_, cx| fixture
            .app
            .read(cx)
            .image_drag
            .as_ref()
            .is_some_and(|(paths, _)| paths.paths().len() == 2)),
        "the platform move must supply its actual typed external payload"
    );
    ui.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    ui.update(|window, cx| {
        window.dispatch_event(
            PlatformInput::FileDrop(FileDropEvent::Submit { position }),
            cx,
        )
    });
    ui.run_until_parked();
    // Two file effects and the final text edit progress asynchronously through the real worker queue.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !fixture.directory.path().join("img1.jpg").exists() {
        fixture.settle(ui);
        assert!(
            std::time::Instant::now() < deadline,
            "the drop did not save its offered files: {}",
            ui.update(|_, cx| fixture.app.read(cx).status.clone())
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    fixture.settle(ui);
    assert_eq!(
        std::fs::read(fixture.directory.path().join("img.png")).unwrap(),
        png()
    );
    assert_eq!(
        std::fs::read(fixture.directory.path().join("img1.jpg")).unwrap(),
        jpeg
    );
    let text = ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string());
    let first = text.find("](img.png)").expect("first actual encoding");
    let second = text.find("](img1.jpg)").expect("second actual encoding");
    assert!(first < second, "the native drop preserves image order");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        fixture.settle(ui);
        let resources = fixture.manager.image_resources();
        if resources.len() == 2
            && resources
                .values()
                .all(|resource| matches!(resource.state, plugin_runtime::ImageState::Ready(_)))
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the imported files must enter the live image preview"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let mut visible = 0;
    fixture.manager.live["markdown"].views["preview"]
        .root
        .visit(&mut |node| {
            if matches!(node.kind, protocol::ui::Kind::Image { .. }) {
                let selector: &'static str =
                    Box::leak(format!("plugin-image-pixels-{}", node.id).into_boxed_str());
                assert!(
                    ui.debug_bounds(selector).is_some(),
                    "the saved image needs native pixels"
                );
                visible += 1;
            }
        });
    assert_eq!(visible, 2);
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-z");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        "中文\n"
    );
    assert!(fixture.directory.path().join("img.png").exists());
    assert!(fixture.directory.path().join("img1.jpg").exists());
}

/// Cancelling the native save-first prompt leaves neither an attachment nor a dangling reference.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_image_paste_cancel_save_first_has_no_side_effect(cx: &mut TestAppContext) {
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", "keep source")]);
    fixture.open("notes.md", ui);
    rust_i18n::set_locale("en");
    std::fs::remove_file(fixture.directory.path().join("notes.md")).unwrap();
    fixture.focus_editor(ui);
    let image = Image::from_bytes(ImageFormat::Png, png());
    ui.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_image(&image)));
    ui.simulate_keystrokes("ctrl-v");
    ui.run_until_parked();
    assert!(
        ui.has_pending_prompt(),
        "a source without a saved file needs a save-first prompt"
    );
    ui.simulate_prompt_answer("Cancel");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        "keep source"
    );
    assert!(!fixture.directory.path().join("img.png").exists());
    assert!(!fixture.directory.path().join("notes.md").exists());
    // The next explicit Save answer restores this same source through the create-only workspace store.
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-v");
    ui.run_until_parked();
    assert!(ui.has_pending_prompt());
    ui.simulate_prompt_answer("Save");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        std::fs::read_to_string(fixture.directory.path().join("notes.md")).unwrap(),
        "keep source"
    );
    assert!(fixture.directory.path().join("img.png").exists());
    assert!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string())
            .contains("](img.png)")
    );
    rust_i18n::set_locale("zh-CN");
}
