//! Independent WASM packages drive the production configuration actor, native windows and real sessions.
#![cfg(windows)]
use super::composable_tests::{publish_frame, pump_recording_all};
use super::*;
use gpui_kit::{TestAppContext, VisualContext as _, gpui};
use plugin_runtime::Manager;
use std::time::{Duration, Instant};

/// Persistent request state mirrors the production actor instead of synthesizing provider answers.
#[derive(Default)]
pub(crate) struct Driver {
    renderer: images::VectorRenderer,
    pub(crate) launches: Vec<(u64, String, u64)>,
    debug: BTreeMap<u64, (String, plugin_runtime::DebugRequest)>,
    targets: super::worker::targets::TargetCalls,
    configurations: super::worker::configurations::ConfigurationCalls,
}
impl Driver {
    /// Paint the active native window after publishing one real manager/actor iteration.
    pub(crate) fn frame(
        &mut self,
        manager: &mut Manager,
        app: &Entity<EditorApp>,
        cx: &mut gpui_kit::VisualTestContext,
    ) {
        manager.poll();
        let (statuses, stops) = pump_recording_all(
            manager,
            app,
            cx,
            &mut self.launches,
            &mut self.debug,
            &mut self.targets,
            &mut self.configurations,
        );
        publish_frame(
            manager,
            &mut self.renderer,
            app,
            cx,
            &self.launches,
            statuses,
            stops,
        );
    }
    /// Bound asynchronous acceptance by elapsed time, retaining every queued event between frames.
    #[track_caller]
    pub(crate) fn wait(
        &mut self,
        manager: &mut Manager,
        app: &Entity<EditorApp>,
        cx: &mut gpui_kit::VisualTestContext,
        predicate: impl Fn(&mut gpui_kit::VisualTestContext) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            self.frame(manager, app, cx);
            if predicate(cx) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "configuration flow timed out: {}",
                cx.update(|_, cx| app.read(cx).status.clone())
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Install two separately identified packages through ordinary public management, with fixture-local storage.
pub(crate) fn fixture<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (
    Manager,
    Entity<EditorApp>,
    &'a mut gpui_kit::VisualTestContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let mut manager = Manager::open(
        root.join("runtime"),
        protocol::Environment {
            workspace: root.display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for path in [
        repo.join("dist/plugins/terminal.zip"),
        repo.join("target/run-config-plugin-tree/configuration-alpha.zip"),
        repo.join("target/run-config-plugin-tree/configuration-beta.zip"),
    ] {
        let package = Package::read(&path).unwrap();
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
    }
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let workspace = Workspace::open(root).unwrap();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        app.update(cx, |app, _| {
            app.run_controls = crate::run::RunControls::load_with_project(
                &app.workspace_key(),
                Some(app.workspace.root().join("private-runs")),
                None,
            );
        });
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    cx.simulate_resize(size(px(1400.), px(900.)));
    (manager, slot.borrow_mut().take().unwrap(), cx)
}

/// Open the production owned dialog and direct physical input to its own window context.
pub(crate) fn open_form(
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
) -> gpui_kit::AnyWindowHandle {
    let parent = cx.window_handle();
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_run_config_dialog(window, cx, None)));
    cx.run_until_parked();
    let handle = cx.cx.update(|cx| app.read(cx).run_dialog_window.unwrap());
    *cx = gpui_kit::VisualTestContext::from_window(handle.into(), &cx.cx);
    cx.run_until_parked();
    parent
}

/// Click actual hit regions; hover participates in native button routing.
#[track_caller]
pub(crate) fn click(cx: &mut gpui_kit::VisualTestContext, selector: &'static str) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let Some(bounds) = cx.debug_bounds(selector) else {
        panic!("missing control {selector}");
    };
    let point = bounds.center();
    if selector.starts_with("plugin-") {
        let viewport = cx.debug_bounds("plugin-ui-form-scroll").unwrap();
        assert!(
            viewport.contains(&point),
            "control is clipped outside its native scroller: {selector}={point:?}, scroll={viewport:?}"
        );
    }
    click_at(cx, point);
}

