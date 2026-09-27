//! Package lifecycle owns rollback, opaque snapshots and resource retirement.
use super::{Instance, Package, package::atomic_write};
use plugin_protocol::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Installed {
    pub manifest: Manifest,
    pub digest: String,
    pub grants: BTreeSet<String>,
    pub enabled: bool,
    #[serde(skip)]
    pub error: Option<String>,
}
impl Installed {
    /// Resolve only a declared SVG from this installed package version.
    pub fn panel_icon(&self, root: &Path, panel_id: &str, dark: bool) -> Option<Vec<u8>> {
        let panel = self
            .manifest
            .panels
            .iter()
            .find(|panel| panel.id == panel_id)?;
        let path = if dark {
            panel.icon_dark.as_ref().or(panel.icon_light.as_ref())
        } else {
            panel.icon_light.as_ref().or(panel.icon_dark.as_ref())
        }?;
        if !self.manifest.id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-'
        }) || self.digest.len() != 64
            || !self.digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !Path::new(path)
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_)))
        {
            return None;
        }
        let bytes = std::fs::read(
            root.join("packages")
                .join(&self.manifest.id)
                .join(&self.digest)
                .join(path),
        )
        .ok()?;
        (bytes.len() <= 64 * 1024
            && std::str::from_utf8(&bytes)
                .ok()?
                .trim_start()
                .starts_with("<svg"))
        .then_some(bytes)
    }
}
/// Run this module on a worker thread; native rendering reads only published scenes.
pub struct Manager {
    root: PathBuf,
    environment: Environment,
    engine: Option<wasmtime::Engine>,
    pub installed: BTreeMap<String, Installed>,
    pub live: BTreeMap<String, Instance>,
}
impl Manager {
    /// Read installed plugin metadata without starting their WASM components.
    pub fn read_registry(root: &Path) -> anyhow::Result<BTreeMap<String, Installed>> {
        let installed = match std::fs::read(root.join("registry.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(e.into()),
        };
        Ok(installed)
    }
    pub fn open(root: PathBuf, environment: Environment) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&root)?;
        let installed = Self::read_registry(&root)?;
        let mut manager = Self {
            root,
            environment,
            // Resource-only packages need no Wasmtime engine during startup.
            engine: None,
            installed,
            live: BTreeMap::new(),
        };
        let ids: Vec<_> = manager
            .installed
            .iter()
            .filter(|(_, p)| p.enabled)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            if let Err(error) = manager.enable(&id) {
                manager.installed.get_mut(&id).unwrap().error = Some(format!("{error:#}"));
            }
        }
        Ok(manager)
    }
    pub fn data_directory(&self, id: &str) -> PathBuf {
        self.root.join("data").join(id)
    }
    fn snapshot_path(&self, id: &str) -> PathBuf {
        let workspace = format!(
            "{:x}",
            Sha256::digest(self.environment.workspace.as_bytes())
        );
        self.data_directory(id)
            .join(format!("state-{}.json", &workspace[..16]))
    }
    fn save_registry(&self) -> anyhow::Result<()> {
        atomic_write(
            &self.root.join("registry.json"),
            &serde_json::to_vec_pretty(&self.installed)?,
        )
    }
    fn load_snapshot(&self, id: &str) -> anyhow::Result<Option<Snapshot>> {
        let path = self.snapshot_path(id);
        if !path.exists() {
            return Ok(None);
        }
        anyhow::ensure!(
            path.metadata()?.len() <= 40 * 1024 * 1024,
            "Saved state exceeds host quota"
        );
        Ok(Some(serde_json::from_slice(&std::fs::read(path)?)?))
    }
    fn save_snapshot(&self, id: &str, snapshot: &Snapshot) -> anyhow::Result<()> {
        let limit = self
            .installed
            .get(id)
            .map(|p| p.manifest.storage_limit)
            .unwrap_or(32 * 1024 * 1024);
        anyhow::ensure!(
            snapshot.data.len() <= limit,
            "Plugin snapshot exceeds declared quota"
        );
        atomic_write(&self.snapshot_path(id), &serde_json::to_vec(snapshot)?)
    }
    /// Installation requires the caller's explicit grants; missing new permissions never inherit.
    pub fn install(&mut self, package: &Package, grants: BTreeSet<String>) -> anyhow::Result<()> {
        let id = package.manifest.id.clone();
        anyhow::ensure!(
            package.manifest.permissions.is_subset(&grants),
            "Permission confirmation required"
        );
        if package.manifest.component.is_none() {
            return self.install_declarative(package, grants);
        }
        let snapshot = if let Some(old) = self.live.get_mut(&id) {
            Some(old.snapshot()?)
        } else {
            self.load_snapshot(&id)?
        };
        let version = self.root.join("packages").join(&id).join(&package.digest);
        package.extract(&version)?;
        if self.engine.is_none() {
            self.engine = Some(Instance::engine()?);
        }
        let mut next = Instance::prepare(
            self.engine.as_ref().unwrap(),
            package.component().expect("component checked above"),
            &package.manifest,
            &grants,
            self.environment.clone(),
            self.data_directory(&id),
            version,
            snapshot.clone(),
        )?;
        if let Some(snapshot) = &snapshot {
            self.save_snapshot(&id, snapshot)?;
        }
        // Cutover stops old process trees. Rollback starts new shells from the old snapshot.
        let mut old = self.live.remove(&id);
        if let Some(old) = &mut old {
            old.stop();
        }
        let previous = self.installed.get(&id).cloned();
        let mut original_data = vec![];
        let result = (|| {
            next.activate()?;
            original_data = next.commit_data()?;
            self.installed.insert(
                id.clone(),
                Installed {
                    manifest: package.manifest.clone(),
                    digest: package.digest.clone(),
                    grants,
                    enabled: true,
                    error: None,
                },
            );
            self.save_registry()?;
            Ok::<_, anyhow::Error>(())
        })();
        match result {
            Ok(()) => {
                self.live.insert(id, next);
                Ok(())
            }
            Err(error) => {
                next.stop();
                Instance::rollback_data(&original_data)?;
                if let Some(previous) = previous {
                    self.installed.insert(id.clone(), previous);
                    // Reuse the old compiled instance; a registry I/O failure must not prevent recovery.
                    if let Some(mut old) = old {
                        let rollback = old
                            .call(Message::Prepare {
                                environment: self.environment.clone(),
                                snapshot,
                            })
                            .and_then(|_| old.activate());
                        if let Err(rollback) = rollback {
                            return Err(anyhow::anyhow!(
                                "Update failed: {error:#}; rollback failed: {rollback:#}"
                            ));
                        }
                        self.live.insert(id.clone(), old);
                    }
                } else {
                    self.installed.remove(&id);
                }
                Err(error)
            }
        }
    }
    /// Commit a resource-only package without starting a redundant WASM instance.
    fn install_declarative(
        &mut self,
        package: &Package,
        grants: BTreeSet<String>,
    ) -> anyhow::Result<()> {
        let id = package.manifest.id.clone();
        if let Some(old) = self.live.get_mut(&id) {
            let snapshot = old.snapshot()?;
            self.save_snapshot(&id, &snapshot)?;
        }
        let version = self.root.join("packages").join(&id).join(&package.digest);
        package.extract(&version)?;
        let previous = self.installed.insert(
            id.clone(),
            Installed {
                manifest: package.manifest.clone(),
                digest: package.digest.clone(),
                grants,
                enabled: true,
                error: None,
            },
        );
        if let Err(error) = self.save_registry() {
            if let Some(previous) = previous {
                self.installed.insert(id, previous);
            } else {
                self.installed.remove(&id);
            }
            return Err(error);
        }
        // Cut over only after the new registry is durable, so a failed write leaves the old guest alive.
        if let Some(mut old) = self.live.remove(&id) {
            old.stop();
        }
        Ok(())
    }
    /// Re-enable from the last committed version and plugin-owned snapshot.
    pub fn enable(&mut self, id: &str) -> anyhow::Result<()> {
        if self.live.contains_key(id) {
            return Ok(());
        }
        let entry = self
            .installed
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("Unknown plugin"))?
            .clone();
        let Some(component_path) = &entry.manifest.component else {
            let contribution_path = entry
                .manifest
                .contributions
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Declarative plugin has no contributions"))?;
            anyhow::ensure!(
                Path::new(contribution_path)
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_))),
                "Invalid installed contribution path"
            );
            anyhow::ensure!(
                self.root
                    .join("packages")
                    .join(id)
                    .join(&entry.digest)
                    .join(contribution_path)
                    .is_file(),
                "Installed contribution manifest is missing"
            );
            if !entry.enabled || entry.error.is_some() {
                let installed = self.installed.get_mut(id).unwrap();
                installed.enabled = true;
                installed.error = None;
                if let Err(error) = self.save_registry() {
                    self.installed.insert(id.to_owned(), entry);
                    return Err(error);
                }
            }
            return Ok(());
        };
        let component = std::fs::read(
            self.root
                .join("packages")
                .join(id)
                .join(&entry.digest)
                .join(component_path),
        )?;
        if self.engine.is_none() {
            self.engine = Some(Instance::engine()?);
        }
        let mut instance = Instance::prepare(
            self.engine.as_ref().unwrap(),
            &component,
            &entry.manifest,
            &entry.grants,
            self.environment.clone(),
            self.data_directory(id),
            self.root.join("packages").join(id).join(&entry.digest),
            self.load_snapshot(id)?,
        )?;
        instance.activate()?;
        let originals = instance.commit_data()?;
        self.installed.get_mut(id).unwrap().enabled = true;
        self.installed.get_mut(id).unwrap().error = None;
        if let Err(error) = self.save_registry() {
            self.installed.insert(id.into(), entry);
            Instance::rollback_data(&originals)?;
            return Err(error);
        }
        self.live.insert(id.to_owned(), instance);
        Ok(())
    }
    /// UI confirmation occurs before calling this when process_count is nonzero.
    pub fn disable(&mut self, id: &str) -> anyhow::Result<()> {
        let snapshot = self.live.get_mut(id).map(Instance::snapshot);
        let saved = match snapshot {
            Some(Ok(snapshot)) => self.save_snapshot(id, &snapshot),
            Some(Err(error)) => Err(error),
            None => Ok(()),
        };
        // A broken guest must still be removable; retain the last good snapshot if it traps.
        if let Err(error) = saved {
            if let Some(entry) = self.installed.get_mut(id) {
                entry.error = Some(format!("保留上次保存的数据：{error:#}"));
            }
        }
        self.live.remove(id);
        self.installed
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("Unknown plugin"))?
            .enabled = false;
        self.save_registry()
    }
    /// Uninstall keeps state unless the user explicitly chose deletion in the manager UI.
    pub fn uninstall(&mut self, id: &str, delete_data: bool) -> anyhow::Result<()> {
        self.disable(id)?;
        self.installed.remove(id);
        self.save_registry()?;
        let package_directory = self.root.join("packages").join(id);
        if package_directory.exists() {
            let root = self.root.canonicalize()?;
            let resolved = package_directory.canonicalize()?;
            anyhow::ensure!(
                resolved.starts_with(root.join("packages")),
                "Invalid package deletion path"
            );
            std::fs::remove_dir_all(resolved)?;
        }
        if delete_data {
            let path = self.data_directory(id);
            if path.exists() {
                let root = self.root.canonicalize()?;
                let resolved = path.canonicalize()?;
                anyhow::ensure!(
                    resolved.starts_with(root.join("data")),
                    "Invalid data deletion path"
                );
                std::fs::remove_dir_all(resolved)?;
            }
        }
        Ok(())
    }
    pub fn event(&mut self, id: &str, event: Event) -> anyhow::Result<()> {
        let instance = self
            .live
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("Plugin is disabled"))?;
        if let Err(error) = instance.call(Message::Event(event)) {
            instance.stop();
            if let Some(entry) = self.installed.get_mut(id) {
                entry.error = Some(format!("{error:#}"));
            }
            return Err(error);
        }
        Ok(())
    }
    pub fn poll(&mut self) {
        for (id, instance) in &mut self.live {
            if let Err(error) = instance.poll() {
                instance.stop();
                if let Some(entry) = self.installed.get_mut(id) {
                    entry.error = Some(format!("{error:#}"));
                }
            }
        }
    }
    /// Periodic and shutdown checkpoints use atomic files, leaving last good data on failure.
    pub fn checkpoint(&mut self) -> anyhow::Result<()> {
        let ids: Vec<_> = self.live.keys().cloned().collect();
        let mut failures = vec![];
        for id in ids {
            match self
                .live
                .get_mut(&id)
                .unwrap()
                .snapshot()
                .and_then(|snapshot| self.save_snapshot(&id, &snapshot))
            {
                Ok(()) => {}
                Err(error) => failures.push(format!("{id}: {error:#}")),
            }
        }
        anyhow::ensure!(failures.is_empty(), "{}", failures.join("; "));
        Ok(())
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod icon_tests {
    use super::*;

    /// An installed panel resolves separate package-owned artwork for each palette.
    #[test]
    fn installed_panel_selects_light_and_dark_icons() {
        let root = tempfile::tempdir().unwrap();
        let digest = "a".repeat(64);
        let icons = root
            .path()
            .join("packages/me.example")
            .join(&digest)
            .join("icons");
        std::fs::create_dir_all(&icons).unwrap();
        let light = b"<svg xmlns=\"http://www.w3.org/2000/svg\" fill=\"black\"/>";
        let dark = b"<svg xmlns=\"http://www.w3.org/2000/svg\" fill=\"white\"/>";
        std::fs::write(icons.join("light.svg"), light).unwrap();
        std::fs::write(icons.join("dark.svg"), dark).unwrap();
        let installed = Installed {
            manifest: Manifest {
                id: "me.example".into(),
                name: "Example".into(),
                version: "1.0.0".into(),
                protocol: 1,
                component: Some("example.wasm".into()),
                contributions: None,
                permissions: BTreeSet::new(),
                panels: vec![Panel {
                    id: "main".into(),
                    title: "Main".into(),
                    position: "bottom".into(),
                    status_order: None,
                    icon_light: Some("icons/light.svg".into()),
                    icon_dark: Some("icons/dark.svg".into()),
                }],
                commands: vec![],
                storage_limit: 1024,
            },
            digest,
            grants: BTreeSet::new(),
            enabled: true,
            error: None,
        };
        assert_eq!(
            installed.panel_icon(root.path(), "main", false).unwrap(),
            light
        );
        assert_eq!(
            installed.panel_icon(root.path(), "main", true).unwrap(),
            dark
        );
    }
}
impl Drop for Manager {
    fn drop(&mut self) {
        let _ = self.checkpoint();
        self.live.clear();
    }
}
