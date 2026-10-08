//! Build immutable candidates, validate every reference, then publish an atomic ZIP or directory.
use super::*;
use crate::{
    Package,
    package::{atomic_write, validate_relative},
};
use anyhow::Context;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::Duration,
};

impl Project {
    /// Build and inspect an immutable candidate without any ZIP or installation side effects.
    /// Logs stream through the callback; missing tools, cancellation and invalid assets return errors.
    pub fn prepare(
        &self,
        options: &BuildOptions,
        log: &mut dyn FnMut(&[u8]),
    ) -> anyhow::Result<Package> {
        check_cancel(options)?;
        let platform = options.platform.clone().unwrap_or_else(super::platform);
        let profile = if options.release { "release" } else { "debug" };
        // Keep compiler paths short on Windows: deeply nested worktrees can exceed MSVC's
        // response-file/object path limit even when the filesystem accepts extended paths.
        let key = format!(
            "{:x}",
            Sha256::digest(self.root.to_string_lossy().as_bytes())
        );
        let build = dirs::cache_dir()
            .context("Build cache directory unavailable")?
            .join("MeEditor/plugin-build")
            .join(&key[..16])
            .join(&platform)
            .join(profile);
        validate_relative(&platform)?;
        std::fs::create_dir_all(&build)?;
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&read_asset(&self.root, &self.description.manifest)?)?;
        // A manually chosen output can be inside a resource directory. Never recursively embed
        // the previous archive in its replacement; all other explicitly declared assets remain.
        let output = options
            .output
            .clone()
            .unwrap_or_else(|| self.root.join(&self.description.output));
        let output = if output.is_absolute() {
            output
        } else {
            self.root.join(output)
        };
        let output = output.canonicalize().unwrap_or(output);
        let excluded = output.join(format!(
            "{}-{}.zip",
            manifest["id"].as_str().context("Missing ID")?,
            manifest["version"].as_str().context("Missing version")?
        ));
        let mut files = BTreeMap::new();
        if let Some(wasm) = &self.description.wasm {
            let component = manifest["component"]
                .as_str()
                .context("WASM build requires a manifest component path")?
                .to_owned();
            validate_relative(&component)?;
            let cargo_manifest = checked_path(&self.root, &wasm.manifest)?;
            let mut args = options.cargo_args.clone();
            args.extend([
                "build".into(),
                "--manifest-path".into(),
                cargo_manifest.display().to_string(),
                "--target".into(),
                "wasm32-wasip2".into(),
                "--target-dir".into(),
                build.join("wasm").display().to_string(),
                "--message-format=json-render-diagnostics".into(),
            ]);
            if options.release {
                args.push("--release".into());
            }
            let output = command("cargo", &args, &self.root, options, log)?;
            let mut artifacts = std::collections::BTreeSet::new();
            for line in output.split(|byte| *byte == b'\n') {
                let Ok(message) = serde_json::from_slice::<serde_json::Value>(line) else {
                    continue;
                };
                if message["reason"] == "compiler-artifact"
                    && message["target"]["crate_types"]
                        .as_array()
                        .is_some_and(|types| types.iter().any(|kind| kind == "cdylib"))
                    && wasm
                        .library
                        .as_ref()
                        .is_none_or(|library| message["target"]["name"].as_str() == Some(library))
                {
                    for file in message["filenames"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|value| value.as_str())
                    {
                        if file.ends_with(".wasm") {
                            artifacts.insert(PathBuf::from(file));
                        }
                    }
                }
            }
            anyhow::ensure!(
                artifacts.len() == 1,
                "Cargo must report exactly one matching WASM component"
            );
            let artifact = artifacts.into_iter().next().unwrap().canonicalize()?;
            anyhow::ensure!(
                artifact.starts_with(build.canonicalize()?),
                "Cargo artifact escapes the managed build directory"
            );
            files.insert(component, read_bounded(&artifact)?);
        }
        for step in &self.description.native {
            anyhow::ensure!(
                step.platform == platform,
                "Native build requires {}, selected {platform}",
                step.platform
            );
            anyhow::ensure!(
                platform == super::platform(),
                "Native cross-compilation is not declared by this project"
            );
            let args = step
                .args
                .iter()
                .map(|arg| {
                    arg.replace("{project}", &self.root.display().to_string())
                        .replace("{build}", &build.display().to_string())
                        .replace("{profile}", profile)
                })
                .collect::<Vec<_>>();
            command(&step.program, &args, &self.root, options, log)?;
        }
        for asset in &self.description.assets {
            check_cancel(options)?;
            let (root, source) = if let Some(source) = asset.source.strip_prefix("{build}/") {
                (&build, source)
            } else {
                (&self.root, asset.source.as_str())
            };
            collect(
                root,
                source,
                &asset.destination,
                &mut files,
                Some(&excluded),
            )?;
        }
        // Generated files may never overwrite the authoritative distribution manifest.
        anyhow::ensure!(
            !files.contains_key("manifest.json"),
            "Do not include manifest.json in assets; it is generated from the declared manifest"
        );
        if let Some(services) = manifest
            .get_mut("services")
            .and_then(serde_json::Value::as_object_mut)
        {
            for service in services.values_mut() {
                if let Some(artifacts) = service
                    .get_mut("installation")
                    .and_then(|plan| plan.get_mut("artifacts"))
                    .and_then(serde_json::Value::as_array_mut)
                {
                    for artifact in artifacts {
                        if artifact["source"]["kind"] == "package" {
                            let path = artifact["source"]["path"]
                                .as_str()
                                .context("Missing native artifact path")?;
                            let bytes = files
                                .get(path)
                                .with_context(|| format!("Missing native artifact: {path}"))?;
                            artifact["sha256"] = format!("{:x}", Sha256::digest(bytes)).into();
                        }
                    }
                }
            }
        }
        files.insert(
            "manifest.json".into(),
            serde_json::to_vec_pretty(&manifest)?,
        );
        check_cancel(options)?;
        let mut package = Package::from_files(files)?;
        package.source = Some(self.root.display().to_string());
        Ok(package)
    }
    /// Prepare and atomically replace a versioned ZIP. A failure leaves an existing ZIP unchanged.
    /// The CLI/local override wins over the shared relative default (project root by default).
    pub fn package(
        &self,
        options: &BuildOptions,
        log: &mut dyn FnMut(&[u8]),
    ) -> anyhow::Result<PathBuf> {
        let package = self.prepare(options, log)?;
        let output = options
            .output
            .clone()
            .unwrap_or_else(|| self.root.join(&self.description.output));
        let output = if output.is_absolute() {
            output
        } else {
            self.root.join(output)
        };
        std::fs::create_dir_all(&output)?;
        let output = output.canonicalize()?;
        let target = output.join(format!(
            "{}-{}.zip",
            package.manifest.id, package.manifest.version
        ));
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let format = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in &package.files {
            zip.start_file(name, format)?;
            zip.write_all(bytes)?;
        }
        let bytes = zip.finish()?.into_inner();
        // The resulting ZIP is inspected too, before replacing the previous distributable.
        Package::from_bytes(&bytes)?;
        check_cancel(options)?;
        atomic_write(&target, &bytes)?;
        Ok(target)
    }
}

