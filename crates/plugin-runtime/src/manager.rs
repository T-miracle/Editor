//! Package lifecycle owns rollback, opaque snapshots and resource retirement.
use super::{Instance, Package, package::atomic_write};
use plugin_protocol::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
mod data_updates;
mod dependencies;
mod language;
mod plugin_services;
mod recovery;
pub use data_updates::PreparedInstallation;
pub(crate) mod scopes;
mod settings;
mod ui_events;
use scopes::ParkedWorkspace;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Installed {
    pub manifest: Manifest,
    pub digest: String,
    pub grants: BTreeSet<String>,
    pub enabled: bool,
    /// Workspace paths whose plugin instance may run despite the global default.
    #[serde(default)]
    pub project_enabled: BTreeSet<String>,
    /// UI snapshots distinguish the global preference from effective availability.
    #[serde(skip)]
    pub global_enabled: Option<bool>,
    #[serde(skip)]
    pub error: Option<String>,
}
impl Installed {
    /// Project choices follow canonical workspace identity, including Windows extended paths.
    pub fn project_enabled_in(&self, workspace: &str) -> bool {
        let key = scopes::workspace_key(workspace);
        self.project_enabled
            .iter()
            .any(|path| scopes::workspace_key(path) == key)
    }
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
    /// Executable guests share one private-data owner; metadata/dependency-only managers remain independent.
    _runtime_lock: Option<std::fs::File>,
    diagnostic_history: BTreeMap<String, Vec<crate::faults::Diagnostic>>,
    plugin_services: crate::plugin_services::Shared,
    root: PathBuf,
    environment: Environment,
    engine: Option<wasmtime::Engine>,
    pub installed: BTreeMap<String, Installed>,
    pub live: BTreeMap<String, Instance>,
    /// Only the selected workspace is published; parked instances retain separate ownership.
    parked: BTreeMap<String, ParkedWorkspace>,
    trusted: bool,
    workspace_open: bool,
    language_services: BTreeMap<String, language::Prepared>,
}
impl Manager {
    /// Identify the workspace whose override is active in this runtime.
    pub fn workspace(&self) -> &str {
        &self.environment.workspace
    }
    /// Read metadata without starting WASM; first read migrates legacy IDs and preserves private data.
    pub fn read_registry(root: &Path) -> anyhow::Result<BTreeMap<String, Installed>> {
        let _transaction_guard = crate::data_transaction::recover(root)?;
        let installed = match std::fs::read(root.join("registry.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(e.into()),
        };
        crate::migration::migrate_registry(root, installed)
    }
    pub fn open(root: PathBuf, environment: Environment) -> anyhow::Result<Self> {
        Self::open_with_trust(root, environment, true)
    }
    /// Trust is a host decision, never sourced from a plugin manifest or project settings.
    pub fn open_with_trust(
        root: PathBuf,
        environment: Environment,
        trusted: bool,
    ) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&root)?;
        let installed = Self::read_registry(&root)?;
        let mut manager = Self {
            _runtime_lock: None,
            diagnostic_history: BTreeMap::new(),
            plugin_services: std::sync::Arc::new(std::sync::Mutex::new(
                crate::plugin_services::Broker::new(plugin_services::read_preferences(&root)?),
            )),
            root,
            environment,
            // Resource-only packages need no Wasmtime engine during startup.
            engine: None,
            installed,
            live: BTreeMap::new(),
            parked: BTreeMap::new(),
            trusted,
            workspace_open: true,
            language_services: BTreeMap::new(),
        };
        if manager
            .installed
            .values()
            .any(|entry| entry.manifest.component.is_some())
        {
            manager.acquire_data_owner()?;
        }
        let ids: Vec<_> = manager
            .installed
            .iter()
            .filter(|(_, p)| {
                trusted
                    && (p.enabled
                        || (p.manifest.scope == api::InstanceScope::Workspace
                            && p.project_enabled_in(&manager.environment.workspace)))
            })
            .map(|(id, p)| (id.clone(), p.enabled))
            .collect();
        manager.activate_saved_plugins(ids);
        // Startup activation may temporarily persist an enabled value; restore global defaults.
        manager.save_registry()?;
        Ok(manager)
    }
    pub fn data_directory(&self, id: &str) -> PathBuf {
        self.installed
            .get(id)
            .map(|entry| self.instance_data_directory(&entry.manifest))
            .unwrap_or_else(|| self.root.join("data").join(id))
    }
    fn snapshot_path(&self, id: &str) -> PathBuf {
        if self
            .installed
            .get(id)
            .is_some_and(|entry| entry.manifest.protocol == 7)
        {
            return self.data_directory(id).parent().unwrap().join("state.json");
        }
        let workspace = format!(
            "{:x}",
            Sha256::digest(self.environment.workspace.as_bytes())
        );
        self.data_directory(id)
            .join(format!("state-{}.json", &workspace[..16]))
    }
    fn save_registry(&self) -> anyhow::Result<()> {
        // Other metadata managers cannot write through an in-flight private-data/registry transaction.
        let _transaction_guard = crate::data_transaction::recover(&self.root)?;
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
        self.install_with_control(package, grants, &crate::InstallControl::default())
    }
    /// The host shares cancellation/progress with its UI without permitting guest callbacks to grant consent.
    pub fn install_with_control(
        &mut self,
        package: &Package,
        grants: BTreeSet<String>,
        control: &crate::InstallControl,
    ) -> anyhow::Result<()> {
        control.check()?;
        anyhow::ensure!(
            self.trusted && self.workspace_open,
            "Workspace is restricted or closed"
        );
        anyhow::ensure!(
            self.parked
                .values()
                .all(|scope| !scope.live.contains_key(&package.manifest.id)),
            "Close other workspace instances before replacing this package"
        );
        let id = package.manifest.id.clone();
        // Changing ownership needs an explicit data migration, not an implicit update cutover.
        anyhow::ensure!(
            self.installed
                .get(&id)
                .is_none_or(|old| old.manifest.scope == package.manifest.scope),
            "Changing instance scope requires reinstalling the package"
        );
        anyhow::ensure!(
            package.manifest.permissions.is_subset(&grants),
            "Permission confirmation required"
        );
        if package.manifest.component.is_none() {
            anyhow::ensure!(
                self.installed
                    .get(&id)
                    .is_none_or(|old| old.manifest.data_format.is_none()),
                "Removing a data format requires an explicit migration"
            );
            return self.install_declarative(package, grants, control);
        }
        if package.manifest.data_format.is_some() {
            let prepared = self.prepare_installation(package, grants, control)?;
            return self.commit_installation(prepared, control);
        }
        anyhow::ensure!(
            self.installed
                .get(&id)
                .is_none_or(|old| old.manifest.data_format.is_none()),
            "Removing a data format requires an explicit migration"
        );
        self.acquire_data_owner()?;
        let snapshot = if let Some(old) = self.live.get_mut(&id) {
            Some(old.snapshot()?)
        } else {
            self.load_snapshot(&id)?
        };
        let version = self.root.join("packages").join(&id).join(&package.digest);
        package.extract(&version)?;
        control.check()?;
        if self.engine.is_none() {
            self.engine = Some(Instance::engine()?);
        }
        let mut next = Instance::prepare(
            self.engine.as_ref().unwrap(),
            package.component().expect("component checked above"),
            &package.manifest,
            &grants,
            self.environment.clone(),
            self.instance_data_directory(&package.manifest),
            version,
            snapshot.clone(),
        )?;
        self.configure_saved_settings(&mut next, &package.manifest)?;
        self.refresh_services();
        next.connect_services(self.plugin_services.clone())?;
        self.prepare_dependencies(package, Some(&mut next), control)?;
        if let Some(snapshot) = &snapshot {
            self.save_snapshot(&id, snapshot)?;
        }
        // Cutover stops old process trees. Rollback starts new shells from the old snapshot.
        control.check()?;
        let mut old = self.live.remove(&id);
        if let Some(old) = &mut old {
            old.stop();
        }
        let previous = self.installed.get(&id).cloned();
        let mut original_data = vec![];
        let result = (|| {
            control.check()?;
            next.activate()?;
            original_data = next.commit_data()?;
            self.installed.insert(
                id.clone(),
                Installed {
                    manifest: package.manifest.clone(),
                    digest: package.digest.clone(),
                    grants,
                    enabled: true,
                    project_enabled: previous
                        .as_ref()
                        .map(|entry| entry.project_enabled.clone())
                        .unwrap_or_default(),
                    global_enabled: None,
                    error: None,
                },
            );
            self.save_registry()?;
            Ok::<_, anyhow::Error>(())
        })();
        match result {
            Ok(()) => {
                self.live.insert(id.clone(), next);
                self.restore_install_scope(&id, previous.as_ref())
            }
            Err(error) => {
                next.stop();
                Instance::rollback_data(&original_data)?;
                if let Some(previous) = previous {
                    self.installed.insert(id.clone(), previous);
                    // Reuse the old compiled instance; a registry I/O failure must not prevent recovery.
                    if let Some(mut old) = old {
                        let rollback = old.restart(self.environment.clone(), snapshot);
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
        control: &crate::InstallControl,
    ) -> anyhow::Result<()> {
        let id = package.manifest.id.clone();
        if let Some(old) = self.live.get_mut(&id) {
            let snapshot = old.snapshot()?;
            self.save_snapshot(&id, &snapshot)?;
        }
        let version = self.root.join("packages").join(&id).join(&package.digest);
        package.extract(&version)?;
        self.prepare_dependencies(package, None, control)?;
        let prior_projects = self
            .installed
            .get(&id)
            .map(|entry| entry.project_enabled.clone())
            .unwrap_or_default();
        control.check()?;
        let previous = self.installed.insert(
            id.clone(),
            Installed {
                manifest: package.manifest.clone(),
                digest: package.digest.clone(),
                grants,
                enabled: true,
                project_enabled: prior_projects,
                global_enabled: None,
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
        self.restore_install_scope(&id, previous.as_ref())
    }
    /// Package replacement keeps the user's global default and project override.
    fn restore_install_scope(
        &mut self,
        id: &str,
        previous: Option<&Installed>,
    ) -> anyhow::Result<()> {
        self.retire_language_services(id);
        if let Some(previous) = previous.filter(|entry| !entry.enabled) {
            let locally_enabled = previous.project_enabled_in(&self.environment.workspace);
            self.disable(id)?;
            if locally_enabled {
                self.set_project_enabled(id, true)?;
            }
        }
        Ok(())
    }
    /// Re-enable from the last committed version and plugin-owned snapshot.
    pub fn enable(&mut self, id: &str) -> anyhow::Result<()> {
        // A prior failed rollback must finish before any instance can touch the formal data directory.
        drop(crate::data_transaction::recover(&self.root)?);
        anyhow::ensure!(
            self.trusted && self.workspace_open,
            "Workspace is restricted or closed"
        );
        if self.live.contains_key(id) {
            if let Some(entry) = self.installed.get_mut(id) {
                entry.enabled = true;
            }
            self.save_registry()?;
            return Ok(());
        }
        if self
            .installed
            .get(id)
            .is_some_and(|entry| entry.manifest.component.is_some())
        {
            self.acquire_data_owner()?;
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
        // A dormant workspace may still contain an older format than the globally installed package.
        if let Some(format) = &entry.manifest.data_format {
            let scope = self.data_directory(id).parent().unwrap().to_owned();
            let from = if scope.exists() {
                data_updates::data_version(&scope, true)?
            } else {
                0
            };
            if from != format.version {
                let package = Package {
                    manifest: entry.manifest.clone(),
                    digest: entry.digest.clone(),
                    files: [(component_path.clone(), component)].into(),
                    source: None,
                };
                let control = crate::InstallControl::default();
                let mut prepared =
                    self.prepare_installation(&package, entry.grants.clone(), &control)?;
                prepared.enable_requested = true;
                return self.commit_installation(prepared, &control);
            }
        }
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
        self.configure_saved_settings(&mut instance, &entry.manifest)?;
        self.refresh_services();
        instance.connect_services(self.plugin_services.clone())?;
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
        let result = self.disable_current(id);
        self.retire_parked_plugin(id);
        self.refresh_services();
        result
    }
    /// Project overrides retire only their owner; global disable additionally retires parked owners.
    fn disable_current(&mut self, id: &str) -> anyhow::Result<()> {
        self.retire_language_services(id);
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
        let entry = self
            .installed
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("Unknown plugin"))?;
        entry.enabled = false;
        let key = scopes::workspace_key(&self.environment.workspace);
        entry
            .project_enabled
            .retain(|path| scopes::workspace_key(path) != key);
        self.save_registry()
    }
    /// Override a globally disabled plugin for the current workspace only.
    pub fn set_project_enabled(&mut self, id: &str, enabled: bool) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.installed
                .get(id)
                .is_some_and(|entry| entry.manifest.scope == api::InstanceScope::Workspace),
            "Application instances do not have project overrides"
        );
        let workspace = self.environment.workspace.clone();
        let globally_enabled = self
            .installed
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("Unknown plugin"))?
            .enabled;
        anyhow::ensure!(!globally_enabled, "Global default is already enabled");
        if enabled {
            self.enable(id)?;
            let entry = self.installed.get_mut(id).unwrap();
            entry.enabled = false;
            entry.project_enabled.insert(workspace);
        } else {
            self.disable_current(id)?;
            self.installed
                .get_mut(id)
                .unwrap()
                .project_enabled
                .remove(&workspace);
        }
        self.save_registry()
    }
    /// Uninstall keeps state unless the user explicitly chose deletion in the manager UI.
    pub fn uninstall(&mut self, id: &str, delete_data: bool) -> anyhow::Result<()> {
        self.disable(id)?;
        self.installed.remove(id);
        self.save_registry()?;
        // Active native-service leases remain independently pinned; only installation/rollback pins are removed.
        let receipts = self.root.join("dependency-receipts").join(id);
        if receipts.exists() {
            let resolved = receipts.canonicalize()?;
            anyhow::ensure!(
                resolved.starts_with(self.root.canonicalize()?.join("dependency-receipts")),
                "Invalid dependency receipt deletion path"
            );
            std::fs::remove_dir_all(resolved)?;
        }
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
            self.remove_saved_settings(id)?;
            let path = self.root.join("data").join(id);
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
    /// Invoke a declared command independently of panel visibility, retaining permission checks.
    /// Parameters belong to the plugin; validation failures do not enter or retire its instance.
    pub fn invoke_command(
        &mut self,
        plugin: &str,
        command: &str,
        arguments: serde_json::Value,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.live.contains_key(plugin),
            "Plugin is not running: {plugin}"
        );
        let entry = self
            .installed
            .get(plugin)
            .ok_or_else(|| anyhow::anyhow!("Plugin is not installed: {plugin}"))?;
        anyhow::ensure!(
            entry
                .manifest
                .commands
                .iter()
                .any(|item| item.id == command),
            "Command is not declared by {plugin}: {command}"
        );
        anyhow::ensure!(
            serde_json::to_vec(&arguments)?.len() <= 65536,
            "Plugin command arguments exceed 64 KiB"
        );
        self.event(
            plugin,
            Event::Command {
                id: command.into(),
                cwd: None,
                text: None,
                arguments: (!arguments.is_null()).then_some(arguments),
            },
        )
    }
    pub fn poll(&mut self) {
        self.route_services();
        self.poll_parked();
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
        let ids: Vec<_> = self
            .live
            .iter()
            .filter(|(_, instance)| instance.service_provider().is_some())
            .map(|(id, _)| id.clone())
            .collect();
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
            .join("packages/example")
            .join(&digest)
            .join("icons");
        std::fs::create_dir_all(&icons).unwrap();
        let light = b"<svg xmlns=\"http://www.w3.org/2000/svg\" fill=\"black\"/>";
        let dark = b"<svg xmlns=\"http://www.w3.org/2000/svg\" fill=\"white\"/>";
        std::fs::write(icons.join("light.svg"), light).unwrap();
        std::fs::write(icons.join("dark.svg"), dark).unwrap();
        let installed = Installed {
            manifest: Manifest {
                data_format: None,
                plugin_services: Default::default(),
                language_servers: Default::default(),
                services: Default::default(),
                settings: Default::default(),
                settings_hook: false,
                id: "example".into(),
                name: "Example".into(),
                version: "1.0.0".into(),
                protocol: 1,
                api: None,
                scope: api::InstanceScope::Workspace,
                component: Some("example.wasm".into()),
                contributions: None,
                permissions: BTreeSet::new(),
                panels: vec![Panel {
                    file_extensions: vec![],
                    id: "main".into(),
                    title: "Main".into(),
                    position: "bottom".into(),
                    default_visible: true,
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
            project_enabled: BTreeSet::new(),
            global_enabled: None,
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

#[cfg(test)]
mod scope_tests {
    use super::*;

    /// A project override starts its plugin without rewriting the global default.
    #[test]
    fn project_override_survives_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("plugins");
        let digest = "a".repeat(64);
        let contribution = root
            .join("packages/example")
            .join(&digest)
            .join("plugin.toml");
        std::fs::create_dir_all(contribution.parent().unwrap()).unwrap();
        std::fs::write(contribution, "").unwrap();
        let manifest = Manifest {
            data_format: None,
            plugin_services: Default::default(),
            language_servers: Default::default(),
            services: Default::default(),
            settings: Default::default(),
            settings_hook: false,
            id: "example".into(),
            name: "Example".into(),
            version: "1.0.0".into(),
            protocol: 1,
            api: None,
            scope: api::InstanceScope::Workspace,
            component: None,
            contributions: Some("plugin.toml".into()),
            permissions: BTreeSet::new(),
            panels: vec![],
            commands: vec![],
            storage_limit: 0,
        };
        let installed = Installed {
            manifest,
            digest,
            grants: BTreeSet::new(),
            enabled: false,
            project_enabled: BTreeSet::from(["project-a".into()]),
            global_enabled: None,
            error: None,
        };
        std::fs::write(
            root.join("registry.json"),
            serde_json::to_vec(&BTreeMap::from([("example", installed)])).unwrap(),
        )
        .unwrap();
        let environment = Environment {
            workspace: "project-a".into(),
            ..Environment::default()
        };
        let mut manager = Manager::open(root.clone(), environment).unwrap();
        assert!(!manager.installed["example"].enabled);
        assert!(
            manager.installed["example"]
                .project_enabled
                .contains("project-a")
        );
        assert!(!Manager::read_registry(&root).unwrap()["example"].enabled);
        manager.set_project_enabled("example", false).unwrap();
        assert!(
            !manager.installed["example"]
                .project_enabled
                .contains("project-a")
        );
        manager.set_project_enabled("example", true).unwrap();
        let persisted = Manager::read_registry(&root).unwrap();
        assert!(!persisted["example"].enabled);
        assert!(persisted["example"].project_enabled.contains("project-a"));
        // Declarative packages stay useful without a guest, but project preferences cannot grant trust.
        assert!(manager.engine.is_none() && manager.live.is_empty());
        assert!(manager.published_entries()[0].enabled);
        manager.set_workspace_trust(false).unwrap();
        assert!(!manager.published_entries()[0].enabled);
        assert!(manager.set_project_enabled("example", true).is_err());
        assert!(manager.engine.is_none());
    }
}
impl Drop for Manager {
    /// Checkpoint failures never prevent scope-wide resource retirement.
    fn drop(&mut self) {
        self.shutdown();
    }
}
