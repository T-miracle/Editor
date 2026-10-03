//! Owned preparation inputs cross threads without lending the live manager or publishing a candidate.
use super::*;
use crate::{InstallControl, data_transaction::Transaction};

/// The actor keeps its old stores while this job compiles and prepares a private replacement.
pub struct InstallationPreparation {
    pub(super) root: PathBuf,
    pub(super) environment: Environment,
    pub(super) previous_digest: Option<String>,
    pub(super) package: Package,
    pub(super) grants: BTreeSet<String>,
    pub(super) transaction: Transaction,
    pub(super) engine: wasmtime::Engine,
    pub(super) configuration: plugin_protocol::settings::Effective,
}

impl InstallationPreparation {
    /// Run on a background thread; the resulting token must return to its original manager for cutover.
    pub fn run(self, control: &InstallControl) -> anyhow::Result<PreparedInstallation> {
        control.check()?;
        let version = self
            .root
            .join("packages")
            .join(&self.package.manifest.id)
            .join(&self.package.digest);
        self.package.extract(&version)?;
        self.transaction.refresh()?;
        control.check()?;
        let candidate = self.transaction.candidate();
        crate::migration::stage_legacy_data(
            &self.root,
            &self.package.manifest.id,
            &self.environment,
            &candidate,
        )?;
        let mut next = Instance::prepare(
            &self.engine,
            self.package
                .component()
                .expect("WASM preparation was validated by the manager"),
            &self.package.manifest,
            &self.grants,
            self.environment.clone(),
            candidate.join("files"),
            version,
            None,
        )?;
        // Discovery sees the new private format. This preview is disposable; cutover migrates the final copy again.
        let from = data_updates::data_version(&candidate, self.transaction.source_exists())?;
        let target = self
            .package
            .manifest
            .data_format
            .as_ref()
            .map_or(1, |format| format.version);
        let mut snapshot = crate::migration::scope_snapshot(&candidate)?;
        if from != target {
            let hook = self
                .package
                .manifest
                .data_format
                .as_ref()
                .is_some_and(|format| format.migration_hook);
            anyhow::ensure!(
                from == 0 || hook,
                "Data format change requires a migration hook"
            );
            if hook {
                snapshot = next.migrate_data(from, target, snapshot)?;
            }
        }
        if let Some(value) = &snapshot {
            anyhow::ensure!(
                value.data.len() <= self.package.manifest.storage_limit,
                "Migrated snapshot exceeds quota"
            );
        }
        next.call(Message::Prepare {
            environment: self.environment.clone(),
            snapshot,
        })?;
        // Flush only into the unpublished candidate and consume the write staging map. A later refresh must
        // never replay these preview writes over the old instance's final data.
        next.commit_data()?;
        next.configure_settings(&self.package.manifest, self.configuration)?;
        control.check()?;
        let configuration = next.configuration.clone();
        let dependency_plans = dependencies::prepare_dependencies_for(
            &self.root,
            &self.environment,
            &self.package,
            Some(&mut next),
            &configuration,
            control,
        )?;
        Ok(PreparedInstallation {
            enable_requested: false,
            root: self.root,
            workspace: self.environment.workspace,
            previous_digest: self.previous_digest,
            package: self.package,
            grants: self.grants,
            transaction: self.transaction,
            next,
            dependency_plans,
        })
    }
}
