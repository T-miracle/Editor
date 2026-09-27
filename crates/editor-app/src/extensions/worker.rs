//! A single worker owns plugin stores; the UI thread never compiles or executes WASM.
use plugin_runtime::{Installed, Manager, Package, plugin_protocol::*};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

pub(super) enum Work {
    Inspect(PathBuf),
    Install(Package),
    Enable(String),
    Disable(String),
    Uninstall(String, bool),
    Event(String, Event),
    Shutdown(Option<futures::channel::oneshot::Sender<()>>),
}
impl Work {
    /// Identify the operation whose button should show loading while queued or running.
    fn lifecycle(&self) -> Option<OperationProgress> {
        match self {
            Self::Install(package) => Some(OperationProgress {
                id: package.manifest.id.clone(),
                action: LifecycleAction::Install,
                delete_data: None,
            }),
            Self::Enable(id) => Some(OperationProgress {
                id: id.clone(),
                action: LifecycleAction::Enable,
                delete_data: None,
            }),
            Self::Uninstall(id, delete_data) => Some(OperationProgress {
                id: id.clone(),
                action: LifecycleAction::Uninstall,
                delete_data: Some(*delete_data),
            }),
            _ => None,
        }
    }
}
/// The operation type maps directly to the loading button in plugin management.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LifecycleAction {
    Install,
    Enable,
    Uninstall,
}
/// Shared worker state remains visible even while a synchronous manager call is running.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct OperationProgress {
    pub id: String,
    pub action: LifecycleAction,
    pub delete_data: Option<bool>,
}
#[derive(Default)]
pub(super) struct Published {
    pub entries: Vec<Installed>,
    pub startup: BTreeMap<String, String>,
    pub scenes: BTreeMap<String, Arc<Scene>>,
    pub effects: Vec<(String, Request)>,
    pub pending: Option<Package>,
    pub status: Option<String>,
    pub progress: Option<OperationProgress>,
    pub generation: u64,
    pub processes: BTreeMap<String, usize>,
}
/// The channel disconnect also shuts down when the last UI owner is released.
pub(super) struct Worker {
    pub tx: mpsc::Sender<Work>,
    pub state: Arc<Mutex<Published>>,
    #[cfg(test)]
    pub recorded: Mutex<mpsc::Receiver<Work>>,
}
impl Worker {
    /// Publish enabled registry entries before the worker compiles any component.
    fn initial_state(root: &std::path::Path) -> Published {
        let startup = Manager::read_registry(root)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(id, entry)| entry.enabled.then_some((id, entry.manifest.name)))
            .collect();
        Published {
            startup,
            ..Published::default()
        }
    }
    /// Queue one lifecycle operation and make its waiting state visible immediately.
    pub fn queue_lifecycle(&self, work: Work) -> bool {
        let Some(progress) = work.lifecycle() else {
            return false;
        };
        let mut state = self.state.lock().unwrap();
        if state.progress.is_some() {
            return false;
        }
        state.progress = Some(progress);
        state.status = None;
        if self.tx.send(work).is_err() {
            state.progress = None;
            state.status = Some("插件后台服务不可用".into());
            return false;
        }
        true
    }
    /// UI tests observe the real message seam without reading user state or launching processes.
    #[cfg(test)]
    pub fn start(root: PathBuf, _: Environment) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            state: Arc::new(Mutex::new(Self::initial_state(&root))),
            recorded: Mutex::new(rx),
        }
    }
    #[cfg(not(test))]
    pub fn start(root: PathBuf, environment: Environment) -> Self {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(Self::initial_state(&root)));
        let output = state.clone();
        std::thread::spawn(move || {
            let mut manager = match Manager::open(root, environment) {
                Ok(m) => m,
                Err(e) => {
                    let mut published = output.lock().unwrap();
                    published.startup.clear();
                    published.status = Some(format!("{e:#}"));
                    return;
                }
            };
            let mut last_save = Instant::now();
            loop {
                let work = match rx.recv_timeout(Duration::from_millis(30)) {
                    Ok(work) => Some(work),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(_) => break,
                };
                let lifecycle = work.as_ref().and_then(Work::lifecycle);
                let result = match work {
                    Some(Work::Shutdown(ack)) => {
                        drop(manager);
                        if let Some(ack) = ack {
                            let _ = ack.send(());
                        }
                        return;
                    }
                    Some(Work::Inspect(path)) => Package::read(&path)
                        .map(|package| output.lock().unwrap().pending = Some(package)),
                    Some(Work::Install(package)) => {
                        manager.install(&package, package.manifest.permissions.clone())
                    }
                    Some(Work::Enable(id)) => manager.enable(&id),
                    Some(Work::Disable(id)) => manager.disable(&id),
                    Some(Work::Uninstall(id, delete)) => manager.uninstall(&id, delete),
                    Some(Work::Event(id, event)) => manager.event(&id, event),
                    None => Ok(()),
                };
                manager.poll();
                let mut effects = vec![];
                let mut scenes = BTreeMap::new();
                let mut processes = BTreeMap::new();
                for (id, instance) in &mut manager.live {
                    for (panel, scene) in &instance.scenes {
                        scenes.insert(format!("{id}/{panel}"), scene.clone());
                    }
                    processes.insert(id.clone(), instance.process_count());
                    effects.extend(
                        instance
                            .effects()
                            .into_iter()
                            .map(|effect| (id.clone(), effect)),
                    );
                }
                if last_save.elapsed() > Duration::from_secs(3) {
                    if let Err(e) = manager.checkpoint() {
                        output.lock().unwrap().status = Some(format!("保存插件状态失败：{e:#}"));
                    }
                    last_save = Instant::now();
                }
                let mut published = output.lock().unwrap();
                // Startup loading ends only after Manager::open has restored every enabled plugin.
                published.startup.clear();
                if let Err(e) = result {
                    published.status = Some(if let Some(operation) = &lifecycle {
                        format!("{}失败：{e:#}", operation.action.label())
                    } else {
                        format!("{e:#}")
                    });
                }
                if lifecycle.is_some() {
                    published.progress = None;
                }
                published.entries = manager.installed.values().cloned().collect();
                published.scenes = scenes;
                published.processes = processes;
                published.effects.extend(effects);
                published.generation += 1;
            }
            // Manager drop atomically saves plugin snapshots and closes owned process trees.
        });
        Self { tx, state }
    }
}
impl LifecycleAction {
    /// Keep error labels aligned with the action shown by the loading button.
    fn label(self) -> &'static str {
        match self {
            Self::Install => "安装 / 更新",
            Self::Enable => "启用",
            Self::Uninstall => "卸载",
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.tx.send(Work::Shutdown(None));
    }
}
