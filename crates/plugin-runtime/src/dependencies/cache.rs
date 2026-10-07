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
                // Interpreter/module loaders can reject Windows device prefixes in argv. Boundary
                // checks keep canonical paths; the child spelling must re-resolve to the same target.
                #[cfg(windows)]
                let target = crate::toolchains::child_path_spelling(target)?;
                Ok(target.display().to_string())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Child-friendly argument spelling preserves the managed target, literals and traversal boundary.
    #[test]
    fn dependency_arguments_keep_ownership_and_ordinary_literals() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("server");
        std::fs::create_dir(&root).unwrap();
        let script = root.join("server.cjs");
        std::fs::write(&script, "// controlled service module").unwrap();
        let prepared = Prepared {
            program: PathBuf::new(),
            roots: BTreeMap::from([("server".into(), root)]),
            locks: Vec::new(),
        };
        let literal = r"\\?\C:\user\literal\argument";
        let args = prepared
            .args(&["${dependency:server}/server.cjs".into(), literal.into()])
            .unwrap();
        assert_eq!(
            Path::new(&args[0]).canonicalize().unwrap(),
            script.canonicalize().unwrap()
        );
        assert_eq!(
            args[1], literal,
            "ordinary arguments cannot be interpreted as paths"
        );
        #[cfg(windows)]
        assert!(
            !args[0].starts_with(r"\\?\"),
            "interpreters must receive the verified ordinary spelling"
        );
        for denied in [
            "${dependency:server}/../outside.cjs",
            "${dependency:unknown}/server.cjs",
            "${dependency:server}/server.cjs:stream",
        ] {
            assert!(prepared.args(&[denied.into()]).is_err(), "{denied}");
        }
        #[cfg(windows)]
        assert!(
            crate::toolchains::child_path_spelling(script).is_err(),
            "an unverified target spelling must be refused"
        );
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
        super::installer::authorize_sdk(artifact, staging.path(), control)?;
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
        if let Some(step) = &artifact.installer {
            super::installer::run(step, staging.path(), control)?;
        }
        // A successful installer must actually produce this plan's service before it becomes reusable.
        let (executable_id, executable_path) = plan.executable.split_once('/').unwrap();
        if executable_id == artifact.id {
            let executable = staging.path().join(executable_path).canonicalize()?;
            anyhow::ensure!(
                executable.starts_with(staging.path().canonicalize()?),
                "Installed executable escaped dependency"
            );
            crate::toolchains::resolve(&executable.to_string_lossy())?;
        }
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
