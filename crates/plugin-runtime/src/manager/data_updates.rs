//! Preparation keeps the old owner usable; final migration runs against its latest serialized private state.
use super::*;
use crate::{
    InstallControl,
    data_transaction::{self, Transaction},
};

/// Opaque, manager-bound preparation token; callers may continue dispatching old-version commands before commit.
pub struct PreparedInstallation {
    pub(super) enable_requested: bool,
    root: PathBuf,
    workspace: String,
    previous_digest: Option<String>,
    package: Package,
    grants: BTreeSet<String>,
    transaction: Transaction,
    next: Instance,
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
        drop(crate::data_transaction::recover(&self.root)?);
        self.acquire_data_owner()?;
        self.validate_data_update(package, &grants)?;
        control.check()?;
        let id = &package.manifest.id;
        let version = self.root.join("packages").join(id).join(&package.digest);
        package.extract(&version)?;
        let scope = self
            .instance_data_directory(&package.manifest)
            .parent()
            .unwrap()
            .to_owned();
        let transaction = Transaction::new(&self.root, &scope)?;
        // Dependency-only managers can publish unrelated packages; preserve their latest committed records.
        if let Some(bytes) = transaction.registry()? {
            self.installed = serde_json::from_slice(&bytes)?;
        }
        transaction.refresh()?;
        if self.engine.is_none() {
            self.engine = Some(Instance::engine()?);
        }
        let mut next = Instance::prepare(
            self.engine.as_ref().unwrap(),
            package.component().unwrap(),
            &package.manifest,
            &grants,
            self.environment.clone(),
            transaction.candidate().join("files"),
            version,
            // Old-format snapshots belong only to the migration hook, never to new-version initialization.
            None,
        )?;
        self.configure_saved_settings(&mut next, &package.manifest)?;
        self.prepare_dependencies(package, Some(&mut next), control)?;
        Ok(PreparedInstallation {
            enable_requested: false,
            root: self.root.clone(),
            workspace: self.environment.workspace.clone(),
            previous_digest: self.installed.get(id).map(|entry| entry.digest.clone()),
            package: package.clone(),
            grants,
            transaction,
            next,
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
        // Snapshot only after earlier commands have finished. The initial preparation copy is never committed.
        control.stage(crate::InstallStage::Migrating)?;
        let snapshot = if let Some(old) = self.live.get_mut(&id) {
            Some(old.snapshot()?)
        } else {
            self.load_snapshot(&id)?
        };
        prepared.transaction.refresh()?;
        let candidate = prepared.transaction.candidate();
        let existed = self
            .instance_data_directory(manifest)
            .parent()
            .unwrap()
            .exists();
        let from = data_version(&candidate, existed)?;
        let format = manifest.data_format.as_ref().unwrap();
        prepared.next.call(Message::Prepare {
            environment: self.environment.clone(),
            snapshot: if from == format.version {
                snapshot.clone()
            } else {
                None
            },
        })?;
        let migrated = if from != format.version {
            anyhow::ensure!(
                from == 0 || format.migration_hook,
                "Data format change requires a migration hook"
            );
            if format.migration_hook {
                prepared
                    .next
                    .migrate_data(from, format.version, snapshot.clone())?
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
        // Re-prepare transfers the migrated opaque snapshot; handles from the migration have already expired.
        prepared.next.call(Message::Prepare {
            environment: self.environment.clone(),
            snapshot: migrated.clone(),
        })?;
        self.configure_saved_settings(&mut prepared.next, manifest)?;
        self.refresh_services();
        prepared
            .next
            .connect_services(self.plugin_services.clone())?;
        control.check()?;
        let previous = self.installed.get(&id).cloned();
        let mut old = self.live.remove(&id);
        if let Some(instance) = &mut old {
            instance.stop();
        }
        let result = (|| {
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
                &serde_json::to_vec(&format.version)?,
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
            if prepared.transaction.recovery_pending() {
                if let Some(entry) = self.installed.get_mut(&id) {
                    entry.error = Some(format!("Recovery incomplete: {error:#}"));
                }
                self.refresh_services();
                return Err(error);
            }
            if let Some(mut old) = old {
                if let Err(recovery) = old.restart(self.environment.clone(), snapshot) {
                    if let Some(entry) = self.installed.get_mut(&id) {
                        entry.error = Some(format!("Recovery failed: {recovery:#}"));
                    }
                    return Err(anyhow::anyhow!(
                        "Update failed: {error:#}; restart failed: {recovery:#}"
                    ));
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
        self.retire_language_services(&id);
        self.refresh_services();
        Ok(())
    }

    /// Preparation tokens carry no additional authority and are invalid after scope or installed-version changes.
    fn validate_data_update(
        &self,
        package: &Package,
        grants: &BTreeSet<String>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.trusted && self.workspace_open,
            "Workspace is restricted or closed"
        );
        anyhow::ensure!(
            package.manifest.protocol == 7
                && package.manifest.component.is_some()
                && package.manifest.data_format.is_some(),
            "Package does not declare a private data format"
        );
        anyhow::ensure!(
            package.manifest.permissions.is_subset(grants),
            "Permission confirmation required"
        );
        anyhow::ensure!(
            self.parked
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
