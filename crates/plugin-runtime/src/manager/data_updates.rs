//! Preparation keeps the old owner usable; final migration runs against its latest serialized private state.
use super::*;
use crate::{
    InstallControl,
    data_transaction::{self, Transaction},
};

/// Opaque, manager-bound preparation token; callers may continue dispatching old-version commands before commit.
pub struct PreparedInstallation {
    pub(super) enable_requested: bool,
    pub(super) root: PathBuf,
    pub(super) workspace: String,
    pub(super) previous_digest: Option<String>,
    pub(super) package: Package,
    pub(super) grants: BTreeSet<String>,
    pub(super) transaction: Transaction,
    pub(super) next: Instance,
    /// Final discovery must match the plans already prepared and authorized off the actor thread.
    pub(super) dependency_plans: BTreeMap<String, plugin_protocol::dependencies::Plan>,
}
impl Manager {
    /// Private-data writers have one owner across processes; declarative dependency managers need no such lease.
    pub(super) fn acquire_data_owner(&mut self) -> anyhow::Result<()> {
        if self._runtime_lock.is_none() {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(self.root.join("runtime.lock"))?;
            file.try_lock().map_err(|error| {
                anyhow::anyhow!(
                    "Private plugin data is already owned by another running host: {error}"
                )
            })?;
            // The previous owner may have committed between our metadata read and acquiring this lease.
            self.installed = Self::read_registry(&self.root)?;
            self._runtime_lock = Some(file);
        }
        Ok(())
    }
    /// Compile, validate configuration and prepare dependencies without retiring the currently installed instance.
    pub fn prepare_installation(
        &mut self,
        package: &Package,
        grants: BTreeSet<String>,
        control: &InstallControl,
    ) -> anyhow::Result<PreparedInstallation> {
        self.begin_installation(package, grants, control)?
            .run(control)
    }

    /// Capture bounded owner/configuration state; extraction, compilation and downloads belong to the job.
    pub fn begin_installation(
        &mut self,
        package: &Package,
        grants: BTreeSet<String>,
        control: &InstallControl,
    ) -> anyhow::Result<InstallationPreparation> {
        drop(crate::data_transaction::recover(&self.root)?);
        self.acquire_data_owner()?;
        self.validate_data_update(package, &grants)?;
        control.check()?;
        let id = &package.manifest.id;
        let scope = self
            .instance_data_directory(&package.manifest)
            .parent()
            .unwrap()
            .to_owned();
        let transaction = Transaction::new(&self.root, &scope)?;
        // Dependency-only managers can publish unrelated packages; preserve their latest committed records.
        if let Some(bytes) = transaction.registry()? {
            self.installed = crate::migration::decode_registry(&bytes)?.0;
        }
        if self.engine.is_none() {
            self.engine = Some(Instance::engine()?);
        }
        Ok(InstallationPreparation {
            root: self.root.clone(),
            environment: self.environment.clone(),
            host_resources: self.host_resources.clone(),
            previous_digest: self.installed.get(id).map(|entry| entry.digest.clone()),
            package: package.clone(),
            grants,
            transaction,
            engine: self.engine.as_ref().unwrap().clone(),
            configuration: self.saved_settings(&package.manifest)?,
        })
    }

