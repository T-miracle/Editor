//! A one-use concrete consent gate and owned native execution precede immutable cache publication.
use super::*;
use plugin_protocol::dependencies::{Installer, InstallerKind};
use std::time::{Duration, Instant};

/// These are the actual resolved arguments and private target, not an unexpanded manifest template.
#[derive(Clone, Debug)]
pub struct InstallerPrompt {
    pub id: u64,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub target: PathBuf,
    pub purpose: String,
    pub project_sdk: bool,
    /// SDK downloads themselves need an opt-in before any source I/O; execution has a later concrete gate.
    pub preparation_only: bool,
    pub source: Option<String>,
}

/// A missing large SDK is only described until the user opts in; no source bytes have been read yet.
pub(super) fn authorize_sdk(
    artifact: &Artifact,
    staging: &Path,
    control: &InstallControl,
) -> anyhow::Result<()> {
    let Some(step) = artifact
        .installer
        .as_ref()
        .filter(|step| step.kind == InstallerKind::ProjectSdk)
    else {
        return Ok(());
    };
    let source = staging.canonicalize()?;
    let target = source.join(&step.target);
    control.authorize(InstallerPrompt {
        id: 0,
        program: source.join(&step.program),
        args: arguments(step, &source, &target),
        target,
        purpose: step.purpose.clone(),
        project_sdk: true,
        preparation_only: true,
        source: Some(format!(
            "{:?} · SHA-256 {}",
            artifact.source, artifact.sha256
        )),
    })
}

/// Only complete literal placeholders expand, preserving every other argument byte and boundary.
fn arguments(step: &Installer, source: &Path, target: &Path) -> Vec<String> {
    step.args
        .iter()
        .map(|arg| match arg.as_str() {
            "${target}" => target.display().to_string(),
            "${source}" => source.display().to_string(),
            _ => arg.clone(),
        })
        .collect()
}

#[derive(Default)]
pub(super) struct Approval {
    next: u64,
    pending: Option<InstallerPrompt>,
    accepted: bool,
}

impl InstallControl {
    /// Hosts explicitly opt into interactive consent; unattended calls fail closed instead of hanging.
    pub fn with_installer_prompts(mut self) -> Self {
        self.prompts = true;
        self
    }
    pub fn installer_prompt(&self) -> Option<InstallerPrompt> {
        let state = self.approval.lock().unwrap();
        if state.accepted {
            None
        } else {
            state.pending.clone()
        }
    }
    /// An approval belongs only to the displayed request; SDKs additionally need an affirmative choice.
    pub fn approve_installer(&self, id: u64, include_project_sdk: bool) -> bool {
        let mut state = self.approval.lock().unwrap();
        if state.accepted
            || !state.pending.as_ref().is_some_and(|prompt| {
                prompt.id == id && (!prompt.project_sdk || include_project_sdk)
            })
        {
            return false;
        }
        state.accepted = true;
        true
    }
    fn authorize(&self, mut prompt: InstallerPrompt) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.prompts,
            "Native installer requires concrete authorization"
        );
        let request_id = {
            let mut state = self.approval.lock().unwrap();
            anyhow::ensure!(
                state.pending.is_none(),
                "Installer authorization already pending"
            );
            state.next += 1;
            prompt.id = state.next;
            state.pending = Some(prompt);
            state.accepted = false;
            state.next
        };
        let result = (|| {
            self.stage(InstallStage::AwaitingAuthorization(request_id))?;
            let deadline = Instant::now() + Duration::from_secs(15 * 60);
            loop {
                self.check()?;
                if self.approval.lock().unwrap().accepted {
                    return Ok(());
                }
                anyhow::ensure!(
                    Instant::now() < deadline,
                    "Installer authorization timed out; retry installation"
                );
                std::thread::sleep(Duration::from_millis(25));
            }
        })();
        self.approval.lock().unwrap().pending = None;
        result
    }
}

/// The verified payload is the only program source; private targets do not imply an OS sandbox.
pub(super) fn run(
    step: &Installer,
    staging: &Path,
    control: &InstallControl,
) -> anyhow::Result<()> {
    let source = staging.canonicalize()?;
    let program = source.join(&step.program).canonicalize()?;
    anyhow::ensure!(
        program.starts_with(&source),
        "Installer escaped verified artifact"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = program.metadata()?.permissions().mode();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(mode | 0o100))?;
    }
    let program = crate::toolchains::resolve(&program.to_string_lossy())?;
    let target = source.join(&step.target);
    std::fs::create_dir_all(&target)?;
    let target = target.canonicalize()?;
    anyhow::ensure!(
        target.starts_with(&source),
        "Installer target escaped private directory"
    );
    let args = arguments(step, &source, &target);
    control.authorize(InstallerPrompt {
        id: 0,
        program: program.clone(),
        args: args.clone(),
        target,
        purpose: step.purpose.clone(),
        // The SDK opt-in already occurred before its download; this prompt grants only concrete execution.
        project_sdk: false,
        preparation_only: false,
        source: None,
    })?;
    control.stage(InstallStage::Installing(step.purpose.clone()))?;
    execute(&program, &args, &source, control)
}

/// Keep the process job through the full completion barrier, even after its root process has exited.
fn execute(
    program: &Path,
    args: &[String],
    source: &Path,
    control: &InstallControl,
) -> anyhow::Result<()> {
    let mut spawned = crate::process::spawn_piped(program, args, source)?;
    drop(spawned.child.stdin.take());
    // Drain without retaining native output in memory; installers receive no interactive input channel.
    let output = spawned.child.stdout.take().unwrap();
    let errors = spawned.child.stderr.take().unwrap();
    let stdout = std::thread::spawn(move || {
        std::io::copy(&mut std::io::BufReader::new(output), &mut std::io::sink())
    });
    let stderr = std::thread::spawn(move || {
        std::io::copy(&mut std::io::BufReader::new(errors), &mut std::io::sink())
    });
    let deadline = Instant::now() + Duration::from_secs(10 * 60);
    let result = (|| {
        loop {
            control.check()?;
            anyhow::ensure!(
                Instant::now() < deadline,
                "Native installer timed out; external effects may remain"
            );
            if let Some(status) = spawned.child.try_wait()? {
                anyhow::ensure!(
                    status.success(),
                    "Native installer failed with {status}; external effects may remain"
                );
                return control.check();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    })();
    #[cfg(windows)]
    let stopped = spawned.job.terminate_and_wait();
    let _ = spawned.child.kill();
    let _ = spawned.child.wait();
    #[cfg(windows)]
    stopped?;
    // Once the Windows job is empty, every inherited pipe and staged-file writer is gone.
    #[cfg(windows)]
    {
        let _ = stdout.join();
        let _ = stderr.join();
    }
    #[cfg(not(windows))]
    {
        drop(stdout);
        drop(stderr);
    }
    result
}
