//! Private development profiles retain data while immutable candidates switch transactionally.
use super::*;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, io::Write, sync::OnceLock};
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Descriptor {
    /// Separate successive controllers using the same persistent profile.
    pub session: String,
    /// One controller substitutes one installed identity; changing the ID needs a new bootstrap.
    pub development: String,
    pub profile: PathBuf,
    pub workspace: PathBuf,
    pub candidate: PathBuf,
    pub grants: BTreeSet<String>,
}
impl Descriptor {
    /// Admit only the running development identity and its explicitly granted permissions.
    /// Reject identity or authority changes before staging a request or touching live installations.
    pub(crate) fn validate_candidate(
        &self,
        manifest: &plugin_runtime::plugin_protocol::Manifest,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            manifest.id == self.development,
            "Development identity changed; stop and restart this development configuration"
        );
        anyhow::ensure!(
            manifest.permissions.is_subset(&self.grants),
            "New permissions need confirmation; stop and restart this development configuration"
        );
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
pub(crate) struct Reload {
    pub session: String,
    pub generation: u64,
    pub candidate: PathBuf,
}
static INSTANCE: OnceLock<Descriptor> = OnceLock::new();
pub(crate) fn context() -> Option<&'static Descriptor> {
    INSTANCE.get()
}
/// Hold exclusive ownership until the returned file is dropped. Controllers and profile reset
/// share this lease so another process cannot reset a live instance; the OS releases it on a crash.
pub(crate) fn profile_lease(profile: &Path) -> anyhow::Result<std::fs::File> {
    use anyhow::Context;
    let lease = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(profile.join("controller.lock"))?;
    lease
        .try_lock()
        .context("This development profile is already running")?;
    Ok(lease)
}
/// Load only a host-generated descriptor in the explicitly selected isolated profile.
pub(crate) fn initialize(path: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        path.metadata()?.len() <= 65536,
        "Development descriptor exceeds quota"
    );
    let descriptor: Descriptor = serde_json::from_slice(&std::fs::read(path)?)?;
    let selected = std::env::var_os("ME_EDITOR_PROFILE_HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("Missing isolated profile"))?
        .canonicalize()?;
    anyhow::ensure!(
        descriptor.profile.canonicalize()? == selected
            && path.canonicalize()?.starts_with(&selected),
        "Development descriptor is outside selected profile"
    );
    anyhow::ensure!(
        descriptor.workspace.is_dir()
            && descriptor
                .candidate
                .canonicalize()?
                .starts_with(selected.join("candidates")),
        "Invalid development directories"
    );
    INSTANCE
        .set(descriptor)
        .map_err(|_| anyhow::anyhow!("Development profile already initialized"))?;
    Ok(())
}
/// Replace metadata in its private directory. Failure retains the previous complete descriptor.
pub(crate) fn atomic_json(path: &Path, value: &impl Serialize) -> anyhow::Result<()> {
    std::fs::create_dir_all(
        path.parent()
            .ok_or_else(|| anyhow::anyhow!("Missing parent"))?,
    )?;
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}
/// Load shipped packages only on first creation. Personal installation records are never copied.
pub(crate) fn bootstrap(descriptor: &Descriptor, development_id: &str) -> anyhow::Result<()> {
    #[derive(Serialize, Deserialize)]
    struct Baseline {
        development: String,
        shipped: BTreeSet<String>,
    }
    let marker = descriptor.profile.join("bootstrapped.json");
    let previous = std::fs::read(&marker)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Baseline>(&bytes).ok());
    if previous
        .as_ref()
        .is_some_and(|previous| previous.development == development_id)
    {
        return Ok(());
    }
    let environment = plugin_runtime::plugin_protocol::Environment {
        workspace: descriptor.workspace.display().to_string(),
        os: std::env::consts::OS.into(),
        ..Default::default()
    };
    let mut manager = plugin_runtime::Manager::open_with_resources(
        descriptor.profile.join("runtime"),
        environment,
        false,
        plugin_runtime::HostResources {
            sdk: Some(crate::sdk_export::descriptor().map_err(|error| format!("{error:#}"))),
            ..Default::default()
        },
    )?;
    if let Some(previous) = &previous {
        if manager.installed.contains_key(&previous.development) {
            // Editing the configuration to another project must not leave its previous development
            // version active. Its private data stays available for a later explicit return.
            manager.disable(&previous.development)?;
        }
    }
    manager.set_workspace_trust(true)?;
    let mut identities = BTreeSet::new();
    for root in super::shipped_roots() {
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries {
                let path = entry?.path();
                if path.extension().is_some_and(|extension| extension == "zip") {
                    let package = plugin_runtime::Package::read(&path)?;
                    if package.manifest.id != development_id
                        && identities.insert(package.manifest.id.clone())
                    {
                        println!("Preparing shipped plugin: {}", package.manifest.id);
                        manager.install(&package, package.manifest.permissions.clone())?;
                        if previous
                            .as_ref()
                            .is_some_and(|previous| previous.development == package.manifest.id)
                        {
                            manager.enable(&package.manifest.id)?;
                        }
                    }
                }
            }
        }
    }
    manager.shutdown();
    atomic_json(
        &marker,
        &Baseline {
            development: development_id.into(),
            shipped: identities,
        },
    )
}
/// Actor-local state accepts each reload once; invalid candidates leave installed state intact.
pub(crate) struct Loader {
    generation: Option<u64>,
    initial: bool,
}
impl Default for Loader {
    fn default() -> Self {
        Self {
            generation: None,
            initial: true,
        }
    }
}
impl Loader {
    /// Called on the existing manager actor. This never opens a second data owner or a ZIP.
    pub(crate) fn poll(&mut self, trusted: bool) -> Option<plugin_runtime::Package> {
        let Some(descriptor) = context() else {
            return None;
        };
        if !trusted {
            return None;
        }
        let request = if self.initial {
            self.initial = false;
            Some(Reload {
                session: descriptor.session.clone(),
                generation: 0,
                candidate: descriptor.candidate.clone(),
            })
        } else {
            let file = descriptor.profile.join("reload.json");
            if let Ok(metadata) = file.metadata() {
                if metadata.len() <= 65536 {
                    std::fs::read(file)
                        .ok()
                        .and_then(|bytes| serde_json::from_slice::<Reload>(&bytes).ok())
                } else {
                    None
                }
            } else {
                None
            }
        };
        let Some(request) = request.filter(|request| {
            request.session == descriptor.session && self.generation != Some(request.generation)
        }) else {
            return None;
        };
        self.generation = Some(request.generation);
        let result = (|| -> anyhow::Result<plugin_runtime::Package> {
            let path = request.candidate.canonicalize()?;
            anyhow::ensure!(
                path.starts_with(descriptor.profile.join("candidates").canonicalize()?),
                "Development candidate escapes profile"
            );
            let package = plugin_runtime::development::read_directory(&path)?;
            descriptor.validate_candidate(&package.manifest)?;
            Ok(package)
        })();
        match result {
            Ok(package) => {
                println!(
                    "Development candidate admitted (generation {}).",
                    request.generation
                );
                Some(package)
            }
            Err(error) => {
                eprintln!("Development admission failed; previous version retained: {error:#}");
                None
            }
        }
    }
}
