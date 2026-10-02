//! Host protocols borrow approved native services; retiring the package revokes every borrowed process.
use crate::process::{Spawned, spawn_piped};
use plugin_protocol::language::Provider;
use std::{
    path::PathBuf,
    process::{ChildStdin, ChildStdout},
    sync::{Arc, Mutex, Weak},
};

/// Validated immutable plan. Consumers cannot mutate the command behind an approved lease.
pub struct LanguageService {
    pub owner: String,
    pub provider: Provider,
    pub root: PathBuf,
    pub(crate) program: PathBuf,
    pub(crate) args: Vec<String>,
    state: Mutex<Lease>,
    /// Active plans pin files even if another workspace removes the package's installation record.
    pub(crate) dependencies: Vec<Arc<std::fs::File>>,
}
#[derive(Default)]
struct Lease {
    retired: bool,
    children: Vec<Weak<Mutex<Spawned>>>,
}

/// A transport may outlive a UI task, but it cannot outlive its package authority.
pub struct ServiceProcess {
    child: Arc<Mutex<Spawned>>,
}
impl ServiceProcess {
    pub fn stop(&self) {
        let mut child = self.child.lock().unwrap();
        #[cfg(windows)]
        child.job.terminate();
        let _ = child.child.kill();
        let _ = child.child.wait();
    }
}
impl Drop for ServiceProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

impl LanguageService {
    pub(crate) fn new(
        owner: String,
        provider: Provider,
        root: PathBuf,
        program: PathBuf,
        args: Vec<String>,
    ) -> Self {
        Self {
            owner,
            provider,
            root,
            program,
            args,
            state: Mutex::new(Lease::default()),
            dependencies: vec![],
        }
    }
    /// This lock serializes launch against revocation; no child can escape the retirement boundary.
    pub fn spawn(&self) -> anyhow::Result<(ServiceProcess, ChildStdin, ChildStdout)> {
        self.spawn_for_owner(&std::sync::atomic::AtomicBool::new(false))
    }
    /// A view's retirement flag shares this launch lock with stop_processes, preventing late starts.
    pub fn spawn_for_owner(
        &self,
        retired: &std::sync::atomic::AtomicBool,
    ) -> anyhow::Result<(ServiceProcess, ChildStdin, ChildStdout)> {
        let mut state = self.state.lock().unwrap();
        anyhow::ensure!(
            !state.retired && !retired.load(std::sync::atomic::Ordering::Acquire),
            "LSP provider has been retired"
        );
        state.children.retain(|child| child.strong_count() > 0);
        anyhow::ensure!(
            state.children.is_empty(),
            "LSP provider already has a live process"
        );
        let mut child = spawn_piped(&self.program, &self.args, &self.root)?;
        let input = child.child.stdin.take().expect("piped stdin");
        let output = child.child.stdout.take().expect("piped stdout");
        let mut errors = child.child.stderr.take().expect("piped stderr");
        // Drain without retaining server-controlled text; stderr must never block the JSON-RPC stream.
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut errors, &mut std::io::sink());
        });
        let child = Arc::new(Mutex::new(child));
        state.children.push(Arc::downgrade(&child));
        Ok((ServiceProcess { child }, input, output))
    }
    pub fn is_active(&self) -> bool {
        !self.state.lock().unwrap().retired
    }
    /// Consent and lifecycle diagnostics include host-owned transports as well as guest-created processes.
    pub fn process_count(&self) -> usize {
        self.state
            .lock()
            .unwrap()
            .children
            .iter()
            .filter(|child| child.strong_count() > 0)
            .count()
    }
    pub(crate) fn same_plan(&self, other: &Self) -> bool {
        self.owner == other.owner
            && self.provider == other.provider
            && self.root == other.root
            && self.program == other.program
            && self.args == other.args
    }
    pub(crate) fn retire(&self) {
        let mut state = self.state.lock().unwrap();
        state.retired = true;
        for child in state.children.drain(..).filter_map(|child| child.upgrade()) {
            ServiceProcess { child }.stop();
        }
    }
    /// Selection changes stop the current transport while keeping the installed startup plan reusable.
    pub fn stop_processes(&self) {
        let mut state = self.state.lock().unwrap();
        for child in state.children.drain(..).filter_map(|child| child.upgrade()) {
            ServiceProcess { child }.stop();
        }
    }
}
