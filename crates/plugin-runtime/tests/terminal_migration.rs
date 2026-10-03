//! The delivered terminal is exercised as an ordinary installed WASM package, never through a host shortcut.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{
        Environment, Event, api,
        ui::{self, Kind},
    },
};
use std::path::Path;

/// Current admission, composed native UI and owned ConPTY lifetime form one public delivery boundary.
#[test]
#[cfg(windows)]
#[ignore = "build the terminal package through the exported SDK first"]
fn installed_terminal_uses_current_capabilities_and_reclaims_processes() {
    let package = Package::read(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/terminal.zip"),
    )
    .expect("the delivered terminal must pass current package admission");
    assert_eq!(package.manifest.protocol, 7);
    assert!(package.manifest.permissions.contains("process.exec"));
    assert!(!package.manifest.permissions.contains("editor.commands"));
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            os: std::env::consts::OS.into(),
            workspace: root.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let instance = &manager.live["terminal"];
    assert_eq!(
        instance.process_count(),
        1,
        "published view: {:?}",
        instance.scene
    );
    let document = instance
        .scene
        .as_ref()
        .unwrap()
        .ui
        .as_ref()
        .expect("terminal publishes a composed native document");
    assert!(matches!(document.root.kind, Kind::Row { .. }));
    let mut canvases = 0;
    document.root.visit(&mut |node| {
        if let Kind::Canvas(canvas) = &node.kind {
            assert!(canvas.focusable && canvas.grid);
            canvases += 1;
        }
    });
    assert_eq!(canvases, 1);
    let revision = document.revision;
    // A native keystroke can publish both Key and committed Text in one displayed frame.
    for action in [
        ui::CanvasEvent::Key {
            key: "x".into(),
            ctrl: false,
            alt: false,
            shift: false,
        },
        ui::CanvasEvent::Text { text: " ".into() },
    ] {
        manager
            .event(
                "terminal",
                Event::Surface {
                    panel: "terminal".into(),
                    event: Box::new(Event::Ui(ui::UiEvent {
                        revision,
                        node: "output".into(),
                        action: ui::Action::Canvas(action),
                    })),
                },
            )
            .expect("same-frame input must remain accepted");
    }
    // PTY output interleaves with later keys without invalidating their displayed target.
    manager.poll();
    input(&mut manager,revision,ui::CanvasEvent::Text{text:"[IO.File]::WriteAllText((Join-Path (Get-Location) 'input.txt'), 'hello world 中文')".into()});
    enter(&mut manager);
    wait_for(&mut manager, |_| root.path().join("input.txt").exists());
    assert_eq!(
        std::fs::read_to_string(root.path().join("input.txt")).unwrap(),
        "hello world 中文"
    );

    let original = session_ids(&manager)[0].clone();
    input_current(
        &mut manager,
        ui::CanvasEvent::Key {
            key: "v".into(),
            ctrl: true,
            shift: false,
            alt: false,
        },
    );
    let paste = manager
        .live
        .get_mut("terminal")
        .unwrap()
        .take_editor_requests()
        .pop()
        .unwrap();
    assert!(matches!(
        paste.operation(),
        api::EditorOperation::ReadClipboard
    ));
    assert!(paste.begin());
    let other = root.path().join("other");
    std::fs::create_dir(&other).unwrap();
    manager
        .invoke_command(
            "terminal",
            "terminal.new",
            serde_json::json!({"name":"Other","cwd":other}),
        )
        .unwrap();
    paste.finish(Ok(api::EditorValue::Clipboard {
        text: "[IO.File]::WriteAllText((Join-Path (Get-Location) 'paste.txt'), 'original')".into(),
    }));
    manager.poll();
    action(&mut manager, "sessions", ui::Action::Select(original));
    enter(&mut manager);
    wait_for(&mut manager, |_| root.path().join("paste.txt").exists());
    assert!(
        !other.join("paste.txt").exists(),
        "pending paste must stay with its original shell"
    );

    // Failed saves cannot launch a command or a new process; acceptance is insufficient.
    manager
        .invoke_command(
            "terminal",
            "terminal.run",
            serde_json::json!({"command":"echo SHOULD_NOT_RUN"}),
        )
        .unwrap();
    let request = manager
        .live
        .get_mut("terminal")
        .unwrap()
        .take_editor_requests()
        .pop()
        .unwrap();
    request.finish(Err(api::Failure::new(
        api::ErrorCode::Conflict,
        "test save rejected",
    )));
    manager.poll();
    assert_eq!(manager.live["terminal"].process_count(), 2);
    let second = session_ids(&manager)[1].clone();
    action(&mut manager, "sessions", ui::Action::Close(second));

    // Migration validates staged settings and restores the old running version on failure.
    let settings = manager.data_directory("terminal").join("settings.json");
    std::fs::write(&settings, "{corrupt").unwrap();
    let replacement = replacement(&package);
    assert!(
        manager
            .install(&replacement, replacement.manifest.permissions.clone())
            .is_err()
    );
    assert_eq!(
        manager.installed["terminal"].manifest.version,
        package.manifest.version
    );
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), "{corrupt");
    assert_eq!(manager.live["terminal"].process_count(), 1);
    std::fs::write(&settings, "{\"history\":4321,\"default_profile\":1}").unwrap();
    let sessions = session_ids(&manager);
    manager
        .install(&replacement, replacement.manifest.permissions.clone())
        .unwrap();
    assert_eq!(session_ids(&manager), sessions);
    assert_eq!(
        std::fs::read_to_string(&settings).unwrap(),
        "{\"history\":4321,\"default_profile\":1}"
    );
    assert_eq!(manager.live["terminal"].process_count(), 1);

    // Ended sessions are saved as history; reactivation must not start their programs again.
    input_current(
        &mut manager,
        ui::CanvasEvent::Text {
            text: "exit".into(),
        },
    );
    enter(&mut manager);
    wait_for(&mut manager, |manager| {
        manager.live["terminal"].process_count() == 0
    });
    manager.disable("terminal").unwrap();
    assert!(!manager.live.contains_key("terminal"));
    manager.enable("terminal").unwrap();
    assert_eq!(manager.live["terminal"].process_count(), 0);
    manager
        .invoke_command("terminal", "terminal.new", serde_json::Value::Null)
        .unwrap();
    assert_eq!(manager.live["terminal"].process_count(), 1);
    manager.uninstall("terminal", false).unwrap();
    assert!(!manager.live.contains_key("terminal"));
}

