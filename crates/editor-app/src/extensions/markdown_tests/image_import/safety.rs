//! Real host queue regressions keep complete files while refusing stale or unauthorized source edits.
use super::*;
use gpui_kit::EntityInputHandler as _;
use protocol::api::{EditorOperation, EditorValue, RequestUpdate};

/// Native host menus and popups block external drops instead of editing the source beneath their overlay.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_image_drop_respects_host_popup_and_explorer_menu(cx: &mut TestAppContext) {
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", "source")]);
    fixture.open("notes.md", ui);
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-end");
    let external = tempfile::tempdir().unwrap();
    let file = external.path().join("image.png");
    std::fs::write(&file, png()).unwrap();
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
    for popup in [false, true] {
        if popup {
            // The fixture opens the existing status card even when there are no live plugin failures.
            ui.update(|_, cx| {
                fixture.app.update(cx, |app, cx| {
                    app.plugin_popup =
                        Some((crate::app::plugins::PluginPopupKind::Error, position));
                    cx.notify();
                })
            });
        } else {
            let explorer = ui.debug_bounds("explorer-root").unwrap().center();
            ui.simulate_mouse_down(explorer, MouseButton::Right, Default::default());
            ui.simulate_mouse_up(explorer, MouseButton::Right, Default::default());
        }
        ui.run_until_parked();
        fixture.settle(ui);
        assert!(
            ui.debug_bounds(if popup {
                "plugin-popup-blocker"
            } else {
                "explorer-context-menu"
            })
            .is_some()
        );
        ui.update(|window, cx| {
            window.dispatch_event(
                PlatformInput::FileDrop(FileDropEvent::Entered {
                    position,
                    paths: ExternalPaths([file.clone()].into_iter().collect()),
                }),
                cx,
            );
            window.draw(cx).clear(cx);
            window.dispatch_event(
                PlatformInput::FileDrop(FileDropEvent::Submit { position }),
                cx,
            );
        });
        for _ in 0..10 {
            ui.run_until_parked();
            fixture.settle(ui);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            !fixture.directory.path().join("img.png").exists(),
            "a host blocker must reject the captured drop"
        );
        assert_eq!(
            ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
            "source"
        );
        assert_eq!(
            ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).selected_range()),
            6..6
        );
        ui.simulate_click(position, Default::default());
        ui.run_until_parked();
        fixture.settle(ui);
    }
}

/// Stop at the real guest's accepted save request, before the host queue performs any file effect.
fn pending_save(
    fixture: &mut NativeMarkdown,
    ui: &mut gpui_kit::VisualTestContext,
) -> plugin_runtime::EditorRequest {
    let image = Image::from_bytes(ImageFormat::Png, png());
    ui.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_image(&image)));
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-v");
    ui.run_until_parked();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        super::super::composable_tests::pump(&mut fixture.manager, &fixture.app, ui);
        fixture.manager.poll();
        let requests = fixture
            .manager
            .live
            .get_mut("markdown")
            .unwrap()
            .take_editor_requests();
        if let Some(save) = requests
            .into_iter()
            .find(|request| matches!(request.operation(), EditorOperation::SaveImageInput { .. }))
        {
            return save;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "native paste did not reach its public save request"
        );
        ui.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// Use the production deferred request queue but hold the manager's completion poll for a later source change.