    /// The actor holds exclusive access across final copy, migration, activation and durable commit.
    pub fn commit_installation(
        &mut self,
        mut prepared: PreparedInstallation,
        control: &InstallControl,
    ) -> anyhow::Result<()> {
        let manifest = &prepared.package.manifest;
        let id = manifest.id.clone();
        self.validate_data_update(&prepared.package, &prepared.grants)?;
        anyhow::ensure!(
            prepared.root == self.root
                && prepared.workspace == self.environment.workspace
                && prepared.previous_digest
                    == self.installed.get(&id).map(|entry| entry.digest.clone()),
            "Prepared installation is stale; prepare it again"
        );
        control.check()?;
        // Read the last safe fallback before retiring any owner; final snapshot failures also require recovery.
        let mut snapshot = self.load_snapshot(&id)?;
        let previous = self.installed.get(&id).cloned();
        let mut old = self.live.remove(&id);
        if let Some(instance) = &mut old {
            instance.quiesce();
        }
        self.retire_language_services(&id);
        self.refresh_services();
        let candidate = prepared.transaction.candidate();
        // Undeclared formats retain version one, but use exactly the same transactional cutover.
        let target = manifest
            .data_format
            .as_ref()
            .map_or(1, |format| format.version);
        let hook = manifest
            .data_format
            .as_ref()
            .is_some_and(|format| format.migration_hook);
        let result = (|| {
            if let Some(instance) = &mut old {
                snapshot = Some(instance.snapshot()?);
                instance.stop();
            }
            control.stage(crate::InstallStage::Migrating)?;
            prepared.transaction.refresh()?;
            crate::migration::stage_legacy_data(&self.root, &id, &self.environment, &candidate)?;
            // Dormant scopes and legacy reinstalls have no live checkpoint. Read their imported final copy.
            if old.is_none() {
                snapshot = crate::migration::scope_snapshot(&candidate)?;
            }
            let from = data_version(&candidate, prepared.transaction.source_exists())?;
            prepared.next.prepare_state(
                self.environment.clone(),
                if from == target {
                    snapshot.clone()
                } else {
                    None
                },
            )?;
            let migrated = if from != target {
                anyhow::ensure!(
                    from == 0 || hook,
                    "Data format change requires a migration hook"
                );
                if hook {
                    prepared.next.migrate_data(from, target, snapshot.clone())?
                } else {
                    snapshot.clone()
                }
            } else {
                snapshot.clone()
            };
            if let Some(value) = &migrated {
                anyhow::ensure!(
                    value.data.len() <= manifest.storage_limit,
                    "Migrated snapshot exceeds quota"
                );
            }
            // Final discovery sees current data/configuration. Changed plans must be prepared anew, outside cutover.
            prepared
                .next
                .prepare_state(self.environment.clone(), migrated)?;
            self.configure_saved_settings(&mut prepared.next, manifest)?;
            let values = prepared.next.configuration.clone();
            let plans = dependencies::resolve_dependency_plans(
                &self.environment,
                &prepared.package,
                Some(&mut prepared.next),
                &values,
            )?;
            anyhow::ensure!(
                serde_json::to_vec(&plans)? == serde_json::to_vec(&prepared.dependency_plans)?,
                "Dependency selection changed during preparation; prepare the update again"
            );
            prepared
                .next
                .connect_services(self.plugin_services.clone())?;
            control.check()?;
            prepared
                .next
                .connect_diagnostics(self.native_diagnostics.clone());
            prepared.next.activate()?;
            prepared.next.commit_data()?;
            let checkpoint = prepared.next.snapshot()?;
            anyhow::ensure!(
                checkpoint.data.len() <= manifest.storage_limit,
                "Snapshot exceeds declared quota"
            );
            atomic_write(
                &candidate.join("state.json"),
                &serde_json::to_vec(&checkpoint)?,
            )?;
            atomic_write(
                &candidate.join("data-format.json"),
                &serde_json::to_vec(&target)?,
            )?;
            let mut registry = self.installed.clone();
            registry.insert(
                id.clone(),
                Installed {
                    manifest: manifest.clone(),
                    digest: prepared.package.digest.clone(),
                    grants: prepared.grants.clone(),
                    enabled: prepared.enable_requested
                        || previous.as_ref().is_none_or(|entry| entry.enabled),
                    project_enabled: previous
                        .as_ref()
                        .map(|entry| entry.project_enabled.clone())
                        .unwrap_or_default(),
                    global_enabled: None,
                    retired_ui_contract: false,
                    error: None,
                },
            );
            prepared
                .transaction
                .publish(&serde_json::to_vec_pretty(&registry)?, control)?;
            self.installed = registry;
            Ok::<_, anyhow::Error>(())
        })();
        if let Err(error) = result {
            prepared.next.stop();
            if let Some(instance) = &mut old {
                // Snapshot failure happens before stop; no native ownership may remain during recovery.
                instance.stop();
            }
            if prepared.transaction.recovery_pending() {
                if let Some(entry) = self.installed.get_mut(&id) {
                    entry.error = Some(format!("Recovery incomplete: {error:#}"));
                }
                self.refresh_services();
                return Err(error);
            }
            if let Some(mut old) = old {
                if let Err(recovery) = old.restart(self.environment.clone(), snapshot) {
                    old.stop();
                    let message = format!("Update failed: {error:#}; restart failed: {recovery:#}");
                    if let Some(entry) = self.installed.get_mut(&id) {
                        entry.error = Some(message.clone());
                    }
                    self.refresh_services();
                    return Err(anyhow::anyhow!(message));
                }
                self.live.insert(id, old);
            }
            self.refresh_services();
            return Err(error);
        }
        prepared
            .next
            .retarget_data(self.instance_data_directory(manifest));
        let entry = &self.installed[&id];
        if entry.enabled || entry.project_enabled_in(&self.environment.workspace) {
            self.live.insert(id.clone(), prepared.next);
        } else {
            prepared.next.stop();
        }
        self.refresh_services();
        Ok(())
    }

    /// Preparation tokens carry no additional authority and are invalid after scope or installed-version changes.
    fn validate_data_update(
        &self,
        package: &Package,
        grants: &BTreeSet<String>,
    ) -> anyhow::Result<()> {
        crate::capabilities::require_current(&package.manifest)?;
        anyhow::ensure!(
            self.trusted && self.workspace_open,
            "Workspace is restricted or closed"
        );
        anyhow::ensure!(
            package.manifest.protocol == 7 && package.manifest.component.is_some(),
            "Transactional preparation requires a protocol 7 WASM package"
        );
        anyhow::ensure!(
            package.manifest.data_format.is_some()
                || self
                    .installed
                    .get(&package.manifest.id)
                    .is_none_or(|old| old.manifest.data_format.is_none()),
            "Removing a data format requires an explicit migration"
        );
        anyhow::ensure!(
            package.manifest.permissions.is_subset(grants),
            "Permission confirmation required"
        );
        anyhow::ensure!(
            // Initializing another scope of the already installed immutable package cannot replace parked code.
            self.installed
                .get(&package.manifest.id)
                .is_some_and(|old| old.digest == package.digest
                    && old.grants == *grants
                    && serde_json::to_value(&old.manifest).ok()
                        == serde_json::to_value(&package.manifest).ok())
                || self
                    .parked
                    .values()
                    .all(|scope| !scope.live.contains_key(&package.manifest.id)),
            "Close other workspace instances before replacing this package"
        );
        anyhow::ensure!(
            self.installed
                .get(&package.manifest.id)
                .is_none_or(|old| old.manifest.scope == package.manifest.scope),
            "Changing instance scope requires reinstalling the package"
        );
        Ok(())
    }
}

/// The persisted version belongs to the logical data scope, not to the globally installed package record.
pub(super) fn data_version(scope: &Path, installed: bool) -> anyhow::Result<u32> {
    Ok(
        match data_transaction::read_optional(&scope.join("data-format.json"))? {
            Some(bytes) => serde_json::from_slice(&bytes)?,
            None if installed || std::fs::read_dir(scope.join("files"))?.next().is_some() => 1,
            None => 0,
        },
    )
}
