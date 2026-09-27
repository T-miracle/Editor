//! Update an already installed local package with its existing permission grants.
use plugin_runtime::{Manager, Package, plugin_protocol::Environment};
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    let root = PathBuf::from(args.get(1).expect("installed plugin root"));
    let package = Package::read(&PathBuf::from(args.get(2).expect("plugin ZIP")))?;
    let workspace = PathBuf::from(args.get(3).expect("workspace path"));
    let existing = Manager::read_registry(&root)?
        .remove(&package.manifest.id)
        .ok_or_else(|| anyhow::anyhow!("The plugin is not installed"))?;
    // Never grant new capabilities through this maintenance command.
    anyhow::ensure!(
        package.manifest.permissions.is_subset(&existing.grants),
        "Updated package requests additional permissions"
    );
    anyhow::ensure!(existing.enabled, "The installed plugin is disabled");
    let mut manager = Manager::open(
        root,
        Environment {
            workspace: workspace.display().to_string(),
            os: std::env::consts::OS.into(),
            ..Environment::default()
        },
    )?;
    // The runtime validates and prepares the new WASM before replacing the old version.
    manager.install(&package, existing.grants)?;
    println!(
        "Updated {} to {}",
        package.manifest.id, package.manifest.version
    );
    Ok(())
}
