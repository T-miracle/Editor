//! Package lifecycle owns rollback, opaque snapshots and resource retirement.
use super::{Instance, Package, package::atomic_write};
use plugin_protocol::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
mod artwork;
mod bundles;
mod data_updates;
mod debug_services;
mod dependencies;
mod host_services;
mod image_input;
mod images;
mod language;
mod plugin_services;
mod preparation;
mod recovery;
pub use data_updates::PreparedInstallation;
pub use debug_services::{
    DebugAbilities, DebugAnswer, DebugBreakpoint, DebugFrame, DebugRequest, DebugSession,
    DebugState, DebugVariable, debug_dependency_for_test, frames_from_value, variables_from_value,
};
pub use host_services::{
    DEBUG_CONTRACT, DEFAULT_STOP_GRACE_MS, EXECUTION_CONTRACT, EXECUTION_START_TIMEOUT_MS,
    ExecutionFailure, ExecutionSnapshot, ExecutionState, HostExecution, ProviderCandidate,
    RunEnvEntry, RunRequest, StopOptions,
};
pub use preparation::InstallationPreparation;
pub(crate) mod scopes;
mod settings;
mod targets;
pub use targets::TargetRequest;
mod ui_events;
use host_services::HostSessions;
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
    /// Compatibility is derived without executing guest code or changing the user's enablement preference.
    pub fn compatibility_error(&self) -> Option<String> {
        crate::capabilities::require_current(&self.manifest)
            .err()
            .map(|error| format!("{error:#}"))
    }
    /// Project choices follow canonical workspace identity, including Windows extended paths.
    pub fn project_enabled_in(&self, workspace: &str) -> bool {
        let key = scopes::workspace_key(workspace);
        self.project_enabled
            .iter()
            .any(|path| scopes::workspace_key(path) == key)
    }
}
/// Run this module on a worker thread; native rendering reads only published documents.
pub struct Manager {
    /// Executable guests share one private-data owner; metadata/dependency-only managers remain independent.
    _runtime_lock: Option<std::fs::File>,
    diagnostic_history: BTreeMap<String, Vec<crate::faults::Diagnostic>>,
    native_diagnostics: crate::faults::NativeDiagnostics,
    plugin_services: crate::plugin_services::Shared,
    root: PathBuf,
    environment: Environment,
    /// Resource identity is fixed for this manager and every asynchronous preparation it owns.
    host_resources: crate::HostResources,
    engine: Option<wasmtime::Engine>,
    pub installed: BTreeMap<String, Installed>,
    pub live: BTreeMap<String, Instance>,
    /// Only the selected workspace is published; parked instances retain separate ownership.
    parked: BTreeMap<String, ParkedWorkspace>,
    trusted: bool,
    workspace_open: bool,
    language_services: BTreeMap<String, language::Prepared>,
    /// Byte producers are independent of WASM calls and retained only for current preview identities.
    images: BTreeMap<String, crate::images::Entry>,
    image_budget: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    /// Reservations also cover payloads retained by an accepted native writer.
    image_input_budget: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    retired_image_sources: BTreeMap<String, api::DocumentVersion>,
    /// Host-owned execution sessions started through the public service contract.
    host_sessions: HostSessions,
    /// Debug targets retain the provider incarnation and a distinct revocable native-resource root.
    debug_sessions: debug_services::Sessions,
    /// Retired with this runtime so a queued start cannot outlive the window that requested it.
    host_alive: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl Manager {
    /// Share the process-local log owner with host UI and independent real-package verification.
    /// Background preparation and retired native reporters append to this same sink.
    pub fn runtime_logs(&self) -> crate::RuntimeLogs {
        self.host_resources.logs.clone()
    }
    /// Expose an opaque live incarnation for host publications, never authority for guest-supplied requests.
    pub fn instance_id(&self, id: &str) -> Option<&str> {
        self.live.get(id).and_then(Instance::instance_id)
    }
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
        let mut installed = crate::migration::migrate_registry(root, installed)?;
        for entry in installed.values_mut() {
            entry.error = entry.compatibility_error();
        }
        Ok(installed)
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
        Self::open_with_resources(root, environment, trusted, Default::default())
    }
    /// Supply host SDK metadata without granting a guest any additional filesystem authority.
    pub fn open_with_resources(
        root: PathBuf,
        environment: Environment,
        trusted: bool,
        host_resources: crate::HostResources,
    ) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&root)?;
        let installed = Self::read_registry(&root)?;
        let host_alive = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let mut manager = Self {
            _runtime_lock: None,
            diagnostic_history: BTreeMap::new(),
            native_diagnostics: crate::faults::NativeDiagnostics::with_logs(
                host_resources.logs.clone(),
            ),
            plugin_services: std::sync::Arc::new(std::sync::Mutex::new(
                crate::plugin_services::Broker::new(plugin_services::read_preferences(&root)?),
            )),
            root,
            environment,
            host_resources,
            // Resource-only packages need no Wasmtime engine during startup.
            engine: None,
            installed,
            live: BTreeMap::new(),
            parked: BTreeMap::new(),
            trusted,
            workspace_open: true,
            language_services: BTreeMap::new(),
            images: BTreeMap::new(),
            image_budget: Default::default(),
            image_input_budget: Default::default(),
            retired_image_sources: BTreeMap::new(),
            host_sessions: HostSessions::new(host_alive.clone()),
            debug_sessions: Default::default(),
            host_alive,
        };
        if manager.installed.values().any(|entry| {
            entry.manifest.component.is_some() && entry.compatibility_error().is_none()
        }) {
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
        // Only admitted instances write snapshots; historical paths belong to the finite importer.
        self.data_directory(id).parent().unwrap().join("state.json")
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
        crate::capabilities::require_current(&package.manifest)?;
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
        let prepared = self.prepare_installation(package, grants, control)?;
        self.commit_installation(prepared, control)
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
        // Reject old metadata before reading or instantiating its component, preserving its saved preferences.
        let entry = self
            .installed
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("Unknown plugin"))?;
        crate::capabilities::require_current(&entry.manifest)?;
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
        // Dormant scopes import historical data through the same transaction as explicit format migrations.
        let scope = self.data_directory(id).parent().unwrap().to_owned();
        let needs_import = crate::migration::needs_legacy_import(&self.root, id, &scope);
        if needs_import || entry.manifest.data_format.is_some() {
            let from = if scope.exists() {
                data_updates::data_version(&scope, true)?
            } else {
                0
            };
            let target = entry
                .manifest
                .data_format
                .as_ref()
                .map_or(1, |format| format.version);
            if needs_import || from != target {
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
        let mut instance = Instance::prepare_with_resources(
            self.engine.as_ref().unwrap(),
            &component,
            &entry.manifest,
            &entry.grants,
            self.environment.clone(),
            self.data_directory(id),
            self.root.join("packages").join(id).join(&entry.digest),
            self.load_snapshot(id)?,
            self.host_resources.clone(),
        )?;
        self.configure_saved_settings(&mut instance, &entry.manifest)?;
        self.refresh_services();
        instance.connect_services(self.plugin_services.clone())?;
        instance.connect_diagnostics(self.native_diagnostics.clone());
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
        self.retire_plugin_images(id);
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
        // A durable host choice must outlive the registry entry and either plugin-data retention option.
        self.record_bundle_uninstall(id)?;
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
            crate::migration::discard_legacy_data(&self.root, id, &self.installed)?;
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
            None,
            api::Notification::Command {
                id: command.into(),
                arguments: (!arguments.is_null()).then_some(arguments),
            },
        )
    }
    pub fn poll(&mut self) {
        self.route_services();
        self.poll_parked();
        // Host sessions end when their pinned provider incarnation is gone, without replaying work.
        let present = self
            .live
            .values()
            .chain(self.parked.values().flat_map(|scope| scope.live.values()))
            .filter_map(Instance::instance_id)
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        self.host_sessions
            .retire_absent_providers(&|instance| present.contains(instance));
        for (id, instance) in &mut self.live {
            if let Err(error) = instance.poll() {
                instance.stop();
                if let Some(entry) = self.installed.get_mut(id) {
                    entry.error = Some(format!("{error:#}"));
                }
            }
        }
        self.reconcile_images();
        self.poll_execution_states();
        self.poll_debug_sessions();
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
                    view_modes: None,
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
            protocol: 7,
            api: Some(api::Requirements {
                base: "^1".parse().unwrap(),
                required: Default::default(),
                optional: Default::default(),
            }),
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