/// Repackage the real component with a newer declaration to exercise its migration hook.
fn replacement(package: &Package) -> Package {
    use std::io::{Cursor, Write};
    let mut files = package.files.clone();
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    // Keep exercising an update as the delivered terminal advances through later migrations.
    let mut version: semver::Version = package.manifest.version.parse().unwrap();
    version.patch += 1;
    manifest["version"] = serde_json::json!(version.to_string());
    manifest["data_format"]["version"] = serde_json::json!(2);
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        archive
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&archive.finish().unwrap().into_inner()).unwrap()
}

/// Tests send the same addressed native events that GPUI publishes to the manager.
fn input(manager: &mut Manager, revision: u64, event: ui::CanvasEvent) {
    manager
        .event(
            "terminal",
            Event::Surface {
                panel: "terminal".into(),
                event: Box::new(Event::Ui(ui::UiEvent {
                    revision,
                    node: "output".into(),
                    action: ui::Action::Canvas(event),
                })),
            },
        )
        .unwrap();
}
fn input_current(manager: &mut Manager, event: ui::CanvasEvent) {
    let revision = document(manager).revision;
    input(manager, revision, event);
}
fn enter(manager: &mut Manager) {
    input_current(
        manager,
        ui::CanvasEvent::Key {
            key: "enter".into(),
            ctrl: false,
            alt: false,
            shift: false,
        },
    );
}
fn action(manager: &mut Manager, node: &str, action: ui::Action) {
    let revision = document(manager).revision;
    manager
        .event(
            "terminal",
            Event::Surface {
                panel: "terminal".into(),
                event: Box::new(Event::Ui(ui::UiEvent {
                    revision,
                    node: node.into(),
                    action,
                })),
            },
        )
        .unwrap();
}
fn document(manager: &Manager) -> &ui::Document {
    manager.live["terminal"]
        .scene
        .as_ref()
        .unwrap()
        .ui
        .as_ref()
        .unwrap()
}
fn session_ids(manager: &Manager) -> Vec<String> {
    let mut ids = Vec::new();
    document(manager).root.visit(&mut |node| {
        if let Kind::SideTabs(tabs) = &node.kind {
            ids = tabs.items.iter().map(|tab| tab.id.clone()).collect();
        }
    });
    ids
}
/// Native processes are asynchronous; wait on observable output with a bounded real deadline.
fn wait_for(manager: &mut Manager, ready: impl Fn(&Manager) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !ready(manager) {
        manager.poll();
        assert!(
            std::time::Instant::now() < deadline,
            "native terminal did not finish: {:?}",
            manager.installed["terminal"].error
        );
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}
