//! Smoke any current SDK package through public installation and lifecycle boundaries.
use plugin_runtime::{Manager, Package, plugin_protocol::Environment};
use std::path::Path;

/// Temporary roots keep this diagnostic independent of real editor installations and preferences.
fn main() -> anyhow::Result<()> {
    let paths = std::env::args().skip(1).collect::<Vec<_>>();
    anyhow::ensure!(!paths.is_empty(), "Supply one or more plugin ZIP paths");
    let root = tempfile::tempdir()?;
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            os: std::env::consts::OS.into(),
            ..Default::default()
        },
    )?;
    for path in paths {
        let package = Package::read(Path::new(&path))?;
        let id = &package.manifest.id;
        manager.install(&package, package.manifest.permissions.clone())?;
        if let Some(instance) = manager.live.get(id) {
            for document in instance.views.values() {
                document.validate().map_err(anyhow::Error::msg)?;
            }
        }
        manager.checkpoint()?;
        manager.disable(id)?;
        manager.enable(id)?;
        manager.uninstall(id, true)?;
        anyhow::ensure!(
            !manager.live.contains_key(id),
            "Uninstall retained an instance"
        );
        println!(
            "PASS: {} {} install, checkpoint, reactivation and cleanup",
            id, package.manifest.version
        );
    }
    Ok(())
}