/// Pointer input uses a real painted coordinate, including tree rows with generated identities.
pub(crate) fn click_at(
    cx: &mut gpui_kit::VisualTestContext,
    point: gpui_kit::Point<gpui_kit::Pixels>,
) {
    cx.simulate_event(gpui::MouseMoveEvent {
        position: point,
        pressed_button: None,
        modifiers: Default::default(),
    });
    cx.run_until_parked();
    cx.simulate_click(point, Default::default());
    cx.run_until_parked();
}

/// Baseline failure protection and single-row Apply are delivered with the first usable slice.
#[gpui::test]
#[ignore = "build configuration examples and terminal package with the public SDK first"]
fn plugin_configuration_apply_cancel_invalid_save_and_duplicate_templates(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (mut manager, app, cx) = fixture(cx, root.path());
    let mut driver = Driver::default();
    driver.frame(&mut manager, &app, cx);
    let parent = open_form(&app, cx);
    click(cx, "run-config-add");
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.debug_bounds("run-template-configuration-alpha-program")
            .is_some()
    });
    click(cx, "run-template-configuration-alpha-program");
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.debug_bounds("plugin-ui-name").is_some()
    });
    edit(cx, "plugin-ui-name", "applied");
    click(cx, "run-config-apply");
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.update(|_, cx| app.read(cx).run_controls.configurations().len() == 1)
    });
    assert!(cx.update(|_, cx| app.read(cx).run_form.is_some()));
    assert!(
        cx.update(|_, cx| app.read(cx).run_controls.selected().is_none()),
        "Apply does not select an external target"
    );
    let first = cx.update(|_, cx| app.read(cx).run_controls.configurations()[0].id.clone());
    edit(cx, "plugin-ui-name", "discard this edit");
    click(cx, "run-config-cancel");
    *cx = gpui_kit::VisualTestContext::from_window(parent, &cx.cx);
    assert_eq!(
        cx.update(|_, cx| app
            .read(cx)
            .run_controls
            .configuration(&first)
            .unwrap()
            .name
            .clone()),
        "applied"
    );
    open_form(&app, cx);
    driver.frame(&mut manager, &app, cx);
    assert!(
        cx.debug_bounds("plugin-configuration-empty").is_some(),
        "no external selection means no automatic row selection"
    );
    let tree = cx.debug_bounds("run-config-tree-list").unwrap();
    click_at(cx, point(tree.center().x, tree.top() + px(25.)));
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.debug_bounds("plugin-ui-name").is_some()
    });
    edit(cx, "plugin-ui-name", "");
    click(cx, "run-config-save");
    *cx = gpui_kit::VisualTestContext::from_window(parent, &cx.cx);
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.update(|_, cx| app.read(cx).run_form.is_none())
    });
    assert!(cx.update(|_, cx| {
        app.read(cx)
            .run_controls
            .plugin_configuration_blocker(&first)
            .is_some()
    }));
    let key = cx.update(|_, cx| app.read(cx).workspace_key());
    let stored = editor_core::load(&root.path().join("private-runs"), &key).unwrap();
    assert_eq!(stored.plugin_configurations[&first].name, "");
    assert!(matches!(
        stored.plugin_configurations[&first].validation,
        editor_core::ConfigurationValidation::Invalid(_)
    ));
    open_form(&app, cx);
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.debug_bounds("plugin-ui-name").is_some()
    });
    for _ in 0..2 {
        click(cx, "run-config-add");
        driver.wait(&mut manager, &app, cx, |cx| {
            cx.debug_bounds("run-template-configuration-alpha-program")
                .is_some()
        });
        click(cx, "run-template-configuration-alpha-program");
        driver.wait(&mut manager, &app, cx, |cx| {
            cx.debug_bounds("plugin-ui-name").is_some()
        });
    }
    click(cx, "run-config-save");
    *cx = gpui_kit::VisualTestContext::from_window(parent, &cx.cx);
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.update(|_, cx| app.read(cx).run_form.is_none())
    });
    let set = editor_core::load(&root.path().join("private-runs"), &key).unwrap();
    assert_eq!(set.configurations.len(), 3);
    assert_eq!(
        set.configurations
            .iter()
            .map(|configuration| &configuration.id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
    assert!(driver.launches.is_empty());
    manager.shutdown();
}

/// A real local filesystem failure retains the editable window and does not advance committed data.
#[gpui::test]
#[ignore = "build configuration examples and terminal package with the public SDK first"]
fn plugin_configuration_failed_write_keeps_the_draft(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (mut manager, app, cx) = fixture(cx, root.path());
    let mut driver = Driver::default();
    driver.frame(&mut manager, &app, cx);
    let parent = open_form(&app, cx);
    click(cx, "run-config-add");
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.debug_bounds("run-template-configuration-alpha-program")
            .is_some()
    });
    click(cx, "run-template-configuration-alpha-program");
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.debug_bounds("plugin-ui-name").is_some()
    });
    edit(cx, "plugin-ui-name", "keep this draft");
    // Replace this fixture's absent storage directory with an ordinary file; no user storage is touched.
    std::fs::write(root.path().join("private-runs"), "storage unavailable").unwrap();
    click(cx, "run-config-save");
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.debug_bounds("plugin-configuration-error").is_some()
    });
    assert!(cx.update(|_, cx| app.read(cx).run_form.is_some()));
    assert!(cx.update(|_, cx| app.read(cx).run_controls.configurations().is_empty()));
    assert!(cx.debug_bounds("plugin-ui-name").is_some());
    click(cx, "run-config-cancel");
    *cx = gpui_kit::VisualTestContext::from_window(parent, &cx.cx);
    assert!(driver.launches.is_empty());
    manager.shutdown();
}

