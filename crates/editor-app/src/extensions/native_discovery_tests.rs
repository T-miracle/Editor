//! The vertical acceptance for Rust discovery: a real project's target, confirmed and run.
//!
//! Everything here goes through the seams the editor uses — the shipped plugin's own contribution
//! file, the host's catalog, the run controls' discovery, and the real terminal package starting
//! real programs — so the check is about what a user gets, not about a helper's behaviour.
#![cfg(windows)]
use super::composable_tests::{publish_with_launches, pump_recording};
use super::*;
use gpui_kit::{TestAppContext, gpui};

/// Whether the toolchain discovery will ask for is available on this machine.
fn cargo_available() -> bool {
    std::process::Command::new("cargo")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// A minimal Rust project whose program prints something a test can see.
fn rust_project(root: &std::path::Path, name: &str, marker: &str) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
             [dependencies]\n"
        ),
    )
    .unwrap();
    std::fs::write(
        root.join("src/main.rs"),
        format!("fn main() {{ println!(\"{marker}\"); }}\n"),
    )
    .unwrap();
}

/// Install the real terminal package and open the editor on this workspace.
fn editor_for<'a>(
    cx: &'a mut TestAppContext,
    root: &std::path::Path,
) -> (
    plugin_runtime::Manager,
    Entity<EditorApp>,
    &'a mut gpui_kit::VisualTestContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let mut manager = plugin_runtime::Manager::open(
        root.join("runtime"),
        protocol::Environment {
            workspace: root.display().to_string(),
            os: "windows".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let terminal = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/terminal.zip"),
    )
    .unwrap();
    let grants = terminal.manifest.permissions.clone();
    manager.install(&terminal, grants).unwrap();

    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let workspace = Workspace::open(root).unwrap();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    cx.simulate_resize(size(px(1400.), px(900.)));
    let mut renderer = images::VectorRenderer::default();
    publish_with_launches(&mut manager, &mut renderer, &app, cx, &[]);
    (manager, app, cx)
}

/// Drive one frame of the editor's own loop.
fn frame(
    manager: &mut plugin_runtime::Manager,
    renderer: &mut images::VectorRenderer,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
    launches: &mut Vec<(u64, String, u64)>,
) {
    let (statuses, stops) = pump_recording(manager, app, cx, launches);
    manager.poll();
    super::composable_tests::publish_frame(manager, renderer, app, cx, launches, statuses, stops);
    std::thread::sleep(std::time::Duration::from_millis(20));
}

