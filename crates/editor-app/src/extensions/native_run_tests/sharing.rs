//! Real B1 sharing, separate project-local overrides, hand edits and a pinned active launch snapshot.
use super::*;
use std::time::{Duration, Instant};

/// Each isolated project records its own literal argv/cwd/environment through the public terminal.
#[gpui::test]
#[ignore = "build the current terminal package through the public SDK first"]
fn native_shared_configuration_runs_in_two_isolated_projects(cx: &mut TestAppContext) {
    let roots = tempfile::tempdir().unwrap();
    let first = roots.path().join("first");
    let second = roots.path().join("second");
    for root in [&first, &second] {
        std::fs::create_dir_all(root.join("personal-bin")).unwrap();
    }
    let first = first.canonicalize().unwrap();
    let second = second.canonicalize().unwrap();
    let source = first.join("probe.rs");
    std::fs::write(&source, PROBE).unwrap();
    let executable = first.join("personal-bin/share-probe.exe");
    let build = std::process::Command::new("rustc")
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    std::fs::copy(&executable, second.join("personal-bin/share-probe.exe")).unwrap();
    let (mut manager, app, window) = fixture(
        cx,
        &first,
        "share-probe.exe",
        vec!["literal space".into(), "quote\"中文;&|".into()],
    );
    let id = window.update(|_, cx| {
        app.update(cx, |state, _| {
            let key = state.workspace_key();
            let mut config = state.run_controls.selected().unwrap().clone();
            config.directory = Some(first.display().to_string());
            config
                .env
                .insert("LOCAL_VALUE".into(), "first-private".into());
            config.tool_paths = vec![first.join("personal-bin").display().to_string()];
            let id = config.id.clone();
            state.run_controls.upsert(config, &key).unwrap();
            id
        })
    });
    window.update(|window, cx| {
        app.update(cx, |state, cx| {
            state.open_run_config_dialog(window, cx, Some(id.clone()))
        })
    });
    window.run_until_parked();
    click(window, "run-config-shared");
    click(window, "run-config-save");
    assert!(window.update(|_, cx| app.read(cx).run_form.is_none()));
    let shared_path = editor_core::project_path(&first);
    let bytes = std::fs::read(&shared_path).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(
        !text.contains("first-private")
            && !text.contains("personal-bin")
            && !text.contains(&first.display().to_string())
    );
    let shared = editor_core::SharedSet::from_json(&bytes).unwrap();
    editor_core::save_shared(&second, &shared).unwrap();
    let mut renderer = images::VectorRenderer::default();
    let mut launches = Vec::new();
    click(window, "run-start");
    wait_for_file(
        &mut manager,
        &app,
        window,
        &mut renderer,
        &mut launches,
        &first.join("observed.txt"),
    );
    let original = std::fs::read_to_string(first.join("observed.txt")).unwrap();
    assert!(
        original.contains("literal space")
            && original.contains("quote\\\"中文;&|")
            && original.ends_with("|first-private"),
        "{original}"
    );
    assert_eq!(
        std::path::Path::new(original.rsplit('|').nth(1).unwrap())
            .canonicalize()
            .unwrap(),
        first
    );
    // The hand edit controls future launches, while the active process remains on its original argv.
    let mut edited = editor_core::load_shared(&first).unwrap();
    edited.configurations[0].target = editor_core::RunTarget::Program {
        program: "share-probe.exe".into(),
        args: vec!["hand edited".into()],
    };
    editor_core::save_shared(&first, &edited).unwrap();
    click(window, "run-start");
    pump_recording(&mut manager, &app, window, &mut launches);
    publish_with_launches(&mut manager, &mut renderer, &app, window, &launches);
    assert_eq!(launches.len(), 1);
    assert_eq!(
        std::fs::read_to_string(first.join("observed.txt")).unwrap(),
        original
    );
    manager.shutdown();
    // Reopening the first project uses the external edit and its preserved host-local environment.
    let reopened = crate::run::RunControls::load_with_project(
        &state_key(&first),
        Some(first.join("private-runs")),
        Some(first.clone()),
    );
    assert_eq!(
        reopened.configuration(&id).unwrap().literal_arguments(),
        ["hand edited"]
    );
    assert_eq!(
        reopened.configuration(&id).unwrap().env["LOCAL_VALUE"],
        "first-private"
    );
    editor_core::save_shared(&second, &edited).unwrap();
    let (mut manager, app, window) = fixture(cx, &second, "share-probe.exe", vec![]);
    window.update(|_, cx| {
        app.update(cx, |state, cx| {
            let key = state.workspace_key();
            let mut config = state.run_controls.configuration(&id).unwrap().clone();
            assert!(config.env.is_empty() && config.tool_paths.is_empty());
            assert_eq!(config.directory.as_deref(), Some(second.to_str().unwrap()));
            config
                .env
                .insert("LOCAL_VALUE".into(), "second-private".into());
            config.tool_paths = vec![second.join("personal-bin").display().to_string()];
            state.run_controls.upsert(config, &key).unwrap();
            state.run_controls.select(&id, &key);
            cx.notify();
        })
    });
    let mut renderer = images::VectorRenderer::default();
    let mut launches = Vec::new();
    click(window, "run-start");
    wait_for_file(
        &mut manager,
        &app,
        window,
        &mut renderer,
        &mut launches,
        &second.join("observed.txt"),
    );
    let result = std::fs::read_to_string(second.join("observed.txt")).unwrap();
    assert!(
        result.contains("hand edited") && result.ends_with("|second-private"),
        "{result}"
    );
    assert!(!result.contains("first-private"));
    manager.shutdown();
    // Publish actual retirement before testing a future launch; locating a live snapshot is allowed.
    publish_with_launches(&mut manager, &mut renderer, &app, window, &launches);
    // An invalid direct edit is visible in B1 and cannot execute the previous cached command.
    std::fs::write(editor_core::project_path(&second), b"{ broken").unwrap();
    window.update(|window, cx| app.update(cx, |state, cx| state.start_selected_run(window, cx)));
    pump_recording(&mut manager, &app, window, &mut launches);
    assert_eq!(launches.len(), 1);
    assert!(window.update(|_, cx| {
        app.read(cx)
            .run_controls
            .launch_plan(&id, &state_key(&second))
            .is_err()
    }));
}

