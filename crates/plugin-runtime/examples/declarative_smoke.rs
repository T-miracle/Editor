//! Verify host-managed Rust, TOML, and theme packages through real lifecycle calls.

use plugin_runtime::{Manager, Package, plugin_protocol::Environment};
use std::{
    io::{Cursor, Write},
    path::PathBuf,
};

fn main() -> anyhow::Result<()> {
    let packages = PathBuf::from(std::env::args().nth(1).expect("plugin package directory"));
    let root = tempfile::tempdir()?;
    let mut manager = Manager::open(root.path().to_owned(), Environment::default())?;
    // Repackage the sample guest under Rust's ID to exercise migration from older guest-backed installs.
    let legacy = Package::read(&packages.join("example.zip"))?;
    let mut files = legacy.files;
    let mut manifest = legacy.manifest;
    manifest.id = "me.rust".into();
    files.insert("manifest.json".into(), serde_json::to_vec(&manifest)?);
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        archive.start_file(name, zip::write::SimpleFileOptions::default())?;
        archive.write_all(&bytes)?;
    }
    let legacy = Package::from_bytes(&archive.finish()?.into_inner())?;
    manager.install(&legacy, Default::default())?;
    anyhow::ensure!(
        manager.live.contains_key("me.rust"),
        "legacy guest did not start"
    );
    for name in ["rust", "toml", "default-light-theme"] {
        let package = Package::read(&packages.join(format!("{name}.zip")))?;
        // Declarative packages install without a guest instance; the host owns their state.
        manager.install(&package, Default::default())?;
        anyhow::ensure!(
            package.manifest.component.is_none()
                && !manager.live.contains_key(&package.manifest.id),
            "declarative package started a guest instance"
        );
        let manifest = package
            .manifest
            .contributions
            .as_deref()
            .expect("declarative manifest");
        anyhow::ensure!(
            root.path()
                .join("packages")
                .join(&package.manifest.id)
                .join(&package.digest)
                .join(manifest)
                .is_file(),
            "declarative assets were not installed"
        );
        manager.disable(&package.manifest.id)?;
        anyhow::ensure!(
            !manager.installed[&package.manifest.id].enabled,
            "disable did not persist"
        );
        manager.enable(&package.manifest.id)?;
    }
    manager.uninstall("me.default-light-theme", false)?;
    anyhow::ensure!(
        !manager.installed.contains_key("me.default-light-theme"),
        "uninstall did not remove package"
    );
    // Restart restores enabled resource packages from the registry without guest instances.
    drop(manager);
    let manager = Manager::open(root.path().to_owned(), Environment::default())?;
    anyhow::ensure!(
        manager.installed["me.rust"].enabled && manager.installed["me.toml"].enabled,
        "enabled packages were not restored"
    );
    anyhow::ensure!(
        manager.live.is_empty(),
        "restart launched a declarative guest"
    );
    println!("PASS: declarative lifecycle, restart, and migration from a guest-backed package");
    Ok(())
}
