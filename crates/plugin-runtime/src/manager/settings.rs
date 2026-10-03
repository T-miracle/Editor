//! Resolve namespaced configuration without allowing it to grant runtime authority.
use super::*;
use plugin_protocol::settings::{Effective, EffectiveValue, Scope, Source};
use serde_json::Value;

/// Project choices live in host-owned settings, never in an automatically trusted repository file.
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedSettings {
    #[serde(default)]
    user: BTreeMap<String, Value>,
    #[serde(default)]
    projects: BTreeMap<String, BTreeMap<String, Value>>,
}
impl SavedSettings {
    fn read(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read(path) {
            Ok(bytes) => {
                anyhow::ensure!(bytes.len() <= 1024 * 1024, "Settings exceed 1 MiB");
                Ok(serde_json::from_slice(&bytes)?)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }
    /// Validate every explicit layer before resolving; a stronger layer cannot hide an invalid user value.
    fn resolve(&self, manifest: &Manifest, workspace: &str) -> anyhow::Result<Effective> {
        let project = self.projects.get(&scopes::workspace_key(workspace));
        for (values, scope) in [(Some(&self.user), Scope::User), (project, Scope::Project)] {
            for (key, value) in values.into_iter().flatten() {
                validate_value(manifest, scope, key, value)?;
            }
        }
        Ok(manifest
            .settings
            .iter()
            .map(|(key, definition)| {
                let (value, source) =
                    if let Some(value) = project.and_then(|values| values.get(key)) {
                        (value, Source::Project)
                    } else if let Some(value) = self.user.get(key) {
                        (value, Source::User)
                    } else {
                        (&definition.default, Source::Default)
                    };
                (
                    key.clone(),
                    EffectiveValue {
                        value: value.clone(),
                        source,
                    },
                )
            })
            .collect())
    }
}

/// Only declared plugin keys are writable; scope validation also blocks application owners borrowing a project.
fn validate_value(
    manifest: &Manifest,
    scope: Scope,
    key: &str,
    value: &Value,
) -> anyhow::Result<()> {
    let definition = manifest
        .settings
        .get(key)
        .ok_or_else(|| anyhow::anyhow!("Unknown setting: {key}"))?;
    anyhow::ensure!(
        scope != Scope::Project
            || (definition.scope == Scope::Project
                && manifest.scope == api::InstanceScope::Workspace),
        "Setting does not allow a project override: {key}"
    );
    anyhow::ensure!(
        definition.accepts(value),
        "Invalid value for setting: {key}"
    );
    Ok(())
}

impl Manager {
    /// Candidates use their own declarations while sharing only user-approved persistent overrides.
    pub(super) fn saved_settings(&self, manifest: &Manifest) -> anyhow::Result<Effective> {
        SavedSettings::read(&self.settings_path(&manifest.id))?
            .resolve(manifest, &self.environment.workspace)
    }
    /// Return effective plugin values and provenance through the same manager used by the settings UI.
    pub fn effective_settings(&self, id: &str) -> anyhow::Result<Effective> {
        let entry = self
            .installed
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("Unknown plugin"))?;
        if let Some(instance) = self.live.get(id) {
            return Ok(instance.configuration.clone());
        }
        SavedSettings::read(&self.settings_path(id))?
            .resolve(&entry.manifest, &self.environment.workspace)
    }

    /// Install, reopen and enable resolve the same persistent layers before guest activation.
    pub(super) fn configure_saved_settings(
        &self,
        instance: &mut Instance,
        manifest: &Manifest,
    ) -> anyhow::Result<()> {
        let values = SavedSettings::read(&self.settings_path(&manifest.id))?
            .resolve(manifest, &self.environment.workspace)?;
        instance.configure_settings(manifest, values)
    }

    /// Host UI confirmation is required before this write; guest APIs cannot grant themselves overrides.
    pub fn update_setting(
        &mut self,
        id: &str,
        scope: Scope,
        key: &str,
        value: Option<Value>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.trusted && self.workspace_open,
            "Workspace is restricted or closed"
        );
        let entry = self
            .installed
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("Unknown plugin"))?
            .clone();
        let manifest = &entry.manifest;
        let definition = manifest
            .settings
            .get(key)
            .ok_or_else(|| anyhow::anyhow!("Unknown setting: {key}"))?;
        validate_value(
            manifest,
            scope,
            key,
            value.as_ref().unwrap_or(&definition.default),
        )?;
        let path = self.settings_path(id);
        let mut saved = SavedSettings::read(&path)?;
        let target = match scope {
            Scope::User => &mut saved.user,
            Scope::Project => saved
                .projects
                .entry(scopes::workspace_key(&self.environment.workspace))
                .or_default(),
        };
        if let Some(value) = value {
            target.insert(key.into(), value);
        } else {
            target.remove(key);
        }
        saved.resolve(manifest, &self.environment.workspace)?;
        let bytes = serde_json::to_vec_pretty(&saved)?;
        anyhow::ensure!(bytes.len() <= 1024 * 1024, "Settings exceed 1 MiB");
        // Prepare all affected owners first; one rejected workspace leaves every prior owner alive.
        let mut candidates = Vec::new();
        if let Some(component) = &manifest.component {
            let assets = self.root.join("packages").join(id).join(&entry.digest);
            let wasm = std::fs::read(assets.join(component))?;
            if self.engine.is_none() {
                self.engine = Some(Instance::engine()?);
            }
            let mut owners = Vec::new();
            if let Some(current) = self.live.get_mut(id) {
                owners.push((
                    None,
                    self.environment.clone(),
                    Some(current.snapshot()?),
                    true,
                ));
            } else {
                // Disabled guests can validate configuration but must not run activation or acquire resources.
                owners.push((
                    None,
                    self.environment.clone(),
                    self.load_snapshot(id)?,
                    false,
                ));
            }
            if scope == Scope::User {
                for (key, workspace) in &mut self.parked {
                    if let Some(instance) = workspace.live.get_mut(id) {
                        owners.push((
                            Some(key.clone()),
                            workspace.environment.clone(),
                            Some(instance.snapshot()?),
                            true,
                        ));
                    }
                }
            }
            for (owner, environment, snapshot, active) in owners {
                let values = saved.resolve(manifest, &environment.workspace)?;
                let mut instance = Instance::prepare_with_resources(
                    self.engine.as_ref().unwrap(),
                    &wasm,
                    manifest,
                    &entry.grants,
                    environment.clone(),
                    self.data_directory_for(manifest, &environment),
                    assets.clone(),
                    snapshot,
                    self.host_resources.clone(),
                )?;
                instance.configure_settings(manifest, values)?;
                instance.connect_diagnostics(self.native_diagnostics.clone());
                if active {
                    instance.connect_services(self.plugin_services.clone())?;
                    instance.activate()?;
                }
                candidates.push((owner, instance, active));
            }
        }
        std::fs::create_dir_all(path.parent().unwrap())?;
        let mut originals = Vec::new();
        let commit = (|| {
            for (_, instance, active) in &mut candidates {
                if *active {
                    originals.extend(instance.commit_data()?);
                }
            }
            atomic_write(&path, &bytes)
        })();
        if let Err(error) = commit {
            Instance::rollback_data(&originals)?;
            return Err(error);
        }
        // Only a durable successful configuration replaces runtime owners; Drop revokes the old handles.
        for (owner, instance, active) in candidates {
            if !active {
                continue;
            }
            let instances = if let Some(key) = owner {
                &mut self.parked.get_mut(&key).unwrap().live
            } else {
                &mut self.live
            };
            instances.insert(id.into(), instance);
        }
        // Compute effective plans before retiring transports: unrelated settings must not restart them.
        self.language_services();
        Ok(())
    }

    /// Configuration is outside guest private roots and cannot be overwritten by storage.private.
    fn settings_path(&self, id: &str) -> PathBuf {
        self.root.join("settings").join(format!("{id}.json"))
    }

    /// Explicit delete-data includes configuration; ordinary uninstall deliberately preserves it.
    pub(super) fn remove_saved_settings(&self, id: &str) -> anyhow::Result<()> {
        let path = self.settings_path(id);
        if path.exists() {
            let root = self.root.canonicalize()?;
            anyhow::ensure!(
                path.canonicalize()?.starts_with(root.join("settings")),
                "Invalid settings deletion path"
            );
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
}
