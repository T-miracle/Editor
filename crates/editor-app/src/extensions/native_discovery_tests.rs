//! Real packages, native staged target selection and save, exact artifacts and explicit repair.
#![cfg(windows)]
use super::composable_tests::{publish_frame, publish_with_launches, pump_recording_all};
use super::*;
use gpui_kit::{TestAppContext, gpui};
use plugin_runtime::Manager;
use std::time::{Duration, Instant};

mod debugging;

/// Retain both production asynchronous dispatchers while native frames are painted.
#[derive(Default)]
struct Driver {
    renderer: images::VectorRenderer,
    launches: Vec<(u64, String, u64)>,
    debug: BTreeMap<u64, (String, plugin_runtime::DebugRequest)>,
    targets: super::worker::targets::TargetCalls,
    configurations: super::worker::configurations::ConfigurationCalls,
}
impl Driver {
    fn frame(
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
    #[track_caller]
    fn wait(
        &mut self,
        manager: &mut Manager,
        app: &Entity<EditorApp>,
        cx: &mut gpui_kit::VisualTestContext,
        predicate: impl Fn(&EditorApp) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            self.frame(manager, app, cx);
            if cx.update(|_, cx| predicate(app.read(cx))) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "native flow timed out: {}; {:?}; selected={:?}; frames={:?}",
                cx.update(|_, cx| app.read(cx).status.clone()),
                manager.debug_observations(),
                cx.update(|_, cx| app
                    .read(cx)
                    .run_controls
                    .debug_session()
                    .map(|(id, _)| id.to_owned())),
                cx.update(|_, cx| app.read(cx).run_controls.debug_frames().to_vec())
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
/// Install actual distribution ZIPs and keep all host-local state under the isolated fixture.
fn editor_for<'a>(
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
        root.join(".runtime-plugin-test"),
        protocol::Environment {
            workspace: root.display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    // Discovery/build rules remain plugins; the terminal is provided by the native application.
    for name in ["rust", "run-target-example"] {
        let package = Package::read(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../dist/plugins/{name}.zip")),
        )
        .unwrap();
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
    }
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let workspace = Workspace::open(root).unwrap();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        app.update(cx, |state, cx| {
            let key = state.workspace_key();
            state.run_controls = crate::run::RunControls::load_with_project(
                &key,
                Some(state.workspace.root().join("private-runs")),
                Some(state.workspace.root().into()),
            );
            cx.notify();
        });
        let holder = cx.new(|cx| FormWindow {
            owner: app.clone(),
            _observe: cx.observe(&app, |_, _, cx| cx.notify()),
        });
        Root::new(holder, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    cx.simulate_resize(size(px(1400.), px(900.)));
    let mut renderer = images::VectorRenderer::default();
    publish_with_launches(&mut manager, &mut renderer, &app, cx, &[]);
    (manager, app, cx)
}
/// The production simplified renderer repaints after asynchronous catalog and native input changes.
struct FormWindow {
    owner: Entity<EditorApp>,
    _observe: Subscription,
}
impl Render for FormWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.owner.read(cx).run_form.is_some() {
            crate::run::ui::render_run_config_form(
                &self.owner.downgrade(),
                crate::ui::controls::DialogContent::new(),
                window,
                cx,
            )
            .into_any_element()
        } else {
            div().child(self.owner.clone()).into_any_element()
        }
    }
}
fn form_window<'a>(
    cx: &'a mut gpui_kit::VisualTestContext,
    app: &Entity<EditorApp>,
) -> &'a mut gpui_kit::VisualTestContext {
    // Use one native test window. Painting the same EditorApp in two test windows causes
    // its measured layout to alternate indefinitely; this holder mounts exactly one view.
    cx.update(|window, cx| {
        let key = app.read(cx).workspace_key();
        let controls = app.read(cx).run_controls.clone();
        let form = cx.new(|cx| crate::run::RunConfigForm::open(&controls, &key, None, window, cx));
        app.update(cx, |state, cx| {
            state.run_form = Some(form);
            cx.notify();
        });
    });
    cx.simulate_resize(size(px(900.), px(900.)));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx
}
/// Return to the actual editor view after confirming/editing the native form.
fn main_window<'a>(
    cx: &'a mut gpui_kit::VisualTestContext,
    app: &Entity<EditorApp>,
) -> &'a mut gpui_kit::VisualTestContext {
    let _ = app;
    cx.simulate_resize(size(px(1400.), px(900.)));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx
}
/// Click the painted native hit region, never a synthetic action dispatched by selector.
fn click(cx: &mut gpui_kit::VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing native control {selector}"));
    // Physical clicks move the pointer first; hover hit testing is part of native event routing.
    cx.simulate_event(gpui::MouseMoveEvent {
        position: bounds.center(),
        pressed_button: None,
        modifiers: Default::default(),
    });
    cx.run_until_parked();
    cx.simulate_click(bounds.center(), Default::default());
    cx.run_until_parked();
}
/// Native focus/Ctrl+A/text input must preserve literal multi-line values.
fn edit(cx: &mut gpui_kit::VisualTestContext, selector: &'static str, value: &str) {
    click(cx, selector);
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input(value);
    cx.run_until_parked();
}
/// The final program itself records argv/env so a compiled artifact is distinguishable from Cargo.
fn rust_project(root: &Path, name: &str) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        format!("[package]\nname=\"{name}\"\nversion=\"0.1.0\"\nedition=\"2024\"\n"),
    )
    .unwrap();
    std::fs::write(root.join("src/main.rs"),"//! Native discovery acceptance program.\nfn main() {let value=format!(\"{:?} / {}\",std::env::args().skip(1).collect::<Vec<_>>(),std::env::var(\"RUN_ENV\").unwrap_or_default());std::fs::write(\"program-ran.txt\",value).unwrap();}\n").unwrap();
}

