//! Verify host-managed language packages through real lifecycle calls.

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
    manifest.id = "rust".into();
    files.insert("manifest.json".into(), serde_json::to_vec(&manifest)?);
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        archive.start_file(name, zip::write::SimpleFileOptions::default())?;
        archive.write_all(&bytes)?;
    }
    let legacy = Package::from_bytes(&archive.finish()?.into_inner())?;
    manager.install(&legacy, Default::default())?;
    anyhow::ensure!(
        manager.live.contains_key("rust"),
        "legacy guest did not start"
    );
    for name in ["rust", "toml", "html", "javascript"] {
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
    // Uninstall one resource package to verify its registry entry disappears.
    manager.uninstall("toml", false)?;
    anyhow::ensure!(
        !manager.installed.contains_key("toml"),
        "uninstall did not remove package"
    );
    // Restart restores enabled resource packages from the registry without guest instances.
    drop(manager);
    let manager = Manager::open(root.path().to_owned(), Environment::default())?;
    anyhow::ensure!(
        manager.installed["rust"].enabled
            && manager.installed["html"].enabled
            && manager.installed["javascript"].enabled
            && !manager.installed.contains_key("toml"),
        "enabled packages were not restored"
    );
    anyhow::ensure!(
        manager.live.is_empty(),
        "restart launched a declarative guest"
    );
    // HTML must also uninstall cleanly after its enabled state survives a restart.
    let mut manager = manager;
    manager.uninstall("html", false)?;
    anyhow::ensure!(
        !manager.installed.contains_key("html"),
        "HTML uninstall did not remove package"
    );
    // JavaScript resources must also disappear cleanly after registry restoration.
    manager.uninstall("javascript", false)?;
    anyhow::ensure!(
        !manager.installed.contains_key("javascript"),
        "JavaScript uninstall did not remove package"
    );
    println!("PASS: declarative lifecycle, restart, and migration from a guest-backed package");
    Ok(())
}
