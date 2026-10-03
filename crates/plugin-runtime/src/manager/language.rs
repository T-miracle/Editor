//! Cache bounded LSP preparation results by package/configuration/workspace, preserving unaffected services.
use super::*;
use crate::LanguageService;
use std::sync::Arc;

pub(super) struct Prepared {
    fingerprint: Vec<u8>,
    pub(super) service: Result<Arc<LanguageService>, String>,
}
impl Drop for Prepared {
    fn drop(&mut self) {
        if let Ok(service) = &self.service {
            service.retire();
        }
    }
}
impl Manager {
    /// Immutable leases are published to the host; a failed explicit path remains a visible error.
    pub fn language_services(&mut self) -> BTreeMap<String, Result<Arc<LanguageService>, String>> {
        let entries = self.published_entries();
        let mut wanted = BTreeSet::new();
        for entry in entries
            .into_iter()
            .filter(|entry| entry.enabled && entry.error.is_none())
        {
            for provider in &entry.manifest.language_servers {
                let key = format!("{}/{}", entry.manifest.id, provider.id);
                wanted.insert(key.clone());
                let values = self
                    .effective_settings(&entry.manifest.id)
                    .map_err(|error| format!("{error:#}"));
                let fingerprint = serde_json::to_vec(&(
                    &entry.digest,
                    &self.environment.workspace,
                    &values,
                    &self.host_resources.sdk,
                ))
                .unwrap();
                if self
                    .language_services
                    .get(&key)
                    .is_some_and(|old| old.fingerprint == fingerprint)
                {
                    continue;
                }
                let service = values.and_then(|values| {
                    self.prepare_language_service(&entry, provider.clone(), values)
                        .map(Arc::new)
                        .map_err(|error| format!("{error:#}"))
                });
                if let Some(old) = self.language_services.get_mut(&key) {
                    if matches!((&old.service, &service), (Ok(old), Ok(new)) if old.same_plan(new) && old.is_active())
                    {
                        old.fingerprint = fingerprint;
                        continue;
                    }
                }
                self.language_services.insert(
                    key,
                    Prepared {
                        fingerprint,
                        service,
                    },
                );
            }
        }
        self.language_services.retain(|key, _| wanted.contains(key));
        self.language_services
            .iter()
            .map(|(key, item)| (key.clone(), item.service.clone()))
            .collect()
    }

    /// The guest may select an approved plan; paths and executable overrides remain host-validated.
    fn prepare_language_service(
        &mut self,
        entry: &Installed,
        mut provider: plugin_protocol::language::Provider,
        settings: plugin_protocol::settings::Effective,
    ) -> anyhow::Result<LanguageService> {
        let candidates = std::iter::once(&provider.service)
            .chain(&provider.alternatives)
            .map(|id| {
                anyhow::ensure!(
                    entry.grants.contains(&format!("process.service.{id}")),
                    "LSP service permission denied"
                );
                Ok((
                    id.clone(),
                    entry
                        .manifest
                        .services
                        .get(id)
                        .ok_or_else(|| anyhow::anyhow!("Missing LSP service"))?
                        .clone(),
                ))
            })
            .collect::<anyhow::Result<BTreeMap<_, _>>>()?;
        let proposal = if provider.hook {
            self.live
                .get_mut(&entry.manifest.id)
                .ok_or_else(|| anyhow::anyhow!("LSP hook instance is unavailable"))?
                .prepare_language(plugin_protocol::language::Context {
                    provider: provider.id.clone(),
                    workspace: self.environment.workspace.clone(),
                    settings: settings.clone(),
                    candidates: candidates.clone(),
                })?
        } else {
            Default::default()
        };
        let mut service = candidates
            .get(proposal.service.as_ref().unwrap_or(&provider.service))
            .ok_or_else(|| anyhow::anyhow!("LSP hook selected an undeclared service"))?
            .clone();
        if let Some(plan) = proposal.installation {
            anyhow::ensure!(
                entry.grants.contains("dependencies.prepare"),
                "Dependency preparation permission required"
            );
            service.installation = Some(plan);
        }
        if proposal.program.is_some() || proposal.args.is_some() {
            anyhow::ensure!(
                entry.manifest.permissions.contains("process.exec")
                    && entry.grants.contains("process.exec"),
                "Dynamic LSP startup requires process.exec permission"
            );
            if let Some(program) = proposal.program {
                service.program = program;
                service.installation = None;
                service.search_paths.clear();
            }
            if let Some(args) = proposal.args {
                service.args = args;
            }
            anyhow::ensure!(service.valid(), "Invalid dynamic LSP startup");
        }
        if let Some(key) = &provider.executable_setting {
            if let Some(value) = settings.get(key).filter(|value| {
                matches!(
                    value.source,
                    plugin_protocol::settings::Source::User
                        | plugin_protocol::settings::Source::Project
                )
            }) {
                service.program = value
                    .value
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Invalid explicit executable"))?
                    .into();
                service.installation = None;
                // An explicit user selection must fail visibly instead of silently falling back.
                service.search_paths.clear();
                anyhow::ensure!(
                    Path::new(&service.program).is_absolute(),
                    "Explicit executable must be an absolute path"
                );
            }
        }
        let workspace = Path::new(&self.environment.workspace).canonicalize()?;
        let root = if let Some(relative) = proposal.project_root {
            crate::package::validate_relative(&relative)?;
            workspace.join(relative).canonicalize()?
        } else {
            workspace.clone()
        };
        anyhow::ensure!(
            root.is_dir() && root.starts_with(&workspace),
            "LSP project root escaped workspace"
        );
        if let Some(options) = proposal.initialization_options {
            provider.initialization_options = options;
        }
        if let Some(configuration) = proposal.configuration {
            provider.configuration = configuration;
        }
        anyhow::ensure!(
            provider.valid(),
            "LSP hook returned invalid or oversized configuration"
        );
        let prepared = service
            .installation
            .as_ref()
            .map(|plan| crate::dependencies::cached(&self.root, plan))
            .transpose()?;
        let program = if let Some(prepared) = &prepared {
            prepared.program.clone()
        } else {
            crate::toolchains::resolve_service(&service)?
        };
        let args = if let Some(prepared) = &prepared {
            prepared.args(&service.args)?
        } else {
            service.args
        };
        let mut language_service =
            LanguageService::new(entry.manifest.id.clone(), provider, root, program, args);
        language_service.dependencies = prepared.map(|prepared| prepared.locks).unwrap_or_default();
        Ok(language_service)
    }
    /// Called only on successful replacement or revocation; failed configuration preserves old leases.
    pub(super) fn retire_language_services(&mut self, id: &str) {
        self.language_services
            .retain(|key, _| !key.starts_with(&format!("{id}/")));
    }
}
