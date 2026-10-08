//! Argument-array CLI for complete builds, independent ZIPs and development orchestration.
use super::*;
use anyhow::Context;
use plugin_runtime::development::{self, BuildOptions, HostProcess, Project, Update};
use std::{
    collections::BTreeMap,
    io::{BufRead, Write},
    sync::mpsc,
    time::{Duration, Instant},
};

/// Handle host development commands before GUI initialization; instance mode continues into the GUI.
pub(crate) fn run_cli() -> anyhow::Result<bool> {
    let mut arguments = std::env::args_os().skip(1);
    let Some(command) = arguments.next() else {
        return Ok(false);
    };
    if command == "--plugin-dev-instance" {
        let file = arguments
            .next()
            .context("Missing development instance descriptor")?;
        anyhow::ensure!(arguments.next().is_none(), "Unexpected instance arguments");
        instance::initialize(Path::new(&file))?;
        return Ok(false);
    }
    if !["--plugin-package", "--plugin-build", "--plugin-dev"]
        .iter()
        .any(|name| command == *name)
    {
        return Ok(false);
    }
    let mut options = BuildOptions {
        release: command == "--plugin-package",
        ..Default::default()
    };
    let mut paths = Vec::new();
    let mut profile = None;
    let mut workspace = None;
    let mut watch = false;
    let mut grants = Vec::<String>::new();
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--output") => {
                options.output = Some(PathBuf::from(
                    arguments.next().context("--output needs a directory")?,
                ))
            }
            Some("--platform") => {
                options.platform = Some(
                    arguments
                        .next()
                        .context("--platform needs OS-architecture")?
                        .to_string_lossy()
                        .into_owned(),
                )
            }
            Some("--profile") => {
                profile = Some(PathBuf::from(
                    arguments.next().context("--profile needs a directory")?,
                ))
            }
            Some("--workspace") => {
                workspace = Some(PathBuf::from(
                    arguments.next().context("--workspace needs a directory")?,
                ))
            }
            Some("--grant") => grants.push(
                arguments
                    .next()
                    .context("--grant needs a permission")?
                    .to_string_lossy()
                    .into_owned(),
            ),
            Some("--watch") => watch = true,
            Some("--release") => options.release = true,
            Some("--debug") => options.release = false,
            Some(value) if value.starts_with('-') => {
                anyhow::bail!("Unknown plugin option: {value}")
            }
            _ => paths.push(PathBuf::from(argument)),
        }
    }
    anyhow::ensure!(
        !paths.is_empty() && paths.len() <= 64,
        "Specify 1–64 plugin projects"
    );
    // A command-line override is relative to the caller, unlike the project's shared default.
    if let Some(output) = options.output.as_mut() {
        if !output.is_absolute() {
            *output = std::env::current_dir()?.join(&*output);
        }
    }
    // SDK setup is needed only by Rust guests; resource-only packages work without Cargo or SDK export.
    if paths
        .iter()
        .filter_map(|path| Project::read(path).ok())
        .any(|project| project.description.wasm.is_some())
    {
        options.cargo_args = crate::sdk_export::cargo_args()?;
    }
    if command == "--plugin-dev" {
        anyhow::ensure!(
            paths.len() == 1,
            "A development instance runs one plugin project"
        );
        let project = Project::read(&paths[0])?;
        let profile =
            profile.unwrap_or(super::profile(&project.root.display().to_string(), "cli")?);
        let workspace = workspace
            .unwrap_or_else(|| project.root.clone())
            .canonicalize()?;
        develop(project, options, profile, workspace, watch, grants)?;
    } else {
        let mut failed = false;
        let mut outputs = std::collections::BTreeSet::new();
        for path in paths {
            let result = (|| -> anyhow::Result<PathBuf> {
                let project = Project::read(&path)?;
                if command == "--plugin-build" {
                    let package = project.prepare(&options, &mut print_log)?;
                    development::stage(&package, &project.root.join("target/nanobug-development"))
                } else {
                    // Detect repeated identities/output paths before a later member can replace a
                    // successful earlier member. Package versions come from the authoritative manifest.
                    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(
                        project.root.join(&project.description.manifest),
                    )?)?;
                    let output = options
                        .output
                        .clone()
                        .unwrap_or_else(|| project.root.join(&project.description.output));
                    let output = if output.is_absolute() {
                        output
                    } else {
                        project.root.join(output)
                    };
                    std::fs::create_dir_all(&output)?;
                    let output = output.canonicalize()?.join(format!(
                        "{}-{}.zip",
                        manifest["id"].as_str().context("Missing ID")?,
                        manifest["version"].as_str().context("Missing version")?
                    ));
                    anyhow::ensure!(outputs.insert(output), "Batch output collision");
                    project.package(&options, &mut print_log)
                }
            })();
            match result {
                Ok(output) => println!("OK {} -> {}", path.display(), output.display()),
                Err(error) => {
                    failed = true;
                    eprintln!("FAILED {}: {error:#}", path.display());
                }
            }
        }
        anyhow::ensure!(
            !failed,
            "One or more plugin projects failed; previous ZIPs remain unchanged for failed projects"
        );
    }
    Ok(true)
}
fn print_log(bytes: &[u8]) {
    let _ = std::io::stdout().write_all(bytes);
    let _ = std::io::stdout().flush();
}