/// Persist a content-addressed development directory; no ZIP is produced, even internally.
pub fn stage(package: &Package, root: &Path) -> anyhow::Result<PathBuf> {
    let target = root.join(&package.digest);
    package.extract(&target)?;
    Ok(target)
}
/// Read a development directory with the exact installed-package admission checks.
pub fn read_directory(root: &Path) -> anyhow::Result<Package> {
    let root = root.canonicalize()?;
    let mut files = BTreeMap::new();
    collect(&root, "manifest.json", "manifest.json", &mut files, None)?;
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name != "manifest.json" {
            collect(&root, &name, &name, &mut files, None)?;
        }
    }
    let mut package = Package::from_files(files)?;
    package.source = Some(root.display().to_string());
    Ok(package)
}
fn check_cancel(options: &BuildOptions) -> anyhow::Result<()> {
    anyhow::ensure!(
        !options.cancelled.load(Ordering::Acquire),
        "Plugin build cancelled"
    );
    Ok(())
}
/// Reject redirections and path escapes before reading a project-controlled source.
fn checked_path(root: &Path, relative: &str) -> anyhow::Result<PathBuf> {
    validate_relative(relative)?;
    let mut path = root.to_owned();
    for piece in Path::new(relative).components() {
        path.push(piece);
        anyhow::ensure!(
            !std::fs::symlink_metadata(&path)?.file_type().is_symlink(),
            "Plugin sources cannot contain symlinks"
        );
    }
    let path = path.canonicalize()?;
    anyhow::ensure!(
        path.starts_with(root.canonicalize()?),
        "Plugin source escapes the project"
    );
    Ok(path)
}
fn read_bounded(path: &Path) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        path.metadata()?.len() <= 64 * 1024 * 1024,
        "Plugin asset exceeds quota"
    );
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() <= 64 * 1024 * 1024,
        "Plugin asset exceeds quota"
    );
    Ok(bytes)
}
fn read_asset(root: &Path, relative: &str) -> anyhow::Result<Vec<u8>> {
    read_bounded(&checked_path(root, relative)?)
}
fn collect(
    root: &Path,
    source: &str,
    destination: &str,
    files: &mut BTreeMap<String, Vec<u8>>,
    excluded: Option<&Path>,
) -> anyhow::Result<()> {
    validate_relative(destination)?;
    let path = checked_path(root, source)?;
    if excluded == Some(path.as_path()) {
        return Ok(());
    }
    if path.is_dir() {
        let mut children = std::fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
        children.sort_by_key(|entry| entry.file_name());
        for child in children {
            let name = child.file_name().to_string_lossy().into_owned();
            collect(
                root,
                &format!("{source}/{name}"),
                &format!("{destination}/{name}"),
                files,
                excluded,
            )?;
        }
    } else {
        anyhow::ensure!(files.len() < 512, "Too many plugin files");
        anyhow::ensure!(
            !files.contains_key(destination),
            "Duplicate package destination: {destination}"
        );
        let bytes = read_bounded(&path)?;
        anyhow::ensure!(
            files.values().map(Vec::len).sum::<usize>() + bytes.len() <= 128 * 1024 * 1024,
            "Expanded package exceeds quota"
        );
        files.insert(destination.into(), bytes);
    }
    Ok(())
}

