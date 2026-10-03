//! Immutable private dependencies are prepared before cutover and pinned independently of native processes.
use anyhow::Context;
use plugin_protocol::dependencies::{Artifact, Format, Plan, Source};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
mod cache;
mod download;
mod installer;
pub(crate) use cache::{cached, collect, lock, prepare};
pub use installer::InstallerPrompt;

/// Progress reports preparation honestly; only the protocol client can report language-service readiness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallStage {
    Migrating,
    Committing,
    Committed,
    Preparing,
    Downloading(String),
    Verifying(String),
    Extracting(String),
    AwaitingAuthorization(u64),
    Installing(String),
    Prepared,
}

/// Cancellation is owned by the UI and remains callable while the package worker is occupied.
#[derive(Clone)]
pub struct InstallControl {
    cancelled: Arc<AtomicBool>,
    report: Arc<dyn Fn(InstallStage) + Send + Sync>,
    prompts: bool,
    approval: Arc<std::sync::Mutex<installer::Approval>>,
}
impl Default for InstallControl {
    fn default() -> Self {
        Self::new(|_| {})
    }
}
impl InstallControl {
    pub fn new(report: impl Fn(InstallStage) + Send + Sync + 'static) -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            report: Arc::new(report),
            prompts: false,
            approval: Default::default(),
        }
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub(crate) fn check(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.cancelled.load(Ordering::Acquire),
            "Dependency preparation cancelled"
        );
        Ok(())
    }
    pub(crate) fn stage(&self, stage: InstallStage) -> anyhow::Result<()> {
        (self.report)(stage);
        self.check()
    }
}

/// A graph is validated before any file or network access; DFS also rejects recursive installation plans.
pub(crate) fn validate(plan: &Plan) -> anyhow::Result<Vec<&Artifact>> {
    anyhow::ensure!(
        !plan.artifacts.is_empty() && plan.artifacts.len() <= 32,
        "Dependency count must be 1..32"
    );
    anyhow::ensure!(
        serde_json::to_vec(plan)?.len() <= 64 * 1024,
        "Dependency plan exceeds 64 KiB"
    );
    let mut artifacts = BTreeMap::new();
    for artifact in &plan.artifacts {
        anyhow::ensure!(
            !artifact.id.is_empty()
                && artifact.id.len() <= 64
                && artifact
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
            "Invalid dependency ID"
        );
        anyhow::ensure!(
            artifacts.insert(artifact.id.as_str(), artifact).is_none(),
            "Duplicate dependency ID"
        );
        anyhow::ensure!(
            !artifact.version.is_empty()
                && artifact.version.len() <= 128
                && !artifact.platform.is_empty()
                && artifact.platform.len() <= 64,
            "Dependency version/platform required"
        );
        anyhow::ensure!(
            artifact.sha256.len() == 64 && artifact.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "Dependency requires SHA-256"
        );
        anyhow::ensure!(artifact.requires.len() <= 32, "Too many dependency edges");
        match &artifact.source {
            Source::Package { path } => crate::package::validate_relative(path)?,
            Source::Local { path } => anyhow::ensure!(
                Path::new(path).is_absolute(),
                "Local dependency requires an absolute path"
            ),
            Source::Url { url } => {
                let uri: ureq::http::Uri = url.parse()?;
                let host = uri.host().unwrap_or_default().trim_matches(['[', ']']);
                let local = host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|address| address.is_loopback());
                anyhow::ensure!(
                    uri.authority()
                        .is_some_and(|authority| !authority.as_str().contains('@'))
                        && (uri.scheme_str() == Some("https")
                            || (uri.scheme_str() == Some("http") && local)),
                    "Dependency URL requires HTTPS without credentials (HTTP loopback is allowed)"
                );
            }
        }
        if let Format::File { path } = &artifact.format {
            crate::package::validate_relative(path)?;
        }
        if let Some(installer) = &artifact.installer {
            crate::package::validate_relative(&installer.program)?;
            crate::package::validate_relative(&installer.target)?;
            anyhow::ensure!(
                !installer.purpose.trim().is_empty() && installer.purpose.len() <= 2048,
                "Installer purpose required"
            );
            anyhow::ensure!(
                installer.args.len() <= 128 && installer.args.iter().all(|arg| !arg.contains('\0')),
                "Invalid installer arguments"
            );
        }
    }
    let (id, path) = plan
        .executable
        .split_once('/')
        .context("Executable must start with artifact ID")?;
    anyhow::ensure!(artifacts.contains_key(id), "Executable dependency missing");
    crate::package::validate_relative(path)?;
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut ordered = Vec::new();
    for id in artifacts.keys() {
        visit(id, &artifacts, &mut visiting, &mut visited, &mut ordered)?;
    }
    Ok(ordered)
}
fn visit<'a>(
    id: &'a str,
    artifacts: &BTreeMap<&'a str, &'a Artifact>,
    visiting: &mut BTreeSet<&'a str>,
    visited: &mut BTreeSet<&'a str>,
    ordered: &mut Vec<&'a Artifact>,
) -> anyhow::Result<()> {
    if visited.contains(id) {
        return Ok(());
    }
    anyhow::ensure!(visiting.insert(id), "Dependency installation cycle at {id}");
    let artifact = artifacts
        .get(id)
        .context("Undeclared dependency reference")?;
    for dependency in &artifact.requires {
        visit(dependency, artifacts, visiting, visited, ordered)?;
    }
    visiting.remove(id);
    visited.insert(id);
    ordered.push(artifact);
    Ok(())
}