/// Use native focus, selection and text entry; tests never write a provider's private form values.
#[track_caller]
pub(crate) fn edit(cx: &mut gpui_kit::VisualTestContext, selector: &'static str, value: &str) {
    click(cx, selector);
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_keystrokes("backspace");
    cx.simulate_input(value);
    cx.run_until_parked();
}

/// The executed program records each literal argument, independently from any host or Shell formatter.
fn probe(root: &Path) {
    let source = root.join("probe.rs");
    std::fs::write(&source, "//! Isolated argv acceptance program.\nfn main(){let args=std::env::args().skip(1).collect::<Vec<_>>();std::fs::write(\"argv.txt\",format!(\"{args:?}\")).unwrap();println!(\"CONFIGURATION_PROBE\");}\n").unwrap();
    let output = std::process::Command::new("rustc")
        .arg(&source)
        .arg("-o")
        .arg(root.join("probe.exe"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// C01/C02/C04-C08/C27: two plugin identities/layouts, editable literal argv, durable reopen and execution.
#[gpui::test]
#[ignore = "build configuration examples and terminal package with the public SDK first"]
fn plugin_configuration_packages_edit_save_reopen_and_execute(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    probe(root.path());
    let (mut manager, app, cx) = fixture(cx, root.path());
    let mut driver = Driver::default();
    driver.frame(&mut manager, &app, cx);
    let parent = open_form(&app, cx);
    driver.frame(&mut manager, &app, cx);
    assert!(cx.debug_bounds("plugin-configuration-empty").is_some());
    assert!(cx.debug_bounds("plugin-ui-name").is_none());
    assert_eq!(
        cx.cx.windows().len(),
        2,
        "one owned native window beside its parent"
    );
    let page = cx.debug_bounds("run-config-page").unwrap();
    click(cx, "run-config-add");
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.debug_bounds("run-template-configuration-alpha-program")
            .is_some()
    });
    let drawer = cx.debug_bounds("run-config-drawer").unwrap();
    assert!(drawer.size.width <= cx.debug_bounds("run-config-sidebar").unwrap().size.width);
    assert_eq!(
        page,
        cx.debug_bounds("run-config-page").unwrap(),
        "drawer leaves the right pane fixed"
    );
    click(cx, "run-template-configuration-alpha-program");
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.debug_bounds("plugin-ui-name").is_some()
    });
    assert!(cx.debug_bounds("run-config-drawer").is_none());
    let label = cx.debug_bounds("plugin-ui-name-field-label").unwrap();
    let input = cx.debug_bounds("plugin-ui-name").unwrap();
    assert!(
        label.bottom() <= input.top(),
        "provider A owns its vertical layout"
    );
    assert!(!root.path().join("argv.txt").exists());
    assert!(
        cx.update(|_, cx| app.read(cx).run_controls.configurations().is_empty()),
        "creating a draft does not save"
    );
    edit(cx, "plugin-ui-name", "中文 argv 配置");
    let arguments = ["run", "two words", "\"quoted\" 中文 ; & $literal"];
    edit(cx, "plugin-ui-argument-0", arguments[0]);
    for (index, value) in arguments.iter().enumerate().skip(1) {
        let selector = [
            "plugin-ui-argument-0",
            "plugin-ui-argument-1",
            "plugin-ui-argument-2",
        ][index];
        click(cx, "plugin-button-add-argument");
        driver.wait(&mut manager, &app, cx, |cx| {
            cx.debug_bounds(selector).is_some()
        });
        edit(cx, selector, value);
    }
    // Save immediately after typing: queued native events must finish before validation and persistence.
    click(cx, "run-config-save");
    *cx = gpui_kit::VisualTestContext::from_window(parent, &cx.cx);
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.update(|_, cx| app.read(cx).run_form.is_none())
    });
    let first = cx.update(|_, cx| app.read(cx).run_controls.selected().unwrap().clone());
    assert_eq!(first.name, "中文 argv 配置");
    assert_eq!(first.target.arguments(), arguments.map(str::to_owned));
    assert!(
        !root.path().join("argv.txt").exists(),
        "saving never executes"
    );
    assert!(
        !root.path().join(".me-editor").exists(),
        "configuration storage is host-local"
    );
    let stored = cx.update(|_, cx| {
        let key = app.read(cx).workspace_key();
        editor_core::load(&root.path().join("private-runs"), &key).unwrap()
    });
    assert_eq!(stored.selected().unwrap().id, first.id);
    assert_eq!(
        stored.plugin_configurations[&first.id].provider,
        "configuration-alpha"
    );
    open_form(&app, cx);
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.debug_bounds("plugin-ui-name").is_some()
    });
    click(cx, "run-config-add");
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.debug_bounds("run-template-configuration-beta-compact")
            .is_some()
    });
    click(cx, "run-template-configuration-beta-compact");
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.debug_bounds("plugin-ui-name").is_some()
    });
    let label = cx.debug_bounds("plugin-ui-name-field-label").unwrap();
    let input = cx.debug_bounds("plugin-ui-name").unwrap();
    assert!(
        label.right() <= input.left(),
        "provider B owns its horizontal layout"
    );
    edit(cx, "plugin-ui-name", "第二个插件");
    click(cx, "run-config-save");
    *cx = gpui_kit::VisualTestContext::from_window(parent, &cx.cx);
    driver.wait(&mut manager, &app, cx, |cx| {
        cx.update(|_, cx| app.read(cx).run_form.is_none())
    });
    assert_eq!(
        cx.update(|_, cx| app.read(cx).run_controls.configurations().len()),
        2
    );
    let beta = cx.update(|_, cx| app.read(cx).run_controls.selected().unwrap().id.clone());
    assert_eq!(
        cx.update(|_, cx| app.read(cx).run_controls.selected().unwrap().name.clone()),
        "第二个插件"
    );
    assert_ne!(beta, first.id);
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.start_configuration_without_environment(&first.id, window, cx)
        })
    });
    driver.wait(&mut manager, &app, cx, |_| {
        root.path().join("argv.txt").exists()
    });
    assert_eq!(
        std::fs::read_to_string(root.path().join("argv.txt")).unwrap(),
        format!("{:?}", arguments)
    );
    assert_eq!(driver.launches.len(), 1);
    manager.shutdown();
}
