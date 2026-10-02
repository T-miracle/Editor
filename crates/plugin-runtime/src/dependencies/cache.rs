//! OS locks protect preparation, active leases and garbage collection across editor windows/processes.
use super::*;
use std::io::{Seek, SeekFrom};

/// Locks live outside removable payload directories so Windows can collect an unused version safely.
pub(crate) struct Prepared {
    pub program: PathBuf,
    pub roots: BTreeMap<String, PathBuf>,
    pub locks: Vec<Arc<File>>,
}
impl Prepared {
    /// Only explicit whole-argument dependency references expand; ordinary user arguments stay literal.
    pub fn args(&self, args: &[String]) -> anyhow::Result<Vec<String>> {
        args.iter()
            .map(|arg| {
                let Some(reference) = arg.strip_prefix("${dependency:") else {
                    return Ok(arg.clone());
                };
                let (id, path) = reference
                    .split_once("}/")
                    .context("Malformed dependency argument")?;
                crate::package::validate_relative(path)?;
                let root = self.roots.get(id).context("Unknown dependency argument")?;
                let target = root.join(path).canonicalize()?;
                anyhow::ensure!(
                    target.starts_with(root.canonicalize()?),
                    "Argument escaped dependency"
                );
                Ok(target.display().to_string())
            })
            .collect()
    }
}

/// The global lock covers preparation through receipt publication, preventing collection of a candidate.
pub(crate) fn lock(root: &Path, control: &InstallControl) -> anyhow::Result<File> {
    std::fs::create_dir_all(root.join("dependencies/locks"))?;
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("dependencies/prepare.lock"))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    loop {
        control.check()?;
        match file.try_lock() {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock) => {
                anyhow::ensure!(
                    std::time::Instant::now() < deadline,
                    "Dependency cache busy; retry shortly"
                );
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
        }
    }
    Ok(file)
}
fn lease(root: &Path, key: &str) -> anyhow::Result<File> {
    Ok(File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("dependencies/locks").join(key))?)
}

/// Preparation never changes a complete cache entry. Only a verified staging directory is published.
pub(crate) fn prepare(
    root: &Path,
    assets: &Path,
    plan: &Plan,
    control: &InstallControl,
) -> anyhow::Result<Prepared> {
    for artifact in validate(plan)? {
        control.check()?;
        anyhow::ensure!(
            artifact.platform == format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            "Unsupported dependency platform: {}",
            artifact.platform
        );
        let key = key(artifact);
        let target = root.join("dependencies/versions").join(&key);
        if target.join(".complete").is_file() {
            continue;
        }
        std::fs::create_dir_all(target.parent().unwrap())?;
        let staging = tempfile::tempdir_in(target.parent().unwrap())?;
        let archive = tempfile::NamedTempFile::new_in(root.join("dependencies"))?;
        let mut file = archive.reopen()?;
        read_source(artifact, assets, &mut file, control)?;
        control.stage(InstallStage::Verifying(artifact.id.clone()))?;
        file.seek(SeekFrom::Start(0))?;
        let mut hash = Sha256::new();
        copy(&mut file, &mut hash, 128 * 1024 * 1024, control)?;
        anyhow::ensure!(
            format!("{:x}", hash.finalize()).eq_ignore_ascii_case(&artifact.sha256),
            "Dependency checksum mismatch: {}",
            artifact.id
        );
        extract(artifact, archive.path(), staging.path(), control)?;
        std::fs::write(staging.path().join(".complete"), b"verified")?;
        control.check()?;
        std::fs::rename(staging.path(), target)?;
    }
    cached(root, plan)
}

/// Runtime activation only reads an already prepared plan; it cannot trigger an unconfirmed download.
pub(crate) fn cached(root: &Path, plan: &Plan) -> anyhow::Result<Prepared> {
    let mut roots = BTreeMap::new();
    let mut locks = Vec::new();
    for artifact in validate(plan)? {
        let key = key(artifact);
        let file = lease(root, &key)?;
        file.lock_shared()?;
        let directory = root.join("dependencies/versions").join(key);
        anyhow::ensure!(
            directory.join(".complete").is_file(),
            "Dependency not prepared; reinstall to retry: {}",
            artifact.id
        );
        roots.insert(artifact.id.clone(), directory);
        locks.push(Arc::new(file));
    }
    let (id, path) = plan.executable.split_once('/').unwrap();
    let directory = &roots[id];
    let executable = directory.join(path).canonicalize()?;
    anyhow::ensure!(
        executable.starts_with(directory.canonicalize()?),
        "Executable escaped dependency"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = executable.metadata()?.permissions().mode();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(mode | 0o100))?;
    }
    let program = crate::toolchains::resolve(&executable.to_string_lossy())?;
    Ok(Prepared {
        program,
        roots,
        locks,
    })
}

/// Receipts from every retained package version pin rollback dependencies; active leases add OS locks.
pub(crate) fn collect(root: &Path) -> anyhow::Result<usize> {
    let _guard = lock(root, &InstallControl::default())?;
    let mut pinned = BTreeSet::new();
    let packages = root.join("dependency-receipts");
    if packages.exists() {
        for package in std::fs::read_dir(packages)? {
            for receipt in std::fs::read_dir(package?.path())? {
                let plans: BTreeMap<String, Plan> =
                    serde_json::from_slice(&std::fs::read(receipt?.path())?)?;
                for plan in plans.values() {
                    for artifact in validate(plan)? {
                        pinned.insert(key(artifact));
                    }
                }
            }
        }
    }
    let versions = root.join("dependencies/versions");
    if !versions.exists() {
        return Ok(0);
    }
    let mut removed = 0;
    for entry in std::fs::read_dir(&versions)? {
        let entry = entry?;
        let key = entry.file_name().to_string_lossy().to_string();
        if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) || pinned.contains(&key) {
            continue;
        }
        let file = lease(root, &key)?;
        if file.try_lock().is_err() {
            continue;
        }
        let target = entry.path().canonicalize()?;
        anyhow::ensure!(
            target.starts_with(versions.canonicalize()?),
            "Invalid dependency cache path"
        );
        std::fs::remove_dir_all(target)?;
        removed += 1;
    }
    Ok(removed)
}
