//! Exercise the real packaged guest, native PTY, snapshot restore and hot update.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, Event, Paint},
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
    manager.disable(id)?;
    assert!(!manager.live.contains_key(id));
    manager.enable(id)?;
    drop(manager);
    let mut manager = Manager::open(temp.path().to_owned(), environment)?;
    assert!(manager.live.contains_key(id));
    manager.uninstall(id, true)?;
    assert!(!manager.data_directory(id).exists());
    // An unrelated, permissionless package declares and renders two different native surfaces.
    // Test the companion package from the same output directory as the supplied terminal ZIP.
    let example = Package::read(&package_path.with_file_name("example.zip"))?;
    manager.install(&example, Default::default())?;
    assert_eq!(manager.live["me.example"].scenes.len(), 2);
    manager.event(
        "me.example",
        Event::Command {
            id: "increment".into(),
            cwd: None,
            text: None,
        },
    )?;
    assert!(
        manager
            .live
            .get_mut("me.example")
            .unwrap()
            .snapshot()?
            .data
            .starts_with("[1,")
    );
    // A valid component can fail only at activation; the old package and its state must return.
    let mut files = example.files.clone();
    let mut manifest = example.manifest.clone();
    manifest.version = "0.1.1".into();
    files.insert("manifest.json".into(), serde_json::to_vec(&manifest)?);
    files.insert("activation-policy.txt".into(), b"reject".to_vec());
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in files {
        archive.start_file(name, zip::write::SimpleFileOptions::default())?;
        std::io::Write::write_all(&mut archive, &bytes)?;
    }
    let failing = Package::from_bytes(&archive.finish()?.into_inner())?;
    assert!(manager.install(&failing, Default::default()).is_err());
    assert_eq!(manager.installed["me.example"].manifest.version, "0.1.0");
    assert!(
        manager
            .live
            .get_mut("me.example")
            .unwrap()
            .snapshot()?
            .data
            .starts_with("[1,")
    );
    let mut escalated = example.clone();
    escalated.manifest.permissions.insert("clipboard".into());
    assert!(manager.install(&escalated, Default::default()).is_err());
    manager.uninstall("me.example", false)?;
    assert!(manager.data_directory("me.example").exists());
    manager.install(&example, Default::default())?;
    assert!(
        manager
            .live
            .get_mut("me.example")
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
