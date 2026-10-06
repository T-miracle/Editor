//! Native clicks and keyboard navigation use independently installed guests for both toolbar groups.
use super::package_ui_test_support::*;
use super::*;
use gpui_kit::{TestAppContext, VisualTestContext, gpui};
use plugin_runtime::Manager;

/// Perform real pointer activation at the recorded native bounds, then settle public runtime views.
fn click(
    id: &'static str,
    app: &Entity<EditorApp>,
    manager: &mut Manager,
    visual: &mut VisualTestContext,
) {
    let point = visual
        .debug_bounds(id)
        .unwrap_or_else(|| panic!("missing native button {id}"))
        .center();
    visual.simulate_click(point, Modifiers::default());
    draw(app, manager, visual);
}

/// Read the guest's selected contribution after native activation has crossed the public manager.
fn selected(manager: &Manager) -> &str {
    let document = &manager.live["layout-example"].views["layout"];
    &document
        .tools
        .iter()
        .find(|tool| tool.selected)
        .unwrap_or_else(|| {
            panic!(
                "no selected tool: source={:?} file={:?} tools={:?}",
                document.source, document.file, document.tools
            )
        })
        .id
}
/// Counter text is an observable guest result rather than a duplicated host business state.
fn count(manager: &Manager, panel: &str) -> String {
    let root = &manager.live["tools-example"].views[panel].root;
    let id = if panel == "auxiliary" {
        "aux-count"
    } else {
        "window-count"
    };
    let mut count = None;
    root.visit(&mut |node| {
        if node.id == id
            && let protocol::ui::Kind::Text { text } = &node.kind
        {
            count = Some(text.clone());
        }
    });
    count.expect("counter text")
}

/// Startup may open another workspace file; activation follows file identity rather than a guessed index.
fn activate(path: &Path, app: &Entity<EditorApp>, visual: &mut VisualTestContext) {
    let path = path.canonicalize().unwrap();
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            let index = app.tabs.iter().position(|tab| tab.path() == path).unwrap();
            app.activate_tab(index, window, cx);
        })
    });
}