#[gpui::test]
#[ignore = "build rust and run-target-example through the current public SDK first"]
fn a_real_rust_project_is_discovered_built_and_run(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    rust_project(root.path(), "delivery-target");
    let (mut manager, app, cx) = editor_for(cx, root.path());
    let mut driver = Driver::default();
    let cx = form_window(cx, &app);

    click(cx, "run-config-discover");
    driver.wait(&mut manager, &app, cx, |state| {
        state.run_controls.discovered_targets().len() == 2
    });
    assert!(
        cx.update(|_, cx| app.read(cx).run_controls.configurations().is_empty()),
        "discovery stores no candidates"
    );
    assert!(!root.path().join("program-ran.txt").exists());
    let target = cx.update(|_, cx| {
        app.read(cx)
            .run_controls
            .discovered_targets()
            .iter()
            .find(|target| target.label.ends_with("Debug"))
            .unwrap()
            .clone()
    });
    let selector = Box::leak(format!("run-config-target-{}", target.id).into_boxed_str());
    click(cx, selector);

    let stored = cx.update(|_, cx| {
        assert!(
            app.read(cx).run_controls.configurations().is_empty(),
            "a plugin choice is only a draft until Save"
        );
        app.read(cx)
            .run_form
            .as_ref()
            .unwrap()
            .read(cx)
            .draft()
            .id
            .clone()
    });
    edit(cx, "run-config-name", "用户保留的名称");
    click(cx, "run-config-arguments-edit");
    edit(
        cx,
        "run-config-arguments",
        "literal space\nquote\"value\n中文;&|",
    );
    click(cx, "run-config-detail-done");
    click(cx, "run-config-more");
    click(cx, "run-config-environment-edit");
    edit(cx, "run-config-environment", "RUN_ENV=本机环境");
    click(cx, "run-config-detail-done");
    click(cx, "run-config-save");
    let config = cx.update(|_, cx| {
        app.read(cx)
            .run_controls
            .configuration(&stored)
            .unwrap()
            .clone()
    });
    assert_eq!(config.name, "用户保留的名称");
    assert_eq!(
        config.literal_arguments(),
        ["literal space", "quote\"value", "中文;&|"]
    );
    assert_eq!(config.env["RUN_ENV"], "本机环境");
    assert!(matches!(
        config.target,
        editor_core::RunTarget::Provided { .. }
    ));
    let cx = main_window(cx, &app);
    click(cx, "run-build");
    driver.wait(&mut manager, &app, cx, |state| {
        !state.run_controls.is_pending(&stored) && !state.run_controls.is_preparing(&stored)
    });
    assert!(driver.launches.is_empty(), "Build creates no final program");
    assert!(!root.path().join("program-ran.txt").exists());
    assert!(cx.debug_bounds("run-build-output").is_some());
    click(cx, "run-build-hide");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("run-build-output").is_none());
    cx.update(|_, cx| {
        app.update(cx, |state, cx| {
            let request = state
                .run_controls
                .menu_entries()
                .iter()
                .find_map(|row| match row {
                    crate::run::RunMenuEntry::Action { id, .. } => id
                        .strip_prefix("run-preparation-")
                        .and_then(|id| id.parse().ok()),
                    _ => None,
                })
                .unwrap();
            state.run_controls.show_preparation_output(request);
            cx.notify();
        })
    });
    driver.frame(&mut manager, &app, cx);
    assert!(cx.debug_bounds("run-build-output").is_some());
    click(cx, "run-start");
    driver.wait(&mut manager, &app, cx, |state| {
        !state.run_controls.is_pending(&stored) && !state.run_controls.is_preparing(&stored)
    });
    assert_eq!(driver.launches.len(), 1, "the artifact is executed once");
    let request = manager
        .execution(driver.launches[0].0)
        .unwrap()
        .request()
        .clone();
    assert!(request.program.ends_with("delivery-target.exe") && !request.program.contains("cargo"));
    let result = std::fs::read_to_string(root.path().join("program-ran.txt")).unwrap();
    assert!(
        result.contains("literal space")
            && result.contains("中文;&|")
            && result.ends_with(" / 本机环境"),
        "{result}"
    );
    manager.shutdown();
}