/// The controller remains alive while the GUI runs, owning builds, watch events and the child tree.
fn develop(
    project: Project,
    options: BuildOptions,
    profile: PathBuf,
    workspace: PathBuf,
    watch: bool,
    grants: Vec<String>,
) -> anyhow::Result<()> {
    let package = project.prepare(&options, &mut print_log)?;
    let grants = grants
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    anyhow::ensure!(
        package.manifest.permissions.is_subset(&grants),
        "Development permissions need explicit --grant entries"
    );
    std::fs::create_dir_all(&profile)?;
    let profile = profile.canonicalize()?;
    // Never let an explicit CLI profile adopt the main editor's settings/history directories.
    for root in [dirs::config_dir(), dirs::data_local_dir()]
        .into_iter()
        .flatten()
    {
        if let Ok(personal) = root.join("MeEditor").canonicalize() {
            anyhow::ensure!(
                profile != personal,
                "Choose a separate development profile, not the editor's personal data root"
            );
        }
    }
    if let Some(parent) =
        std::env::var_os("ME_EDITOR_PROFILE_HOME").filter(|value| !value.is_empty())
    {
        anyhow::ensure!(
            profile != PathBuf::from(parent).canonicalize()?,
            "Development profile cannot be the parent editor profile"
        );
    }
    // A controller lease survives GUI startup and is released by the OS on a crash. A second
    // invocation cannot overwrite this live controller's descriptors or reload requests.
    let lease = instance::profile_lease(&profile)?;
    let candidate = development::stage(&package, &profile.join("candidates"))?;
    let descriptor = instance::Descriptor {
        development: package.manifest.id.clone(),
        session: format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ),
        profile: profile.clone(),
        workspace: workspace.clone(),
        candidate,
        grants,
    };
    instance::bootstrap(&descriptor, &package.manifest.id)?;
    let descriptor_path = profile.join("instance.json");
    instance::atomic_json(&descriptor_path, &descriptor)?;
    let env = BTreeMap::from([
        (
            "ME_EDITOR_PROFILE_HOME".into(),
            profile.display().to_string(),
        ),
        (
            "ME_EDITOR_PLUGIN_HOME".into(),
            profile.join("runtime").display().to_string(),
        ),
    ]);
    let mut child = HostProcess::spawn(
        &std::env::current_exe()?,
        &[
            "--plugin-dev-instance".into(),
            descriptor_path.display().to_string(),
        ],
        &workspace,
        &env,
    )?;
    let (send, receive) = mpsc::channel();
    let input = send.clone();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if input.send(line).is_err() {
                break;
            }
        }
    });
    let mut watcher = None;
    if watch {
        use notify::Watcher;
        let source_root = project.root.clone();
        let mut native =
            notify::recommended_watcher(move |event: Result<notify::Event, notify::Error>| {
                if let Ok(event) = event {
                    // Watch the root so deleted/recreated inputs stay observed; recompute rules when
                    // the description changes and ignore caches or unrelated source-tree files.
                    let inputs = Project::read(&source_root)
                        .map(|project| project.inputs())
                        .unwrap_or_else(|_| vec![source_root.join(development::DESCRIPTION)]);
                    if !matches!(event.kind, notify::EventKind::Access(_))
                        && event.paths.iter().any(|path| {
                            inputs
                                .iter()
                                .any(|input| path == input || path.starts_with(input))
                        })
                    {
                        let _ = send.send("changed".into());
                    }
                }
            })?;
        native.watch(&project.root, notify::RecursiveMode::Recursive)?;
        watcher = Some(native);
    }
    println!(
        "Development instance running. Send 'reload' to rebuild; 'stop' closes the owned instance."
    );
    let mut changed = None::<Instant>;
    let mut generation = 0u64;
    loop {
        let mut reload = false;
        for command in receive.try_iter() {
            match command.trim() {
                "reload" => reload = true,
                "changed" => changed = Some(Instant::now()),
                "stop" => {
                    let _ = child.terminate();
                    return Ok(());
                }
                _ => {}
            }
        }
        if changed.is_some_and(|time| time.elapsed() >= Duration::from_millis(400)) {
            changed = None;
            reload = true;
        }
        if reload {
            match Project::read(&project.root)
                .and_then(|current| current.prepare(&options, &mut print_log))
                .and_then(|package| {
                    descriptor.validate_candidate(&package.manifest)?;
                    let candidate = development::stage(&package, &profile.join("candidates"))?;
                    generation += 1;
                    instance::atomic_json(
                        &profile.join("reload.json"),
                        &instance::Reload {
                            session: descriptor.session.clone(),
                            generation,
                            candidate,
                        },
                    )
                }) {
                Ok(()) => println!("Development candidate prepared; waiting for activation."),
                Err(error) => eprintln!("Reload failed; previous version retained: {error:#}"),
            }
        }
        for event in child.poll()? {
            match event {
                Update::Output { bytes, .. } => print_log(&bytes),
                Update::Exited { code } => {
                    anyhow::ensure!(code == 0, "Development instance exited: {code}");
                    return Ok(());
                }
                Update::Terminated => return Ok(()),
            }
        }
        let _ = (&watcher, &lease); // Retain watcher and exclusive ownership for the whole session.
        std::thread::sleep(Duration::from_millis(25));
    }
}
