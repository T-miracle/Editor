//! Exercise the exported UI contract against a real independently compiled WASM package.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{
        Environment, Event,
        ui::{Action, UiEvent},
    },
};

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("path to example.zip");
    let package = Package::read(std::path::Path::new(&path))?;
    let directory = tempfile::tempdir()?;
    let mut manager = Manager::open(directory.path().into(), Environment::default())?;
    manager.install(&package, Default::default())?;
    let send = |manager: &mut Manager, panel: &str, node: &str, action| {
        manager.event(
            "example",
            Event::Surface {
                panel: panel.into(),
                event: Box::new(Event::Ui(UiEvent {
                    revision: 0,
                    node: node.into(),
                    action,
                })),
            },
        )
    };
    send(&mut manager, "counter", "increment", Action::Click)?;
    send(
        &mut manager,
        "notes",
        "note",
        Action::Change("中文笔记".into()),
    )?;
    send(&mut manager, "counter", "open-dialog", Action::Click)?;
    assert!(
        manager.live["example"].scenes["counter"]
            .ui
            .as_ref()
            .unwrap()
            .dialog
            .is_some()
    );
    send(&mut manager, "counter", "sample-dialog", Action::Dismiss)?;
    assert!(
        manager.live["example"].scenes["counter"]
            .ui
            .as_ref()
            .unwrap()
            .dialog
            .is_none()
    );
    for scene in manager.live["example"].scenes.values() {
        scene.ui.as_ref().unwrap().validate().unwrap();
    }
    let snapshot = manager.live.get_mut("example").unwrap().snapshot()?;
    assert_eq!(snapshot.data, "[1,\"中文笔记\"]");
    // A guest cannot silently opt into native UI while declaring legacy compatibility.
    let mut legacy = package.clone();
    legacy.manifest.protocol = 1;
    assert!(manager.install(&legacy, Default::default()).is_err());
    assert_eq!(manager.installed["example"].manifest.protocol, 2);
    assert_eq!(
        manager.live.get_mut("example").unwrap().snapshot()?.data,
        snapshot.data
    );
    manager.install(&package, Default::default())?;
    assert_eq!(
        manager.live.get_mut("example").unwrap().snapshot()?.data,
        snapshot.data
    );
    println!(
        "PASS: UI scenes, typed events, modal lifecycle, compatibility rejection and update restore"
    );
    Ok(())
}