/// Click a real painted local button, respecting its normal enabled and focus behavior.
fn click(cx: &mut gpui_kit::VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let center = cx.debug_bounds(selector).unwrap().center();
    cx.simulate_click(center, Default::default());
    cx.run_until_parked();
}
/// Poll the production publication seam until the actual program writes its observable values.
fn wait_for_file(
    manager: &mut plugin_runtime::Manager,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
    renderer: &mut images::VectorRenderer,
    launches: &mut Vec<(u64, String, u64)>,
    path: &std::path::Path,
) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !path.exists() {
        pump_recording(manager, app, cx, launches);
        manager.poll();
        publish_with_launches(manager, renderer, app, cx, launches);
        assert!(
            Instant::now() < deadline,
            "shared program did not run: {}; sessions={:?}; text={}",
            cx.update(|_, cx| app.read(cx).status.clone()),
            manager
                .executions()
                .iter()
                .map(|session| (session.id(), session.state(), session.snapshot().failure))
                .collect::<Vec<_>>(),
            painted_text(manager)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
/// Workspace keys use the editor's canonical path; no other project's settings are borrowed.
fn state_key(root: &std::path::Path) -> String {
    root.canonicalize().unwrap().display().to_string()
}
/// This native instrument records plain data; it never interprets argv as a script.
const PROBE: &str = "//! Shared configuration acceptance instrument.\nfn main(){let value=format!(\"{:?}|{}|{}\",std::env::args().skip(1).collect::<Vec<_>>(),std::env::current_dir().unwrap().display(),std::env::var(\"LOCAL_VALUE\").unwrap_or_default());std::fs::write(\"observed.txt\",value).unwrap();std::thread::sleep(std::time::Duration::from_secs(60));}\n";