/// No shell or archived helpers are involved; the existing process module owns descendants.
fn command(
    program: &str,
    args: &[String],
    root: &Path,
    options: &BuildOptions,
    log: &mut dyn FnMut(&[u8]),
) -> anyhow::Result<Vec<u8>> {
    let name = Path::new(program)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(program)
        .to_ascii_lowercase();
    anyhow::ensure!(
        ![
            "powershell",
            "pwsh",
            "cmd",
            "sh",
            "bash",
            "python",
            "python3",
            "py"
        ]
        .contains(&name.as_str()),
        "Plugin builds invoke native tools directly, not scripts or shells"
    );
    anyhow::ensure!(
        !args.iter().any(|arg| {
            let normalized = arg.replace('\\', "/").to_ascii_lowercase();
            normalized.contains("/.codex/workspaces/nanobug/scripts/")
                || normalized.starts_with("scripts/")
        }),
        "Archived packaging scripts cannot be called"
    );
    log(format!("Building: {program}\n").as_bytes());
    let mut child = HostProcess::spawn(Path::new(program), args, root, &BTreeMap::new())
        .with_context(|| {
            format!("Could not start {program}; install/configure the required toolchain")
        })?;
    let mut output = Vec::new();
    loop {
        if options.cancelled.load(Ordering::Acquire) {
            let _ = child.terminate();
            check_cancel(options)?;
        }
        for event in child.poll()? {
            match event {
                Update::Output { stream, bytes } => {
                    if matches!(stream, Stream::Stdout) {
                        anyhow::ensure!(
                            output.len() + bytes.len() <= 16 * 1024 * 1024,
                            "Build output exceeds quota"
                        );
                        output.extend_from_slice(&bytes);
                    }
                    log(&bytes);
                }
                Update::Exited { code } => {
                    anyhow::ensure!(code == 0, "{program} failed with exit code {code}");
                    return Ok(output);
                }
                Update::Terminated => anyhow::bail!("Build process terminated"),
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
