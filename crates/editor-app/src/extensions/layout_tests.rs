//! Actual independent layout packages exercise native provider selection and editor-state continuity.
use super::*;
use gpui_kit::EntityInputHandler;
use gpui_kit::{TestAppContext, VisualTestContext, gpui};
use plugin_runtime::Manager;
use std::io::{Cursor, Write};

/// Repack one independent component under two identities through the ordinary package validator.
fn package(id: &str, name: &str) -> Package {
    let mut files = Package::read(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-layout-test/layout-example.zip"),
    )
    .expect("build-layout-example.ps1 first")
    .files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["id"] = id.into();
    manifest["name"] = name.into();
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&zip.finish().unwrap().into_inner()).unwrap()
}

/// Execute the existing native transport, then publish Manager-owned immutable views at its real boundary.
fn pump(app: &Entity<EditorApp>, manager: &mut Manager, cx: &mut App) {
    let owner = app.read(cx).extensions.clone();
    let work = owner
        .read(cx)
        .worker
        .recorded
        .lock()
        .unwrap()
        .try_iter()
        .collect::<Vec<_>>();
    for work in work {
        if let Work::Event(id, _, panel, event) = work {
            let result = manager.event(&id, panel, event);
            assert!(
                result.is_ok()
                    || result
                        .as_ref()
                        .err()
                        .and_then(|e| e.downcast_ref::<protocol::api::Failure>())
                        .is_some_and(|e| e.code == protocol::api::ErrorCode::StaleRevision),
                "{result:?}"
            );
        }
    }
    let views = manager
        .live
        .iter()
        .flat_map(|(id, live)| {
            live.views
                .iter()
                .map(move |(panel, view)| (format!("{id}/{panel}"), view.clone()))
        })
        .collect();
    let until = Instant::now() + Duration::from_secs(5);
    let resources = loop {
        let resources = manager.image_resources();
        if resources
            .values()
            .all(|image| !matches!(image.state, plugin_runtime::ImageState::Loading))
        {
            break resources;
        }
        assert!(Instant::now() < until, "file image did not finish");
        std::thread::sleep(Duration::from_millis(5));
    };
    let pixels = images::VectorRenderer::default().prepare_resources(&views, &resources);
    owner.update(cx, |owner, cx| {
        let mut state = owner.worker.state.lock().unwrap();
        state.entries = manager.installed.values().cloned().collect();
        state.views = views;
        state.images = pixels;
        drop(state);
        owner.poll(cx);
    });
    let panels = app
        .read(cx)
        .plugin_panels
        .values()
        .cloned()
        .collect::<Vec<_>>();
    for panel in panels {
        panel.update(cx, |panel, cx| panel.poll(cx));
    }
}

/// Flush two ordinary render/notification turns; resize publications settle without private test APIs.
fn draw(app: &Entity<EditorApp>, manager: &mut Manager, cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.run_until_parked();
        cx.update(|window, cx| {
            pump(app, manager, cx);
            window.draw(cx).clear(cx);
        });
    }
}

