//! Actual independent layout packages exercise native provider selection and editor-state continuity.
use super::package_ui_test_support::*;
use super::*;
use gpui_kit::EntityInputHandler;
use gpui_kit::{TestAppContext, VisualTestContext, gpui};
use plugin_runtime::Manager;
use std::io::Cursor;

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
    image_views_withdraw_files_before_svg(directory.path(), &app, &mut manager, visual);
}

/// Named providers are found through the real menu, then activated through its native keyboard path.
fn choose_named(
    name: &str,
    app: &Entity<EditorApp>,
    manager: &mut Manager,
    visual: &mut VisualTestContext,
) {
    let active = visual.update(|_, cx| app.read(cx).active_tab_index().unwrap());
    let tab = visual
        .debug_bounds(["editor-tab-0", "editor-tab-1", "editor-tab-2"][active])
        .unwrap()
        .center();
    visual.simulate_mouse_down(tab, MouseButton::Right, Modifiers::default());
    visual.simulate_mouse_up(tab, MouseButton::Right, Modifiers::default());
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let index = visual.update(|_, cx| {
        app.read(cx)
            .file_view_menu
            .as_ref()
            .unwrap()
            .read(cx)
            .items
            .iter()
            .position(|item| item.label.starts_with(name))
            .unwrap()
    });
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    choose(index, app, manager, visual);
}

/// Two still-enabled Image packages relinquish PNG ownership before one returns to editable SVG.
fn image_views_withdraw_files_before_svg(
    directory: &Path,
    app: &Entity<EditorApp>,
    manager: &mut Manager,
    visual: &mut VisualTestContext,
) {
    let image_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/svg.zip");
    for (id, name) in [("image-a", "A Image"), ("image-b", "B Image")] {
        let package = identify(Package::read(&image_path).unwrap(), id, name);
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
    }
    draw(app, manager, visual);
    choose_named("A Image", app, manager, visual);
    assert!(visual.debug_bounds("plugin-file-image").is_some());
    choose_named("B Image", app, manager, visual);
    assert!(visual.debug_bounds("plugin-file-image").is_some());
    assert!(
        !manager
            .image_resources()
            .keys()
            .any(|key| key.starts_with("image-a/"))
    );
    assert!(manager.live["image-a"].views["preview"].file.is_none());
    let svg = directory.join("drawing.svg");
    std::fs::write(&svg, "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"40\" height=\"30\"><rect width=\"40\" height=\"30\" fill=\"red\"/></svg>").unwrap();
    visual.update(|window, cx| app.update(cx, |app, cx| app.open_file(svg, window, cx)));
    draw(app, manager, visual);
    choose_named("A Image", app, manager, visual);
    assert!(manager.installed["image-a"].error.is_none());
    assert!(manager.live["image-a"].views["preview"].source.is_some());
    assert!(manager.live["image-a"].views["preview"].file.is_none());
    assert!(visual.debug_bounds("editor-source-pane").is_some());
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