/// Rename/repair preserves edits; an independent plugin's defaults are staged and explicitly saved.
#[gpui::test]
#[ignore = "build rust and run-target-example through the current public SDK first"]
fn a_discovered_project_is_confirmed_built_run_and_offered_for_debugging(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    rust_project(root.path(), "original-target");
    std::fs::create_dir_all(root.path().join("tool")).unwrap();
    std::fs::write(root.path().join("tool/native-tool.toml"),"[tool]\nname=\"Independent native tool\"\nprogram=\"powershell.exe\"\narguments=\"-NoProfile\\n-Command\\nWrite-Output INDEPENDENT\"\n").unwrap();
    let (mut manager, app, cx) = editor_for(cx, root.path());
    let mut driver = Driver::default();
    cx.update(|_, cx| app.update(cx, |state, cx| state.discover_run_targets(cx)));
    driver.wait(&mut manager, &app, cx, |state| {
        state.run_controls.discovered_targets().len() == 3
    });
    let (target, independent) = cx.update(|_, cx| {
        let targets = app.read(cx).run_controls.discovered_targets();
        (
            targets
                .iter()
                .find(|target| target.label.ends_with("Debug"))
                .unwrap()
                .id
                .clone(),
            targets
                .iter()
                .find(|target| target.provider == "run-target-example")
                .unwrap()
                .clone(),
        )
    });
    assert_eq!(independent.target_type, "native-tool");
    let stored = cx.update(|_, cx| {
        app.update(cx, |state, _| {
            let key = state.workspace_key();
            let id = state.run_controls.confirm_target(&target, &key).unwrap();
            let mut config = state.run_controls.configuration(&id).unwrap().clone();
            config.name = "My retained edits".into();
            config.env.insert("RUN_ENV".into(), "RETAINED".into());
            if let editor_core::RunTarget::Provided { args, .. } = &mut config.target {
                args.push("user argv".into());
            }
            state.run_controls.upsert(config, &key).unwrap();
            id
        })
    });
    rust_project(root.path(), "renamed-target");
    cx.update(|_, cx| app.update(cx, |state, cx| state.discover_run_targets(cx)));
    driver.wait(&mut manager, &app, cx, |state| {
        state.run_controls.target_missing(&stored)
    });
    assert!(cx.update(|_, cx| app.read(cx).run_controls.launch_blocker(&stored).is_some()));
    assert!(driver.launches.is_empty());
    let renamed = cx.update(|_, cx| {
        app.read(cx)
            .run_controls
            .discovered_targets()
            .iter()
            .find(|target| target.label.ends_with("Debug"))
            .unwrap()
            .id
            .clone()
    });
    cx.update(|_, cx| {
        app.update(cx, |state, _| {
            let key = state.workspace_key();
            state
                .run_controls
                .repair_target_with(&stored, &renamed, &key)
                .unwrap();
        })
    });
    let repaired = cx.update(|_, cx| {
        app.read(cx)
            .run_controls
            .configuration(&stored)
            .unwrap()
            .clone()
    });
    assert_eq!(repaired.name, "My retained edits");
    assert_eq!(repaired.env["RUN_ENV"], "RETAINED");
    assert_eq!(repaired.literal_arguments(), ["user argv"]);
    let count = cx.update(|_, cx| app.read(cx).run_controls.configurations().len());
    click(cx, "run-start");
    driver.wait(&mut manager, &app, cx, |state| {
        !state.run_controls.is_pending(&stored) && !state.run_controls.is_preparing(&stored)
    });
    assert_eq!(driver.launches.len(), 1);
    assert!(cx.update(|_, cx| app.read(cx).run_controls.debug_blocker(&stored).is_some()));
    assert_eq!(
        std::fs::read_to_string(root.path().join("program-ran.txt")).unwrap(),
        "[\"user argv\"] / RETAINED"
    );
    let cx = form_window(cx, &app);
    let selector = Box::leak(format!("run-config-target-{}", independent.id).into_boxed_str());
    click(cx, selector);
    assert_eq!(
        cx.update(|_, cx| app.read(cx).run_controls.configurations().len()),
        count
    );
    click(cx, "run-config-save");
    assert!(cx.update(|_,cx|matches!(app.read(cx).run_controls.selected().unwrap().target,editor_core::RunTarget::Program {ref program,..} if program=="powershell.exe")));
    assert_eq!(
        cx.update(|_, cx| app.read(cx).run_controls.configurations().len()),
        count + 1
    );
    manager.shutdown();
}