/// End the test without leaving a program running on the machine.
fn shut_down(
    manager: &mut plugin_runtime::Manager,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
) {
    let sessions = cx.update(|_, cx| app.read(cx).run_controls.active_sessions());
    for session in sessions {
        let _ = manager.stop_execution(session.id);
    }
    for _ in 0..40 {
        manager.poll();
        if manager.executions().iter().all(|execution| {
            !matches!(
                execution.state(),
                plugin_runtime::ExecutionState::Starting | plugin_runtime::ExecutionState::Running
            )
        }) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    manager.shutdown();
}

/// A real Rust project is discovered, confirmed, built and run through the editor's own controls.
#[gpui::test]
#[ignore = "build terminal and capability-example through the public SDK first"]
fn a_real_rust_project_is_discovered_built_and_run(cx: &mut TestAppContext) {
    if !cargo_available() {
        // The toolchain is an environment fact, not something this test may install.
        eprintln!("skipping: cargo is not available on this machine");
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let name = "discovery-acceptance";
    let marker = "DISCOVERY_TARGET_RAN";
    rust_project(root.path(), name, marker);
    let (mut manager, app, cx) = editor_for(cx, root.path());
    let mut renderer = images::VectorRenderer::default();
    // Publish the shipped Rust plugin's own declarations into the host catalog, after the editor has
    // started: opening a workspace refreshes the catalog, and this is the package under test.
    crate::extensions::contributions::publish_declarative_plugin_for_test(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/rust"),
        "rust",
    );
    // Discovery is asked for through the editor's own entry point.
    cx.update(|_, cx| {
        app.update(cx, |app, cx| app.discover_run_targets(cx));
    });
    let (targets, status) = cx.update(|_, cx| {
        let state = app.read(cx);
        (
            state.run_controls.discovered_targets().to_vec(),
            state.status.clone(),
        )
    });
    assert!(
        !targets.is_empty(),
        "the project's target was found: {status}"
    );
    let target = targets
        .iter()
        .find(|target| target.label == name)
        .unwrap_or_else(|| panic!("the package target is offered: {targets:?}"));
    assert_eq!(
        target.program, "cargo",
        "the program is the toolchain the provider declared, not the artifact"
    );
    assert_eq!(
        target.fields.get("package").map(String::as_str),
        Some(name),
        "the package the manifest declares is reported"
    );
    assert_eq!(
        targets.len(),
        targets
            .iter()
            .map(|target| target.id.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        "no target is offered twice"
    );
    // Discovery stored nothing: confirming is what adds a configuration.
    assert!(cx.update(|_, cx| app.read(cx).run_controls.configurations().is_empty()));

    let target_id = target.id.clone();
    let workspace = cx.update(|_, cx| app.read(cx).workspace_key());
    let stored = cx
        .update(|_, cx| {
            app.update(cx, |app, _| {
                app.run_controls.confirm_target(&target_id, &workspace)
            })
        })
        .expect("the candidate is confirmed");
    let (configuration, plan) = cx.update(|_, cx| {
        let state = app.read(cx);
        let configuration = state
            .run_controls
            .configuration(&stored)
            .cloned()
            .expect("it is stored");
        // The confirmed configuration runs the binary the build produces, and its own build action
        // is the command the provider described.
        let plan = state
            .run_controls
            .launch_plan(&stored, &workspace)
            .expect("the confirmed configuration is launchable");
        (configuration, plan)
    });
    assert_eq!(
        configuration.target.executable(),
        "cargo",
        "the program runs the target through the toolchain that builds it"
    );
    assert_eq!(
        configuration.build.len(),
        1,
        "the provider's build action is part of the confirmed configuration"
    );
    // The plan is the whole launch: the build, the step that runs the target, then the program. A
    // package manager's own subcommand belongs in a step, so the program field stays the artifact
    // for a debugger to attach to rather than the manager.
    assert_eq!(
        plan.steps.iter().map(|step| step.kind).collect::<Vec<_>>(),
        vec![
            crate::run::StepKind::Build,
            crate::run::StepKind::Prelaunch,
            crate::run::StepKind::Program
        ],
        "the launch builds, then runs the target, then leaves the program"
    );
    assert_eq!(plan.steps[0].request.program, "cargo.exe");
    assert_eq!(plan.steps[0].request.args, vec!["build".to_owned()]);
    assert_eq!(plan.steps[2].request.program, "cargo");

    // Run it: the build compiles the project and the program is the artifact just built.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.start_configuration_without_environment(&stored, window, cx);
        });
    });
    let mut launches = Vec::new();
    let mut finished = false;
    for _ in 0..1200 {
        frame(&mut manager, &mut renderer, &app, cx, &mut launches);
        let complete = cx.update(|_, cx| app.read(cx).run_controls.preparation_complete(&stored));
        if complete {
            finished = true;
            break;
        }
    }
    let status = cx.update(|_, cx| app.read(cx).status.clone());
    assert!(finished, "the launch prepared and started: {status}");
    assert_eq!(
        launches.len(),
        3,
        "a build, the binary's own step and the program: {status}"
    );
    // The build really produced an artifact, and the program started it.
    let artifact = root
        .path()
        .join("target")
        .join("debug")
        .join(format!("{name}.exe"));
    assert!(
        artifact.exists(),
        "the build produced the artifact it was asked for"
    );
    let program = manager
        .execution(launches[2].0)
        .expect("the program session exists")
        .request()
        .clone();
    assert_eq!(
        program.program, "cargo",
        "the program is the command the provider described"
    );
    assert_eq!(program.args, vec!["run".to_owned()]);
    let cwd = program.cwd.clone().unwrap_or_default();
    assert_eq!(
        cwd.trim_start_matches(r"\\?\"),
        root.path().display().to_string(),
        "the program runs in this project"
    );
    shut_down(&mut manager, &app, cx);
    let _ = cx;
}
