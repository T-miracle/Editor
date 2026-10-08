//! Publish independent structure callbacks without requiring an LSP service or process permission.
use super::*;
use crate::StructureProvider;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64},
};

pub(super) struct Prepared {
    fingerprint: Vec<u8>,
    provider: Result<Arc<StructureProvider>, String>,
}

impl Drop for Prepared {
    fn drop(&mut self) {
        if let Ok(provider) = &self.provider {
            provider.retire();
        }
    }
}

impl Manager {
    /// Publish only healthy trusted owners; an unchanged package/configuration keeps its current lease.
    pub fn structure_providers(
        &mut self,
    ) -> BTreeMap<String, Result<Arc<StructureProvider>, String>> {
        let mut wanted = BTreeSet::new();
        for entry in self
            .published_entries()
            .into_iter()
            .filter(|entry| entry.enabled && entry.error.is_none())
        {
            for declaration in &entry.manifest.structure_providers {
                let key = format!("{}/{}", entry.manifest.id, declaration.id);
                wanted.insert(key.clone());
                let settings = self
                    .effective_settings(&entry.manifest.id)
                    .map_err(|error| format!("{error:#}"));
                let fingerprint = serde_json::to_vec(&(
                    &entry.digest,
                    &self.environment.workspace,
                    &settings,
                    &self.host_resources.sdk,
                ))
                .unwrap();
                if self
                    .structure_providers
                    .get(&key)
                    .is_some_and(|old| old.fingerprint == fingerprint)
                {
                    continue;
                }
                let provider = settings.and_then(|settings| {
                    self.prepare_structure(&entry, declaration.clone(), settings)
                        .map(Arc::new)
                        .map_err(|error| format!("{error:#}"))
                });
                self.structure_providers.insert(
                    key,
                    Prepared {
                        fingerprint,
                        provider,
                    },
                );
            }
        }
        self.structure_providers
            .retain(|key, _| wanted.contains(key));
        self.structure_providers
            .iter()
            .map(|(key, item)| (key.clone(), item.provider.clone()))
            .collect()
    }

    /// All authority checks precede pure initialization; no private snapshot, IO or process is restored.
    fn prepare_structure(
        &self,
        entry: &Installed,
        declaration: plugin_protocol::structure::Provider,
        settings: plugin_protocol::settings::Effective,
    ) -> anyhow::Result<StructureProvider> {
        anyhow::ensure!(
            entry.grants.contains("editor.read"),
            "Structure requires editor.read permission"
        );
        let assets = self
            .root
            .join("packages")
            .join(&entry.manifest.id)
            .join(&entry.digest);
        let bytes = std::fs::read(crate::instance::safe_path(
            &assets,
            entry
                .manifest
                .component
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Structure requires a component"))?,
            true,
        )?)?;
        let instance = Instance::prepare_language_worker(
            self.engine
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Plugin engine is unavailable"))?,
            &bytes,
            &entry.manifest,
            &entry.grants,
            self.environment.clone(),
            self.data_directory(&entry.manifest.id),
            assets,
            self.host_resources.clone(),
        )?;
        Ok(StructureProvider {
            owner: entry.manifest.id.clone(),
            declaration,
            instance: Mutex::new(Some(instance)),
            settings,
            entry: entry.clone(),
            root: self.root.clone(),
            active: AtomicBool::new(true),
            sequence: AtomicU64::new(1),
        })
    }

    /// Revocation covers disable/uninstall, successful cutover, rollback and configuration replacement.
    pub(super) fn retire_structure_providers(&mut self, id: &str) {
        self.structure_providers
            .retain(|key, _| !key.starts_with(&format!("{id}/")));
    }

    /// Invalidate UI-held Arcs explicitly before forgetting the selected workspace's executors.
    pub(super) fn clear_structure_providers(&mut self) {
        for prepared in self.structure_providers.values() {
            if let Ok(provider) = &prepared.provider {
                provider.retire();
            }
        }
        self.structure_providers.clear();
    }

    /// Each active pure executor owns one bounded memory resource, independent of native process counts.
    pub(super) fn structure_resource_count(&self) -> usize {
        self.structure_providers
            .values()
            .filter_map(|item| item.provider.as_ref().ok())
            .filter(|provider| provider.is_active())
            .count()
    }
}
