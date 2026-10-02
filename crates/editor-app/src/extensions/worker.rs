//! A single worker owns plugin stores; the UI thread never compiles or executes WASM.
use plugin_runtime::{Installed, Manager, Package, plugin_protocol::*};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

pub(super) enum Work {
    /// Only a confirmed host form can modify user/project configuration.
    SetSetting {
        request: u64,
        plugin: String,
        scope: settings::Scope,
        key: String,
        value: Option<serde_json::Value>,
    },
    Inspect(PathBuf),
    Install(Package),
    Enable(String),
    Disable(String),
    /// Persist a per-workspace override without changing the global default.
    SetProjectEnabled(String, bool),
    /// Only an explicit host-local choice can lift workspace restrictions.
    SetTrust(bool),
    Uninstall(String, bool),
    Event(String, Event),
    /// Host-originated commands target a plugin directly, even while its panel is hidden.
    Invoke {
        plugin: String,
        command: String,
        arguments: serde_json::Value,
    },
    Shutdown(Option<futures::channel::oneshot::Sender<()>>),
}
impl Work {
    /// Identify the operation whose button should show loading while queued or running.
    fn lifecycle(&self) -> Option<OperationProgress> {
        match self {
            Self::Inspect(path) => Some(OperationProgress {
                id: path.display().to_string(),
                action: LifecycleAction::Inspect,
                delete_data: None,
            }),
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
            Self::Disable(id) => Some(OperationProgress {
                id: id.clone(),
                action: LifecycleAction::Disable,
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
    Inspect,
    Install,
    Enable,
    Disable,
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
    pub language_services: BTreeMap<String, Result<Arc<plugin_runtime::LanguageService>, String>>,
    pub configurations: BTreeMap<String, Result<settings::Effective, String>>,
    pub configuration_result: Option<(u64, Result<(), String>)>,
    pub configuration_revision: u64,
    /// UI document ingress is bounded independently of the worker's command channel.
    pub document_events: plugin_runtime::DocumentEvents,
    /// Bounded typed work has a completion gate that survives queue transfer and rejects stale callbacks.
    pub editor_requests: Vec<(String, plugin_runtime::EditorRequest)>,
    pub entries: Vec<Installed>,
    pub startup: BTreeMap<String, String>,
    pub scenes: BTreeMap<String, Arc<Scene>>,
    /// Each scene's full-color image operations are ready before the UI observes that scene.
    pub images: super::images::SceneImages,
    pub effects: Vec<(String, Request)>,
    pub pending: Option<Package>,
    pub status: Option<String>,
    pub progress: Option<OperationProgress>,
    pub generation: u64,
    /// Changes when a plugin instance is replaced, even by the same package digest.
    pub instance_epochs: BTreeMap<String, u64>,
    pub processes: BTreeMap<String, usize>,
}
/// The channel disconnect also shuts down when the last UI owner is released.
pub(super) struct Worker {
    pub tx: mpsc::Sender<Work>,
    pub state: Arc<Mutex<Published>>,
    /// UI publication is masked immediately, including results queued before revocation.
    pub trusted: std::sync::atomic::AtomicBool,
    #[cfg(test)]
    pub recorded: Mutex<mpsc::Receiver<Work>>,
}
impl Worker {
    /// Publish enabled registry entries before the worker compiles any component.
    fn initial_state(
        root: &std::path::Path,
        environment: &Environment,
        trusted: bool,
    ) -> Published {
        let startup = Manager::read_registry(root)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(id, entry)| {
                (trusted && (entry.enabled || entry.project_enabled_in(&environment.workspace)))
                    .then_some((id, entry.manifest.name))
            })
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
    pub fn start(root: PathBuf, environment: Environment, trusted: bool) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            state: Arc::new(Mutex::new(Self::initial_state(
                &root,
                &environment,
                trusted,
            ))),
            trusted: std::sync::atomic::AtomicBool::new(trusted),
            recorded: Mutex::new(rx),
        }
    }
    #[cfg(not(test))]
    pub fn start(root: PathBuf, environment: Environment, trusted: bool) -> Self {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(Self::initial_state(
            &root,
            &environment,
            trusted,
        )));
        let output = state.clone();
        std::thread::spawn(move || {
            let mut manager = match Manager::open_with_trust(root, environment, trusted) {
                Ok(m) => m,
                Err(e) => {
                    let mut published = output.lock().unwrap();
                    published.startup.clear();
                    published.status = Some(format!("{e:#}"));
                    return;
                }
            };
            let mut last_save = Instant::now();
            let mut vectors = super::images::VectorRenderer::default();
            loop {
                let work = match rx.recv_timeout(Duration::from_millis(30)) {
                    Ok(work) => Some(work),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(_) => break,
                };
                let lifecycle = work.as_ref().and_then(Work::lifecycle);
                // A successful replacement needs a fresh surface Resize event.
                let restarted_plugin = match work.as_ref() {
                    Some(Work::SetSetting { plugin, .. }) => Some(plugin.clone()),
                    Some(Work::Install(package)) => Some(package.manifest.id.clone()),
                    Some(Work::Enable(id)) => Some(id.clone()),
                    _ => None,
                };
                let result = match work {
                    Some(Work::SetSetting {
                        request,
                        plugin,
                        scope,
                        key,
                        value,
                    }) => {
                        let result = manager.update_setting(&plugin, scope, &key, value);
                        let mut published = output.lock().unwrap();
                        published.configuration_result = Some((
                            request,
                            result
                                .as_ref()
                                .map(|_| ())
                                .map_err(|error| format!("{error:#}")),
                        ));
                        published.configuration_revision += 1;
                        result
                    }
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
                    Some(Work::SetTrust(trusted)) => manager.set_workspace_trust(trusted),
                    Some(Work::Disable(id)) => manager.disable(&id),
                    Some(Work::SetProjectEnabled(id, enabled)) => {
                        manager.set_project_enabled(&id, enabled)
                    }
                    Some(Work::Uninstall(id, delete)) => manager.uninstall(&id, delete),
                    Some(Work::Event(id, event)) => manager.event(&id, event),
                    Some(Work::Invoke {
                        plugin,
                        command,
                        arguments,
                    }) => manager.invoke_command(&plugin, &command, arguments),
                    None => Ok(()),
                };
                match output.lock().unwrap().document_events.take_batch(64) {
                    Ok(changes) => {
                        for change in changes {
                            manager.document_changed(change);
                        }
                    }
                    Err(error) => manager.document_events_failed(error),
                }
                manager.poll();
                let mut effects = vec![];
                let mut scenes = BTreeMap::new();
                let mut processes = BTreeMap::new();
                let mut editor_requests = Vec::new();
                for (id, instance) in &mut manager.live {
                    editor_requests.extend(
                        instance
                            .take_editor_requests()
                            .into_iter()
                            .map(|request| (id.clone(), request)),
                    );
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
                // Vector parsing and rendering stay on this worker, outside the shared-state lock.
                let images = vectors.prepare(&scenes);
                let configurations = manager
                    .installed
                    .keys()
                    .map(|id| {
                        (
                            id.clone(),
                            manager
                                .effective_settings(id)
                                .map_err(|error| format!("{error:#}")),
                        )
                    })
                    .collect::<BTreeMap<_, _>>();
                let language_services = manager.language_services();
                for service in language_services
                    .values()
                    .filter_map(|service| service.as_ref().ok())
                {
                    *processes.entry(service.owner.clone()).or_default() += service.process_count();
                }
                let mut published = output.lock().unwrap();
                let services_changed = language_services.len() != published.language_services.len()
                    || language_services.iter().any(|(key, value)| {
                        match (value, published.language_services.get(key)) {
                            (Ok(current), Some(Ok(old))) => !Arc::ptr_eq(current, old),
                            (Err(current), Some(Err(old))) => current != old,
                            _ => true,
                        }
                    });
                if services_changed {
                    published.language_services = language_services;
                    published.configuration_revision += 1;
                }
                if published.configurations != configurations {
                    published.configurations = configurations;
                    published.configuration_revision += 1;
                }
                published
                    .editor_requests
                    .retain(|(_, request)| !request.status().is_terminal());
                for request in editor_requests {
                    if published.editor_requests.len() < 256 {
                        published.editor_requests.push(request);
                    } else {
                        request.1.finish(Err(api::Failure::new(
                            api::ErrorCode::LimitExceeded,
                            "Editor publication queue is full",
                        )));
                    }
                }
                let replacement_succeeded = result.is_ok();
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
                if replacement_succeeded {
                    if let Some(id) = restarted_plugin {
                        *published.instance_epochs.entry(id).or_default() += 1;
                    }
                }
                published.entries = manager.published_entries();
                published.scenes = scenes;
                published.images = images;
                published.processes = processes;
                published.effects.extend(effects);
                published.generation += 1;
            }
            // Manager drop atomically saves plugin snapshots and closes owned process trees.
        });
        Self {
            tx,
            state,
            trusted: std::sync::atomic::AtomicBool::new(trusted),
        }
    }
}
impl LifecycleAction {
    /// Keep error labels aligned with the action shown by the loading button.
    fn label(self) -> &'static str {
        match self {
            Self::Inspect => "检查插件包",
            Self::Install => "安装 / 更新",
            Self::Enable => "启用",
            Self::Disable => "禁用",
            Self::Uninstall => "卸载",
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.tx.send(Work::Shutdown(None));
    }
}
