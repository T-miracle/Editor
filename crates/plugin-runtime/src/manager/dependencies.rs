//! Installation prepares dependencies before retiring old instances; ordinary activation only reads caches.
use super::*;
use crate::{InstallControl, InstallStage};
use plugin_protocol::{dependencies::Plan, settings::Effective};

impl Manager {
    /// Keep retained-version receipts as rollback pins; unrelated workspace leases remain independently live.
    pub fn collect_dependency_cache(&self) -> anyhow::Result<usize> {
        crate::dependencies::collect(&self.root)
    }

    /// Legacy/resource-only callers share the same preparation path as detached WASM candidates.
    pub(super) fn prepare_dependencies(
        &self,
        package: &Package,
        next: Option<&mut Instance>,
        control: &InstallControl,
    ) -> anyhow::Result<()> {
        let values = next
            .as_ref()
            .map(|instance| instance.configuration.clone())
            .unwrap_or(self.saved_settings(&package.manifest)?);
        prepare_dependencies_for(
            &self.root,
            &self.environment,
            package,
            next,
            &values,
            control,
            !self.installed.contains_key(&package.manifest.id),
            &self.host_resources.logs,
        )?;
        Ok(())
    }
}

/// Capture the exact plans used by preparation so final migration cannot silently choose unprepared bytes.
pub(super) fn prepare_dependencies_for(
    root: &Path,
    environment: &Environment,
    package: &Package,
    next: Option<&mut Instance>,
    values: &Effective,
    control: &InstallControl,
    allow_optional_failure: bool,
    logs: &crate::RuntimeLogs,
) -> anyhow::Result<BTreeMap<String, Plan>> {
    control.stage(InstallStage::Preparing)?;
    if !package
        .manifest
        .permissions
        .contains("dependencies.prepare")
    {
        control.stage(InstallStage::Prepared)?;
        return Ok(BTreeMap::new());
    }
    let _guard = crate::dependencies::lock(root, control)?;
    let plans = resolve_dependency_plans(environment, package, next, values)?;
    let assets = root
        .join("packages")
        .join(&package.manifest.id)
        .join(&package.digest);
    // Keep every prepared dependency pinned until its immutable receipt has been published.
    let mut prepared = Vec::new();
    for (key, plan) in &plans {
        match crate::dependencies::prepare(root, &assets, plan, control) {
            Ok(dependency) => prepared.push(dependency),
            Err(error) => {
                // Cancellation/authority withdrawal always aborts, including an optional download.
                control.check()?;
                let optional = allow_optional_failure
                    && package.manifest.language_servers.iter().any(|provider| {
                        provider.optional_installation
                            && key == &format!("provider:{}", provider.id)
                    });
                if !optional {
                    return Err(error);
                }
                // No service is published as ready: the ordinary cache lookup later returns this
                // provider's unavailable state while resource contributions can still activate.
                logs.append(
                    &package.manifest.id,
                    crate::LogLevel::Error,
                    "dependencies/prepare",
                    format!("Optional language service is unavailable: {error:#}"),
                );
            }
        }
    }
    control.check()?;
    let identity = format!("{:x}", Sha256::digest(serde_json::to_vec(&plans)?));
    let receipt = root
        .join("dependency-receipts")
        .join(&package.manifest.id)
        .join(format!("{}-{identity}.json", package.digest));
    atomic_write(&receipt, &serde_json::to_vec(&plans)?)?;
    control.stage(InstallStage::Prepared)?;
    Ok(plans)
}

/// Resolve bounded declarations/hooks without downloading, installing, or changing a committed instance.
pub(super) fn resolve_dependency_plans(
    environment: &Environment,
    package: &Package,
    mut next: Option<&mut Instance>,
    values: &Effective,
) -> anyhow::Result<BTreeMap<String, Plan>> {
    let manifest = &package.manifest;
    let mut plans = BTreeMap::new();
    if !manifest.permissions.contains("dependencies.prepare") {
        return Ok(plans);
    }
    // Fixed services outside LSP use the same cache and process authority, without language-specific APIs.
    for (id, service) in &manifest.services {
        let used_by_lsp = manifest
            .language_servers
            .iter()
            .any(|provider| provider.service == *id || provider.alternatives.contains(id));
        if !used_by_lsp && let Some(plan) = &service.installation {
            check_installer_permission(plan, manifest)?;
            plans.insert(format!("service:{id}"), plan.clone());
        }
    }
    for provider in &manifest.language_servers {
        let candidates = std::iter::once(&provider.service)
            .chain(&provider.alternatives)
            .map(|id| (id.clone(), manifest.services[id].clone()))
            .collect();
        let proposal = if provider.hook {
            next.as_mut()
                .ok_or_else(|| anyhow::anyhow!("Dependency hook requires a prepared guest"))?
                .prepare_language(plugin_protocol::language::Context {
                    provider: provider.id.clone(),
                    workspace: environment.workspace.clone(),
                    package_root: String::new(),
                    data_root: String::new(),
                    settings: values.clone(),
                    candidates,
                })?
        } else {
            Default::default()
        };
        // Explicit local programs take precedence and never cause unnecessary downloads.
        if let Some(value) = provider
            .executable_setting
            .as_ref()
            .and_then(|key| values.get(key))
            .filter(|value| {
                matches!(
                    value.source,
                    plugin_protocol::settings::Source::User
                        | plugin_protocol::settings::Source::Project
                )
            })
        {
            let program = value
                .value
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid explicit executable"))?;
            anyhow::ensure!(
                Path::new(program).is_absolute(),
                "Explicit executable must be an absolute path"
            );
            crate::toolchains::resolve(program)?;
            continue;
        }
        let service_id = proposal.service.as_ref().unwrap_or(&provider.service);
        anyhow::ensure!(
            service_id == &provider.service || provider.alternatives.contains(service_id),
            "Undeclared dependency service"
        );
        let service = &manifest.services[service_id];
        // Dynamic native discovery has the same precedence at install time as at activation.
        if let Some(program) = &proposal.program {
            anyhow::ensure!(
                manifest.permissions.contains("process.exec"),
                "Dynamic LSP startup requires process.exec permission"
            );
            crate::toolchains::resolve(program)?;
            continue;
        }
        if let Some(plan) = proposal
            .installation
            .as_ref()
            .or(service.installation.as_ref())
        {
            check_installer_permission(plan, manifest)?;
            plans.insert(format!("provider:{}", provider.id), plan.clone());
        }
    }
    Ok(plans)
}

/// Hook-produced plans face the same permission gate as static declarations before any native step.
fn check_installer_permission(plan: &Plan, manifest: &Manifest) -> anyhow::Result<()> {
    anyhow::ensure!(
        plan.artifacts
            .iter()
            .all(|artifact| artifact.installer.is_none())
            || manifest.permissions.contains("dependencies.install"),
        "Installer permission required"
    );
    Ok(())
}
