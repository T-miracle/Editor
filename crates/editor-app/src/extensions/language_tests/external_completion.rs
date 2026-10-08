//! Workspace-scoped guest snapshots must not disable native completion in opened dependency files.
use super::*;
use std::time::{Duration, Instant};

/// Install an unfamiliar provider and edit an external file through the normal native key path.
#[gpui::test]
#[ignore = "build current capability-example and native lsp_fixture first"]
fn native_completion_remains_available_outside_guest_workspace(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let workspace_path = directory.path().join("workspace");
    std::fs::create_dir(&workspace_path).unwrap();
    let external = directory.path().join("dependency.novel");
    std::fs::write(&external, "").unwrap();
    let workspace = Workspace::open(&workspace_path).unwrap();
    let mut manager = plugin_runtime::Manager::open(
        workspace.root().join(".runtime-plugin-test"),
        protocol::Environment {
            workspace: workspace.root().display().to_string(),
            os: std::env::consts::OS.into(),
            ..Default::default()
        },
    )
    .unwrap();
    let recognition = language_package("novel-recognition");
    manager.install(&recognition, Default::default()).unwrap();
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    let exe = target
        .join(format!(
            "debug/examples/lsp_fixture{}",
            std::env::consts::EXE_SUFFIX
        ))
        .canonicalize()
        .unwrap();
    let package = crate::extensions::lsp_tests::package(&exe, &directory.path().join("wire.jsonl"));
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
        app.update(cx, |app, cx| app.open_file(external.clone(), window, cx));
    });
    crate::extensions::lsp_tests::publish(&app, &mut manager, visual);
    let server = visual.update(|_, cx| app.read(cx).language_servers["novel"].clone());
    server.prepare_until_ready().unwrap();
    let editor = visual.update(|window, cx| {
        // The plugin document API correctly keeps this external file outside its read authority.
        let app = app.read(cx);
        let index = app
            .tabs
            .iter()
            .position(|tab| tab.path() == external.canonicalize().unwrap())
            .unwrap();
        assert!(app.plugin_document_version(index).is_err());
        let editor = app.editor.clone();
        editor.update(cx, |state, cx| state.focus(window, cx));
        editor
    });
    visual.simulate_input("f");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        if visual.update(|_, cx| editor.read(cx).completion_menu_state().open) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native completion was lost because the file is outside guest scope"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        visual.update(|_, cx| editor.read(cx).completion_menu_state().items[0]
            .label
            .clone()),
        "fixture-completion"
    );
    visual.simulate_keystrokes("enter");
    visual.run_until_parked();
    assert_eq!(
        visual.update(|_, cx| editor.read(cx).text().to_string()),
        "fixture-completion"
    );
    assert_eq!(std::fs::read_to_string(&external).unwrap(), "");
    manager.disable("capability-example").unwrap();
    crate::extensions::lsp_tests::publish(&app, &mut manager, visual);
    assert!(!server.is_active());
}