/// Layout and auxiliary tools coexist, window functions keep their focus target, and each group overflows.
#[gpui::test]
#[ignore = "build actual public-SDK packages with scripts/build-layout-example.ps1 first"]
fn real_plugin_tools_preserve_context_shared_intent_and_separate_overflow(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("first.layout");
    let second = directory.path().join("second.layout");
    std::fs::write(&first, "first draft").unwrap();
    std::fs::write(&second, "second draft").unwrap();
    let mut manager = Manager::open(
        // Match the existing test host's package root so artwork is read through ordinary ownership.
        directory.path().join(".runtime-plugin-test"),
        protocol::Environment {
            workspace: directory.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let tools = Package::read(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-layout-test/tools-example.zip"),
    )
    .unwrap();
    for package in [package("layout-example", "Layout Example"), tools] {
        manager
            .install(&package, package.manifest.permissions.clone())
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
        app.update(cx, |app, cx| app.open_file(first.clone(), window, cx));
    });
    draw(&app, &mut manager, visual);
    assert!(
        visual.debug_bounds("plugin-tool-group-separator").is_some(),
        "{}",
        visual.update(|_, cx| {
            let app = app.read(cx);
            format!(
                "context={:?}; file={:?}; panels={:?}",
                app.function_context,
                app.active_tab_index()
                    .and_then(|index| app.plugin_file_context(index).ok()),
                app.plugin_panels
                    .iter()
                    .map(|(key, panel)| {
                        let panel = panel.read(cx);
                        (
                            key,
                            panel.visible.get(),
                            panel.preview_version.clone(),
                            panel.preview_file.clone(),
                            panel.tool_icons.len(),
                            panel.current_document().map(|doc| doc.tools.len()),
                        )
                    })
                    .collect::<Vec<_>>()
            )
        })
    );
    assert!(
        visual
            .debug_bounds("plugin-window-layout-example/layout")
            .is_none()
    );
    assert!(
        visual
            .debug_bounds("plugin-window-tools-example/auxiliary")
            .is_none()
    );
    assert!(
        visual
            .debug_bounds("plugin-tool-tools-example/auxiliary/aux-7")
            .is_none()
    );
    let editor = visual.debug_bounds("editor-source-pane").unwrap().center();
    visual.simulate_click(editor, Modifiers::default());
    visual.simulate_input("未保存");
    draw(&app, &mut manager, visual);
    let first_draft = visual.update(|_, cx| app.read(cx).editor.read(cx).text().to_string());
    click(
        "plugin-tool-layout-example/layout/tool-column",
        &app,
        &mut manager,
        visual,
    );
    assert_eq!(selected(&manager), "tool-column");
    let editor = visual.debug_bounds("editor-source-pane").unwrap();
    let content = visual.debug_bounds("plugin-ui-plugin-content").unwrap();
    assert!(editor.bottom() <= content.top() + px(2.));
    click(
        "plugin-tool-tools-example/auxiliary/aux-0",
        &app,
        &mut manager,
        visual,
    );
    assert_eq!(count(&manager, "auxiliary"), "File count: 1");
    // The disabled contribution is drawn but cannot enqueue or execute its guest function.
    click(
        "plugin-tool-tools-example/auxiliary/aux-2",
        &app,
        &mut manager,
        visual,
    );
    assert_eq!(count(&manager, "auxiliary"), "File count: 1");
    visual.update(|window, cx| app.update(cx, |app, cx| app.open_file(second.clone(), window, cx)));
    draw(&app, &mut manager, visual);
    assert_eq!(selected(&manager), "tool-column");
    assert_eq!(
        visual.update(|_, cx| app.read(cx).editor.read(cx).text().to_string()),
        "second draft"
    );
    click(
        "plugin-tool-layout-example/layout/tool-content",
        &app,
        &mut manager,
        visual,
    );
    assert!(visual.debug_bounds("editor-source-pane").is_none());
    activate(&first, &app, visual);
    draw(&app, &mut manager, visual);
    assert_eq!(selected(&manager), "tool-content");
    assert_eq!(
        visual.update(|_, cx| app.read(cx).editor.read(cx).text().to_string()),
        first_draft
    );
    // Showing a real independent window selects its functions; toolbar focus retains that owner.
    click(
        "plugin-window-tools-example/window-a",
        &app,
        &mut manager,
        visual,
    );
    assert!(
        visual
            .debug_bounds("plugin-tool-tools-example/window-a/increment")
            .is_some()
    );
    assert!(
        visual
            .debug_bounds("plugin-tool-layout-example/layout/tool-row")
            .is_none()
    );
    click(
        "plugin-tool-tools-example/window-a/increment",
        &app,
        &mut manager,
        visual,
    );
    assert_eq!(count(&manager, "window-a"), "Window count: 1");
    assert_eq!(count(&manager, "auxiliary"), "File count: 1");
    click(
        "plugin-window-tools-example/window-a",
        &app,
        &mut manager,
        visual,
    );
    assert!(
        visual
            .debug_bounds("plugin-tool-layout-example/layout/tool-row")
            .is_some()
    );
    click(
        "plugin-tool-layout-example/layout/tool-row",
        &app,
        &mut manager,
        visual,
    );
    visual.simulate_resize(size(px(360.), px(700.)));
    draw(&app, &mut manager, visual);
    let left = visual.debug_bounds("plugin-windows-overflow").unwrap();
    let right = visual.debug_bounds("plugin-tools-overflow").unwrap();
    let separator = visual.debug_bounds("plugin-tool-group-separator").unwrap();
    assert!(left.right() <= separator.left() && separator.right() <= right.left());
    assert!((left.top() - right.top()).abs() < px(2.));
    click("plugin-tools-overflow", &app, &mut manager, visual);
    let items = visual.update(|_, cx| {
        app.read(cx)
            .tool_overflow
            .as_ref()
            .unwrap()
            .read(cx)
            .items
            .clone()
    });
    assert!(items.iter().any(|item| item.disabled));
    assert!(items.iter().any(|item| item.label.starts_with("✓ ")));
    // The third down step crosses a disabled item and still reaches an enabled guest function.
    visual.simulate_keystrokes("home down down down enter");
    draw(&app, &mut manager, visual);
    assert_eq!(count(&manager, "auxiliary"), "File count: 2");
    click("plugin-tools-overflow", &app, &mut manager, visual);
    visual.simulate_keystrokes("home enter");
    draw(&app, &mut manager, visual);
    assert!(visual.update(|_, cx| app.read(cx).tool_overflow.is_none()));
    // Opening a popup then replacing its file prevents the captured old function from executing.
    click("plugin-tools-overflow", &app, &mut manager, visual);
    let before = count(&manager, "auxiliary");
    activate(&second, &app, visual);
    draw(&app, &mut manager, visual);
    visual.simulate_keystrokes("end enter");
    draw(&app, &mut manager, visual);
    assert_eq!(count(&manager, "auxiliary"), before);
    assert!(
        visual.update(|_, cx| app.read(cx).tool_overflow.is_none()),
        "stale popup still owns a captured action"
    );
    assert_eq!(
        visual.update(|_, cx| app.read(cx).editor.read(cx).text().to_string()),
        "second draft",
        "popup keys must not edit a different file"
    );
    visual.simulate_keystrokes("escape");
    click("plugin-windows-overflow", &app, &mut manager, visual);
    let items = visual.update(|_, cx| {
        app.read(cx)
            .tool_overflow
            .as_ref()
            .unwrap()
            .read(cx)
            .items
            .clone()
    });
    assert!(items.iter().all(|item| item.label.starts_with("Window ")));
    visual.simulate_keystrokes("end enter");
    draw(&app, &mut manager, visual);
    assert!(
        visual
            .debug_bounds("plugin-tool-tools-example/window-e/increment")
            .is_some()
    );
    // Theme changes reselect package artwork and keep controls on one native status line.
    visual.update(|_, cx| apply_theme(builtin_theme(true), cx));
    visual.simulate_scale_factor_change(1.5);
    draw(&app, &mut manager, visual);
    assert_eq!(visual.update(|window, _| window.scale_factor()), 1.5);
    assert!(visual.debug_bounds("plugin-tool-group-separator").is_some());
    click(
        "plugin-tool-tools-example/window-e/increment",
        &app,
        &mut manager,
        visual,
    );
    assert_eq!(count(&manager, "window-e"), "Window count: 1");
    manager.disable("tools-example").unwrap();
    draw(&app, &mut manager, visual);
    assert!(
        visual
            .debug_bounds("plugin-window-tools-example/window-a")
            .is_none()
    );
    assert!(
        visual
            .debug_bounds("plugin-tool-tools-example/window-e/increment")
            .is_none()
    );
    assert_eq!(
        visual.update(|_, cx| app.read(cx).editor.read(cx).text().to_string()),
        "second draft"
    );
}
