//! Exercise the real packaged guest, native PTY, snapshot restore and hot update.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, Event, Paint, ui::SideTabsPosition},
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
fn main() -> anyhow::Result<()> {
    let package_path = PathBuf::from(std::env::args().nth(1).expect("package path"));
    let package = Package::read(&package_path)?;
    let temp = tempfile::tempdir()?;
    let environment = Environment {
        workspace: std::env::current_dir()?.display().to_string(),
        os: std::env::consts::OS.into(),
        foreground: 0xffffff,
        ..Environment::default()
    };
    let mut manager = Manager::open(temp.path().to_owned(), environment.clone())?;
    // Missing grants must fail before a process can be created.
    assert!(manager.install(&package, Default::default()).is_err());
    manager.install(&package, package.manifest.permissions.clone())?;
    let id = &package.manifest.id;
    assert_eq!(manager.live[id].process_count(), 1);
    let controls = manager.live[id]
        .scene
        .as_ref()
        .unwrap()
        .controls
        .as_ref()
        .expect("terminal uses canvas controls");
    controls.validate().unwrap();
    let session = controls.sidebar.as_ref().unwrap().items[0].id.clone();
    manager.event(
        id,
        Event::Surface {
            panel: "terminal".into(),
            event: Box::new(Event::Ui(plugin_runtime::plugin_protocol::ui::UiEvent {
                revision: controls.revision,
                node: "sessions".into(),
                action: plugin_runtime::plugin_protocol::ui::Action::Rename {
                    id: session,
                    value: "Native UI smoke".into(),
                },
            })),
        },
    )?;
    assert_eq!(
        manager.live[id]
            .scene
            .as_ref()
            .unwrap()
            .controls
            .as_ref()
            .unwrap()
            .sidebar
            .as_ref()
            .unwrap()
            .items[0]
            .label,
        "Native UI smoke"
    );
    manager.event(
        id,
        Event::Text("Write-Output ('WASM_PLUGIN_'+'SMOKE')\r".into()),
    )?;
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut found = false;
    while Instant::now() < deadline {
        manager.poll();
        let scene = manager.live[id].scene.as_ref().unwrap();
        let text: String = scene
            .paint
            .iter()
            .filter_map(|p| {
                if let Paint::Text { text, .. } = p {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect();
        if text.contains("WASM_PLUGIN_SMOKE") {
            found = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    anyhow::ensure!(
        found,
        "No terminal output; error: {:?}",
        manager.live[id].error
    );
    // Configuration moves the live WASM layout without replacing its PTY or changing cells.
    let before = manager.live[id].scene.as_ref().unwrap();
    assert_eq!(
        before
            .controls
            .as_ref()
            .unwrap()
            .sidebar
            .as_ref()
            .unwrap()
            .position,
        SideTabsPosition::Right
    );
    let cursor = before.cursor;
    let width = before
        .controls
        .as_ref()
        .unwrap()
        .sidebar
        .as_ref()
        .unwrap()
        .width;
    let pids = manager.live[id].process_ids();
    let snapshot = manager.live.get_mut(id).unwrap().snapshot()?;
    let mut saved: serde_json::Value = serde_json::from_str(&snapshot.data)?;
    saved["settings"]["tab_position"] = "left".into();
    std::fs::write(
        manager.data_directory(id).join("settings.json"),
        serde_json::to_vec(&saved["settings"])?,
    )?;
    manager.invoke_command(id, "terminal.reload", serde_json::Value::Null)?;
    let left = manager.live[id].scene.as_ref().unwrap();
    assert_eq!(
        left.controls
            .as_ref()
            .unwrap()
            .sidebar
            .as_ref()
            .unwrap()
            .position,
        SideTabsPosition::Left
    );
    assert_eq!(left.cursor.x, cursor.x + width);
    assert_eq!(manager.live[id].process_ids(), pids);
    manager.checkpoint()?;
    let old_pids = manager.live[id].process_ids();
    let mut invalid = package.clone();
    invalid.files.insert(
        invalid
            .manifest
            .component
            .clone()
            .expect("terminal component"),
        b"invalid WASM".to_vec(),
    );
    assert!(
        manager
            .install(&invalid, invalid.manifest.permissions.clone())
            .is_err()
    );
    assert_eq!(
        manager.live[id].process_ids(),
        old_pids,
        "failed validation must leave the old programs untouched"
    );
    manager.install(&package, package.manifest.permissions.clone())?;
    assert_eq!(manager.live[id].process_count(), 1);
    assert_ne!(
        manager.live[id].process_ids(),
        old_pids,
        "successful cutover starts a fresh shell"
    );
    let snapshot = manager.live.get_mut(id).unwrap().snapshot()?;
    assert!(snapshot.data.contains("WASM_PLUGIN_SMOKE"));
    assert!(!snapshot.data.contains("restored session; new shell"));
    // Fresh-shell startup output must not erase the restored session after cutover.
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        manager.poll();
        std::thread::sleep(Duration::from_millis(30));
    }
    let snapshot = manager.live.get_mut(id).unwrap().snapshot()?;
    assert!(snapshot.data.contains("WASM_PLUGIN_SMOKE"));
    assert!(!snapshot.data.contains("restored session; new shell"));
    manager.disable(id)?;
    assert!(!manager.live.contains_key(id));
    // Host calls cannot silently enable a stopped plugin.
    assert!(
        manager
            .invoke_command(
                id,
                "terminal.new",
                serde_json::json!({ "name": "拒绝创建" })
            )
            .is_err()
    );
    manager.enable(id)?;
    drop(manager);
    let mut manager = Manager::open(temp.path().to_owned(), environment)?;
    assert!(manager.live.contains_key(id));
    // Hot update and editor restart both retain the plugin's declared sidebar edge.
    assert_eq!(
        manager.live[id]
            .scene
            .as_ref()
            .unwrap()
            .controls
            .as_ref()
            .unwrap()
            .sidebar
            .as_ref()
            .unwrap()
            .position,
        SideTabsPosition::Left
    );
    // The real component receives host parameters while ordinary terminal labels stay unnumbered.
    let pids = manager.live[id].process_ids();
    assert!(
        manager
            .invoke_command(id, "undeclared", serde_json::Value::Null)
            .is_err()
    );
    assert!(
        manager
            .invoke_command(
                id,
                "terminal.new",
                serde_json::json!({ "name": "x".repeat(65536) })
            )
            .is_err()
    );
    assert_eq!(manager.live[id].process_ids(), pids);
    manager.invoke_command(
        id,
        "terminal.new",
        serde_json::json!({ "name": "宿主新建" }),
    )?;
    assert_eq!(
        manager.live[id]
            .scene
            .as_ref()
            .unwrap()
            .controls
            .as_ref()
            .unwrap()
            .sidebar
            .as_ref()
            .unwrap()
            .items
            .last()
            .unwrap()
            .label,
        "宿主新建"
    );
    manager.invoke_command(
        id,
        "terminal.run",
        serde_json::json!({
            "name": "宿主运行", "command": "Write-Output ('HOST_TASK_'+'SMOKE')",
        }),
    )?;
    assert!(
        manager
            .live
            .get_mut(id)
            .unwrap()
            .effects()
            .iter()
            .any(|effect| matches!(effect,
                plugin_runtime::plugin_protocol::Request::Editor { command } if command == "save"
            ))
    );
    // The editor normally supplies this callback after saving its current file.
    manager.event(
        id,
        Event::Command {
            id: "save.result".into(),
            cwd: None,
            text: None,
            arguments: None,
        },
    )?;
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut found = false;
    while Instant::now() < deadline {
        manager.poll();
        let text: String = manager.live[id]
            .scene
            .as_ref()
            .unwrap()
            .paint
            .iter()
            .filter_map(|paint| {
                if let Paint::Text { text, .. } = paint {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect();
        if text.contains("HOST_TASK_SMOKE") {
            found = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    anyhow::ensure!(
        found,
        "named host task did not produce output: {:?}",
        manager.live[id].error
    );
    assert_eq!(
        manager.live[id]
            .scene
            .as_ref()
            .unwrap()
            .controls
            .as_ref()
            .unwrap()
            .sidebar
            .as_ref()
            .unwrap()
            .items
            .last()
            .unwrap()
            .label,
        "宿主运行"
    );
    let snapshot = manager.live.get_mut(id).unwrap().snapshot()?;
    assert!(snapshot.data.contains("宿主运行"));
    // Closing all packaged guest sessions emits one panel-hide request after the final PTY closes.
    let sessions = manager.live[id].process_count();
    manager.live.get_mut(id).unwrap().effects();
    for remaining in (0..sessions).rev() {
        manager.invoke_command(id, "terminal.close", serde_json::Value::Null)?;
        assert_eq!(manager.live[id].process_count(), remaining);
        let effects = manager.live.get_mut(id).unwrap().effects();
        assert_eq!(
            effects
                .iter()
                .filter(|effect| matches!(effect,
                    plugin_runtime::plugin_protocol::Request::Editor { command }
                        if command == "hide_panel:terminal"
                ))
                .count(),
            usize::from(remaining == 0),
        );
    }
    assert!(
        manager.live[id]
            .scene
            .as_ref()
            .unwrap()
            .controls
            .as_ref()
            .unwrap()
            .sidebar
            .as_ref()
            .unwrap()
            .items
            .is_empty()
    );
    // Native panel reopening is a scoped lifecycle event and creates exactly one fresh shell.
    for _ in 0..2 {
        manager.event(
            id,
            Event::Surface {
                panel: "terminal".into(),
                event: Box::new(Event::Command {
                    id: "panel.opened".into(),
                    cwd: None,
                    text: None,
                    arguments: None,
                }),
            },
        )?;
        assert_eq!(manager.live[id].process_count(), 1);
        assert_eq!(
            manager.live[id]
                .scene
                .as_ref()
                .unwrap()
                .controls
                .as_ref()
                .unwrap()
                .sidebar
                .as_ref()
                .unwrap()
                .items
                .len(),
            1,
        );
    }
    manager.uninstall(id, true)?;
    assert!(!manager.data_directory(id).exists());
    // An unrelated, permissionless package declares and renders two different native surfaces.
    // Test the companion package from the same output directory as the supplied terminal ZIP.
    let example = Package::read(&package_path.with_file_name("example.zip"))?;
    manager.install(&example, Default::default())?;
    assert_eq!(manager.live["example"].scenes.len(), 2);
    manager.event(
        "example",
        Event::Command {
            id: "increment".into(),
            cwd: None,
            text: None,
            arguments: None,
        },
    )?;
    assert!(
        manager
            .live
            .get_mut("example")
            .unwrap()
            .snapshot()?
            .data
            .starts_with("[1,")
    );
    // A valid component can fail only at activation; the old package and its state must return.
    let mut files = example.files.clone();
    let mut manifest = example.manifest.clone();
    manifest.version = "99.0.0".into();
    files.insert("manifest.json".into(), serde_json::to_vec(&manifest)?);
    files.insert("activation-policy.txt".into(), b"reject".to_vec());
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in files {
        archive.start_file(name, zip::write::SimpleFileOptions::default())?;
        std::io::Write::write_all(&mut archive, &bytes)?;
    }
    let failing = Package::from_bytes(&archive.finish()?.into_inner())?;
    assert!(manager.install(&failing, Default::default()).is_err());
    assert_eq!(
        manager.installed["example"].manifest.version,
        example.manifest.version
    );
    assert!(
        manager
            .live
            .get_mut("example")
            .unwrap()
            .snapshot()?
            .data
            .starts_with("[1,")
    );
    let mut escalated = example.clone();
    escalated.manifest.permissions.insert("clipboard".into());
    assert!(manager.install(&escalated, Default::default()).is_err());
    manager.uninstall("example", false)?;
    assert!(manager.data_directory("example").exists());
    manager.install(&example, Default::default())?;
    assert!(
        manager
            .live
            .get_mut("example")
            .unwrap()
            .snapshot()?
            .data
            .starts_with("[1,")
    );
    println!(
        "PASS: package, permissions, WASM VT parser, ConPTY, hot update, restart restore, uninstall"
    );
    Ok(())
}