/// Content and extraction identity share the cache; source location does not prevent offline reuse.
fn key(artifact: &Artifact) -> String {
    let extracted = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&(
                &artifact.version,
                &artifact.platform,
                &artifact.sha256,
                &artifact.format
            ))
            .unwrap()
        )
    );
    // Preserve existing download-only identities while keeping distinct approved installers isolated.
    match &artifact.installer {
        None => extracted,
        Some(installer) => format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&(extracted, installer)).unwrap())
        ),
    }
}

/// Bound actual bytes as well as metadata; a forged size cannot consume unbounded disk or memory.
fn copy(
    mut input: impl Read,
    mut output: impl Write,
    limit: u64,
    control: &InstallControl,
) -> anyhow::Result<u64> {
    let mut total = 0;
    let mut buffer = [0; 32768];
    loop {
        control.check()?;
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        anyhow::ensure!(total <= limit, "Dependency size quota exceeded");
        output.write_all(&buffer[..count])?;
    }
    control.check()?;
    Ok(total)
}

fn read_source(
    artifact: &Artifact,
    assets: &Path,
    output: &mut File,
    control: &InstallControl,
) -> anyhow::Result<()> {
    control.stage(InstallStage::Downloading(artifact.id.clone()))?;
    let path = match &artifact.source {
        Source::Package { path } => {
            let path = assets.join(path).canonicalize()?;
            anyhow::ensure!(
                path.starts_with(assets.canonicalize()?),
                "Dependency escaped package"
            );
            path
        }
        Source::Local { path } => PathBuf::from(path),
        Source::Url { url } => return download::download(url, output, control),
    };
    copy(File::open(path)?, output, 128 * 1024 * 1024, control)?;
    Ok(())
}

/// ZIP paths, aliases and symlinks are untrusted even after authenticating the archive bytes.
fn extract(
    artifact: &Artifact,
    archive: &Path,
    directory: &Path,
    control: &InstallControl,
) -> anyhow::Result<()> {
    control.stage(InstallStage::Extracting(artifact.id.clone()))?;
    match &artifact.format {
        Format::File { path } => {
            let target = directory.join(path);
            std::fs::create_dir_all(target.parent().unwrap())?;
            copy(
                File::open(archive)?,
                File::create(target)?,
                128 * 1024 * 1024,
                control,
            )?;
        }
        Format::Zip => {
            let mut zip = zip::ZipArchive::new(File::open(archive)?)?;
            anyhow::ensure!(zip.len() <= 10000, "Dependency ZIP has too many entries");
            let mut total = 0;
            for index in 0..zip.len() {
                let mut entry = zip.by_index(index)?;
                if entry.is_dir() {
                    continue;
                }
                crate::package::validate_relative(entry.name())?;
                anyhow::ensure!(!entry.is_symlink(), "Dependency ZIP symlinks forbidden");
                let target = directory.join(entry.name());
                std::fs::create_dir_all(target.parent().unwrap())?;
                let file = File::options().write(true).create_new(true).open(&target)?;
                total += copy(&mut entry, file, 256 * 1024 * 1024 - total, control)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(
                        target,
                        std::fs::Permissions::from_mode(entry.unix_mode().unwrap_or(0o644) & 0o777),
                    )?;
                }
            }
        }
    }
    Ok(())
}