/// Select from the actual file Tab popup, capturing the same native context used in production.
fn choose(
    index: usize,
    app: &Entity<EditorApp>,
    manager: &mut Manager,
    cx: &mut VisualTestContext,
) {
    let active = cx.update(|_, cx| app.read(cx).active_tab_index().unwrap());
    let tab = cx
        .debug_bounds(["editor-tab-0", "editor-tab-1"][active])
        .unwrap()
        .center();
    cx.simulate_mouse_down(tab, MouseButton::Right, Modifiers::default());
    cx.simulate_mouse_up(tab, MouseButton::Right, Modifiers::default());
    // Drain tab activation's deferred focus before exercising the popup's keyboard owner.
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(cx.update(|_, cx| app.read(cx).file_view_menu.is_none()));
    cx.simulate_mouse_down(tab, MouseButton::Right, Modifiers::default());
    cx.simulate_mouse_up(tab, MouseButton::Right, Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let steps = cx.update(|_, cx| {
        let items = &app.read(cx).file_view_menu.as_ref().unwrap().read(cx).items;
        assert!(!items[index].disabled);
        items
            .iter()
            .take(index)
            .filter(|item| !item.disabled)
            .count()
    });
    // Navigation skips disabled candidates just as the ordinary native popup does.
    cx.simulate_keystrokes(&format!("home {}enter", "down ".repeat(steps)));
    draw(app, manager, cx);
}

/// Layout buttons and provider recovery preserve dirty text, selection and native Undo/Redo.
#[gpui::test]
#[ignore = "build actual independent fixture with scripts/build-layout-example.ps1 first"]
fn real_layout_packages_preserve_native_edits_and_explicit_provider_choice(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.layout");
    std::fs::write(&path, "original draft").unwrap();
    let mut manager = Manager::open(
        data.path().to_path_buf(),
        protocol::Environment {
            workspace: directory.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    for (id, name) in [
        ("layout-example", "First Layout"),
        ("other-layout", "Second Layout"),
    ] {
        let pkg = package(id, name);
        manager
            .install(&pkg, pkg.manifest.permissions.clone())
            .unwrap();
    }
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
        app.update(cx, |app, cx| app.open_file(path.clone(), window, cx));
    });
    draw(&app, &mut manager, visual);
    // Two available packages remain ambiguous until an explicit choice, independent of package ID order.
    assert!(visual.debug_bounds("editor-plugin-layout").is_none());
    choose(1, &app, &mut manager, visual);
    let editor = visual
        .debug_bounds("editor-source-pane")
        .expect("borrowed native editor");
    let content = visual.debug_bounds("plugin-ui-plugin-content").unwrap();
    assert!(editor.right() <= content.left() + px(2.));
    visual.simulate_click(editor.center(), Modifiers::default());
    visual.simulate_input("未保存");
    draw(&app, &mut manager, visual);
    let (draft, selection) = visual.update(|_, cx| {
        let editor = app.read(cx).editor.read(cx);
        (editor.text().to_string(), editor.selected_range())
    });
    assert_ne!(draft, "original draft");
    let column = visual
        .debug_bounds("plugin-ui-layout-column")
        .unwrap()
        .center();
    visual.simulate_click(column, Modifiers::default());
    draw(&app, &mut manager, visual);
    let editor = visual.debug_bounds("editor-source-pane").unwrap();
    let content = visual.debug_bounds("plugin-ui-plugin-content").unwrap();
    assert!(editor.bottom() <= content.top() + px(2.));
    assert!(editor.size.height > px(80.) && content.size.height > px(80.));
    let hide = visual
        .debug_bounds("plugin-ui-layout-content")
        .unwrap()
        .center();
    visual.simulate_click(hide, Modifiers::default());
    draw(&app, &mut manager, visual);
    assert!(visual.debug_bounds("editor-source-pane").is_none());
    visual.simulate_input("must not leak");
    assert_eq!(
        visual.update(|_, cx| app.read(cx).editor.read(cx).text().to_string()),
        draft
    );
    let old_source = manager.live["layout-example"].views["layout"]
        .source
        .clone()
        .unwrap();
    choose(2, &app, &mut manager, visual);
    let stale = manager
        .event(
            "other-layout",
            Some("layout".into()),
            PluginEvent::Preview {
                document: Some(old_source),
                text: draft.clone(),
            },
        )
        .unwrap_err();
    assert_eq!(
        stale.downcast_ref::<protocol::api::Failure>().unwrap().code,
        protocol::api::ErrorCode::StaleRevision
    );
    assert!(manager.live.contains_key("other-layout"));
    assert_eq!(
        visual.update(|_, cx| app.read(cx).editor.read(cx).selected_range()),
        selection
    );
    assert_eq!(
        visual.update(|_, cx| app.read(cx).editor.read(cx).text().to_string()),
        draft
    );
    // A subsequent installation cannot override the chosen package.
    let third = package("third-layout", "Third Layout");
    manager
        .install(&third, third.manifest.permissions.clone())
        .unwrap();
    draw(&app, &mut manager, visual);
    assert_eq!(
        visual.update(|_, cx| app
            .read(cx)
            .active_editor_preview(cx)
            .unwrap()
            .read(cx)
            .active
            .clone()),
        Some("other-layout".into())
    );
    manager.disable("other-layout").unwrap();
    draw(&app, &mut manager, visual);
    assert!(visual.debug_bounds("editor-source-pane").is_some());
    assert!(visual.debug_bounds("editor-plugin-layout").is_none());
    choose(0, &app, &mut manager, visual);
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
    let saved = crate::app::session::SessionState::load(&directory.path().canonicalize().unwrap());
    assert!(matches!(
        saved.file_view_providers.get("layout"),
        Some(crate::app::session::FileProviderChoice::Native)
    ));
    let other = tempfile::tempdir().unwrap();
    assert!(
        crate::app::session::SessionState::load(other.path())
            .file_view_providers
            .is_empty()
    );
    ime_composition_survives_layouts_without_hidden_input(&app, &mut manager, visual);
    binary_views_can_switch_providers(directory.path(), &app, &mut manager, visual);
}

/// The native preedit stays in its original editor while an omitted surface accepts no committed input.
fn ime_composition_survives_layouts_without_hidden_input(
    app: &Entity<EditorApp>,
    manager: &mut Manager,
    visual: &mut VisualTestContext,
) {
    choose(1, app, manager, visual);
    let row = visual
        .debug_bounds("plugin-ui-layout-row")
        .unwrap()
        .center();
    visual.simulate_click(row, Modifiers::default());
    draw(app, manager, visual);
    visual.update(|window, cx| {
        let editor = app.read(cx).editor.clone();
        editor.update(cx, |editor, cx| {
            editor.focus_handle(cx).focus(window, cx);
            editor.replace_and_mark_text_in_range(None, "拼", Some(0..1), window, cx);
        });
    });
    draw(app, manager, visual);
    let composing = visual.update(|_, cx| app.read(cx).editor.read(cx).text().to_string());
    let column = visual
        .debug_bounds("plugin-ui-layout-column")
        .unwrap()
        .center();
    visual.simulate_click(column, Modifiers::default());
    draw(app, manager, visual);
    assert!(
        visual
            .update(|window, cx| app
                .read(cx)
                .editor
                .clone()
                .update(cx, |editor, cx| editor.marked_text_range(window, cx)))
            .is_some()
    );
    let hide = visual
        .debug_bounds("plugin-ui-layout-content")
        .unwrap()
        .center();
    visual.simulate_click(hide, Modifiers::default());
    draw(app, manager, visual);
    assert!(visual.debug_bounds("editor-source-pane").is_none());
    visual.simulate_input("误入");
    assert_eq!(
        visual.update(|_, cx| app.read(cx).editor.read(cx).text().to_string()),
        composing
    );
    choose(0, app, manager, visual);
    assert!(visual.debug_bounds("editor-source-pane").is_some());
    assert_eq!(
        visual.update(|_, cx| app.read(cx).editor.read(cx).text().to_string()),
        composing
    );
    visual.update(|window, cx| {
        app.read(cx)
            .editor
            .clone()
            .update(cx, |editor, cx| editor.unmark_text(window, cx))
    });
}

/// Binary candidates retain their Tab and offer another viewer, without a text recovery entry.
fn binary_views_can_switch_providers(
    directory: &Path,
    app: &Entity<EditorApp>,
    manager: &mut Manager,
    visual: &mut VisualTestContext,
) {
    let png = directory.join("photo.png");
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        100,
        50,
        image::Rgb([30, 80, 150]),
    ))
    .write_to(&mut bytes, image::ImageFormat::Png)
    .unwrap();
    std::fs::write(&png, bytes.into_inner()).unwrap();
    visual.update(|window, cx| app.update(cx, |app, cx| app.open_file(png, window, cx)));
    draw(app, manager, visual);
    assert!(visual.debug_bounds("file-view-unavailable").is_some());
    choose(0, app, manager, visual);
    assert!(visual.debug_bounds("plugin-file-image").is_some());
    assert!(visual.debug_bounds("editor-source-pane").is_none());
    manager.disable("layout-example").unwrap();
    draw(app, manager, visual);
    assert!(visual.debug_bounds("file-view-unavailable").is_some());
    let tab = visual.debug_bounds("editor-tab-1").unwrap().center();
    visual.simulate_mouse_down(tab, MouseButton::Right, Modifiers::default());
    visual.simulate_mouse_up(tab, MouseButton::Right, Modifiers::default());
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let menu = visual.update(|_, cx| {
        app.read(cx)
            .file_view_menu
            .as_ref()
            .unwrap()
            .read(cx)
            .items
            .clone()
    });
    assert!(
        menu.iter()
            .all(|item| item.label != t!("file_view.restore_text").to_string())
    );
    let third = visual
        .debug_bounds("native-menu-provider-2")
        .unwrap()
        .center();
    visual.simulate_click(third, Modifiers::default());
    draw(app, manager, visual);
    assert!(visual.debug_bounds("plugin-file-image").is_some());
    assert!(visual.debug_bounds("editor-source-pane").is_none());
}