fn perform_save(
    fixture: &mut NativeMarkdown,
    save: &plugin_runtime::EditorRequest,
    ui: &mut gpui_kit::VisualTestContext,
) {
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
            .editor_requests
            .push(("markdown".into(), save.clone()))
    });
    super::super::composable_tests::publish(
        &mut fixture.manager,
        &mut fixture.renderer,
        &fixture.app,
        ui,
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !matches!(save.status(), RequestUpdate::Completed { .. }) {
        ui.run_until_parked();
        assert!(
            std::time::Instant::now() < deadline,
            "native save request did not complete: {:?}",
            save.status()
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// A completed file remains when its source changes or closes before the guest receives the save receipt.
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_image_saved_receipts_survive_revision_and_close_without_reference(
    cx: &mut TestAppContext,
) {
    let (mut fixture, ui) =
        NativeMarkdown::mount(cx, &[("first.md", "first"), ("second.md", "second")]);
    fixture.open("first.md", ui);
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-end");
    let save = pending_save(&mut fixture, ui);
    perform_save(&mut fixture, &save, ui);
    assert!(matches!(
        save.status(),
        RequestUpdate::Completed {
            result: Ok(EditorValue::ImageSaved { .. })
        }
    ));
    // The native file operation has finished, but no manager poll has delivered its receipt yet.
    ui.simulate_input(" changed");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        "first changed"
    );
    assert_eq!(
        std::fs::read(fixture.directory.path().join("img.png")).unwrap(),
        png()
    );
    let receipt = fixture.manager.live["markdown"].views["preview"]
        .active_node("format-error")
        .unwrap();
    assert!(matches!(&receipt.kind, protocol::ui::Kind::Text { text }
        if text.contains("img.png") && (text.contains("引用未插入") || text.contains("References were not inserted"))));

    fixture.open("second.md", ui);
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-end");
    let mut second = pending_save(&mut fixture, ui);
    loop {
        perform_save(&mut fixture, &second, ui);
        if !matches!(second.status(), RequestUpdate::Completed { result: Err(ref error) }
            if error.code == protocol::api::ErrorCode::Conflict)
        {
            break;
        }
        // Collision retries are still real guest operations; no test manufactures a saved receipt.
        fixture.manager.poll();
        second = fixture
            .manager
            .live
            .get_mut("markdown")
            .unwrap()
            .take_editor_requests()
            .into_iter()
            .find(|request| matches!(request.operation(), EditorOperation::SaveImageInput { .. }))
            .unwrap();
    }
    assert!(matches!(
        second.status(),
        RequestUpdate::Completed {
            result: Ok(EditorValue::ImageSaved { .. })
        }
    ));
    let path = fixture
        .directory
        .path()
        .join("second.md")
        .canonicalize()
        .unwrap();
    ui.update(|window, cx| {
        fixture
            .app
            .update(cx, |app, cx| app.close_tab(path, window, cx))
    });
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        "first changed"
    );
    assert!(fixture.directory.path().join("img1.png").exists());
    assert_eq!(
        std::fs::read_to_string(fixture.directory.path().join("second.md")).unwrap(),
        "second"
    );
    fixture.open("second.md", ui);
    let receipt = fixture.manager.live["markdown"].views["preview"]
        .active_node("format-error")
        .unwrap();
    assert!(matches!(&receipt.kind, protocol::ui::Kind::Text { text }
        if text.contains("img1.png") && (text.contains("引用未插入") || text.contains("References were not inserted"))));
}

/// Installed grants, late selection changes, native write errors and preedit all refuse dangling references.
#[cfg(windows)]
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_image_import_rejects_grants_selection_composition_and_write_failures(
    cx: &mut TestAppContext,
) {
    use std::os::windows::fs::OpenOptionsExt;
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", "source")]);
    fixture.open("notes.md", ui);
    fixture
        .manager
        .installed
        .get_mut("markdown")
        .unwrap()
        .grants
        .remove("workspace.write");
    fixture.settle(ui);
    fixture.focus_editor(ui);
    let image = Image::from_bytes(ImageFormat::Png, png());
    ui.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_image(&image)));
    ui.simulate_keystrokes("ctrl-v");
    ui.run_until_parked();
    fixture.settle(ui);
    assert!(!fixture.directory.path().join("img.png").exists());
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        "source"
    );
    fixture
        .manager
        .installed
        .get_mut("markdown")
        .unwrap()
        .grants
        .insert("workspace.write".into());
    fixture.settle(ui);

    ui.simulate_keystrokes("ctrl-end");
    let moved = pending_save(&mut fixture, ui);
    ui.simulate_keystrokes("ctrl-home");
    perform_save(&mut fixture, &moved, ui);
    assert!(
        matches!(moved.status(), RequestUpdate::Completed { result: Err(ref error) }
        if error.code == protocol::api::ErrorCode::StaleRevision)
    );
    fixture.settle(ui);
    assert!(!fixture.directory.path().join("img.png").exists());

    let failed = pending_save(&mut fixture, ui);
    // A real Windows sharing denial rejects file preparation, rather than a synthesized completion.
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(fixture.directory.path().join("notes.md"))
        .unwrap();
    perform_save(&mut fixture, &failed, ui);
    assert!(matches!(
        failed.status(),
        RequestUpdate::Completed { result: Err(_) }
    ));
    drop(lock);
    fixture.settle(ui);
    assert!(!fixture.directory.path().join("img.png").exists());
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        "source"
    );

    ui.update(|window, cx| {
        fixture
            .app
            .read(cx)
            .editor
            .clone()
            .update(cx, |editor, cx| {
                editor.set_selected_range(0..0, cx);
                editor.replace_and_mark_text_in_range(None, "拼", Some(0..1), window, cx);
            })
    });
    ui.run_until_parked();
    fixture.settle(ui);
    let composing = ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string());
    fixture.focus_editor(ui);
    ui.simulate_keystrokes("ctrl-v");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        composing
    );
    assert!(
        ui.update(|window, cx| fixture
            .app
            .read(cx)
            .editor
            .clone()
            .update(cx, |editor, cx| editor.marked_text_range(window, cx)))
            .is_some()
    );
    assert!(!fixture.directory.path().join("img.png").exists());
    ui.update(|window, cx| {
        fixture
            .app
            .read(cx)
            .editor
            .clone()
            .update(cx, |editor, cx| editor.unmark_text(window, cx))
    });
}

/// Save-first must reject a directory replaced with a junction before creating the missing source or its images.
#[cfg(windows)]
#[gpui::test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_markdown_image_save_first_rejects_redirected_parent(cx: &mut TestAppContext) {
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[]);
    let parent = fixture.directory.path().join("sub");
    std::fs::create_dir(&parent).unwrap();
    std::fs::write(parent.join("notes.md"), "keep source").unwrap();
    fixture.open("sub/notes.md", ui);
    rust_i18n::set_locale("en");
    std::fs::remove_file(parent.join("notes.md")).unwrap();
    std::fs::remove_dir(&parent).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let status = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:ME_EDITOR_TEST_IMAGE_JUNCTION -Value $env:ME_EDITOR_TEST_IMAGE_TARGET | Out-Null"])
        .env("ME_EDITOR_TEST_IMAGE_JUNCTION", &parent)
        .env("ME_EDITOR_TEST_IMAGE_TARGET", outside.path())
        .status().unwrap();
    assert!(status.success());
    fixture.focus_editor(ui);
    let image = Image::from_bytes(ImageFormat::Png, png());
    ui.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_image(&image)));
    ui.simulate_keystrokes("ctrl-v");
    ui.run_until_parked();
    assert!(ui.has_pending_prompt());
    ui.simulate_prompt_answer("Save");
    ui.run_until_parked();
    fixture.settle(ui);
    assert!(
        !outside.path().join("notes.md").exists(),
        "save-first must not restore a document through the redirected parent"
    );
    assert!(!outside.path().join("img.png").exists());
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        "keep source"
    );
    // Remove only the known temporary junction; its external target remains owned by outside's TempDir.
    std::fs::remove_dir(&parent).unwrap();
    rust_i18n::set_locale("zh-CN");
}
