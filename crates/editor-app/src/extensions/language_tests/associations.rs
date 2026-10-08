//! User file associations exercise installed language packages and the actual open text session.
use super::*;

/// Changing a user association updates an already-open document without authorizing a plugin.
#[gpui::test]
fn custom_extension_association_updates_open_document(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sample.CFG");
    std::fs::write(&path, "answer = 42\n").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let root = workspace.root().join(".runtime-plugin-test");
    let mut manager = plugin_runtime::Manager::open(
        root.clone(),
        protocol::Environment {
            workspace: workspace.root().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    visual.update(|window, cx| app.update(cx, |app, cx| app.open_file(path.clone(), window, cx)));
    manager
        .install(&language_package("novel-associated"), Default::default())
        .unwrap();
    publish_languages(&app, &manager, visual);
    let language = |visual: &mut gpui_kit::VisualTestContext| {
        visual.update(|_, cx| {
            app.read(cx)
                .tabs
                .last()
                .unwrap()
                .text
                .as_ref()
                .unwrap()
                .editor
                .read(cx)
                .language_name()
                .to_string()
        })
    };
    assert_eq!(language(visual), "text");
    // Exercise the real settings input instead of injecting the persisted association.
    let editor_window = visual.update(|window, _| window.window_handle());
    let trigger = visual.debug_bounds("settings-trigger").unwrap();
    visual.simulate_click(trigger.center(), Default::default());
    let dialog = visual
        .update(|_, cx| cx.windows())
        .into_iter()
        .find(|handle| *handle != editor_window)
        .unwrap();
    let form = gpui_kit::VisualTestContext::from_window(dialog, visual).into_mut();
    form.run_until_parked();
    let nav = form.debug_bounds("settings-nav-languages").unwrap();
    form.simulate_click(nav.center(), Default::default());
    form.run_until_parked();
    let add = form.debug_bounds("file-association-add").unwrap();
    form.simulate_click(add.center(), Default::default());
    form.run_until_parked();
    let field = form.debug_bounds("file-association-extension").unwrap();
    form.simulate_click(field.center(), Default::default());
    form.simulate_input(".CFG");
    form.run_until_parked();
    let choice = form
        .debug_bounds("file-association-language-novel")
        .unwrap();
    form.simulate_click(choice.center(), Default::default());
    form.run_until_parked();
    visual.update(|_, cx| app.update(cx, |app, cx| app.sync_dynamic_languages(cx)));
    visual.run_until_parked();
    assert_eq!(language(visual), "novel");
    // Invalid paths cannot become a persisted selector or replace the working association.
    assert!(crate::language::providers::associate_extension("../cfg", Some("novel")).is_err());
    manager.disable("novel-associated").unwrap();
    publish_languages(&app, &manager, visual);
    assert_eq!(language(visual), "text");
    manager.enable("novel-associated").unwrap();
    publish_languages(&app, &manager, visual);
    assert_eq!(language(visual), "novel");
    crate::language::providers::associate_extension("cfg", None).unwrap();
    visual.update(|_, cx| app.update(cx, |app, cx| app.sync_dynamic_languages(cx)));
    visual.run_until_parked();
    assert_eq!(language(visual), "text");
}
