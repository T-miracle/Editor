//! A single worker owns plugin stores; the UI thread never compiles or executes WASM.
use plugin_runtime::{Installed, Manager, Package, plugin_protocol::*};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

pub(super) enum Work {
    /// Provider choices are explicit host actions, never executable project configuration.
    SetServiceProvider {
        request: u64,
        owner: api::InstanceScope,
        scope: settings::Scope,
        contract: String,
        provider: Option<String>,
    },
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
    Restart(String),
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
            Self::Restart(id) => Some(OperationProgress {
                id: id.clone(),
                action: LifecycleAction::Restart,
                delete_data: None,
            }),
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
    Restart,
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
/// Installation success and language-service readiness are separate user-visible states.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct InstallationProgress {
    pub id: String,
    pub message: String,
    pub cancellable: bool,
    /// Only a successful install may be followed by readiness; old-version callbacks cannot mask failures.
    pub installed: bool,
}
#[derive(Default)]
pub(super) struct Published {
    pub diagnostics: BTreeMap<String, Vec<plugin_runtime::faults::Diagnostic>>,
    pub plugin_service_choices: Vec<service::Choice>,
    pub installation: Option<InstallationProgress>,
    pub install_control: Option<plugin_runtime::InstallControl>,
    pub service_states: BTreeMap<String, String>,
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
    /// This bypasses the serialized command queue so a long download cannot delay shutdown or trust revocation.
    pub fn cancel_installation(&self) {
        if let Some(control) = &self.state.lock().unwrap().install_control {
            control.cancel();
        }
    }
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
        if let Work::Install(package) = &work {
            state.installation = Some(InstallationProgress {
                id: package.manifest.id.clone(),
                message: "准备安装…".into(),
                cancellable: true,
                installed: false,
            });
            let output = Arc::downgrade(&self.state);
            state.install_control = Some(
                plugin_runtime::InstallControl::new(move |stage| {
                    if let Some(output) = output.upgrade() {
                        let mut state = output.lock().unwrap();
                        if let Some(report) = &mut state.installation {
                            use plugin_runtime::InstallStage::*;
                            report.cancellable = stage != Prepared;
                            report.message = match stage {
                                Preparing => "正在准备依赖…".into(),
                                Downloading(id) => format!("正在下载 / 读取依赖：{id}"),
                                Verifying(id) => format!("正在验证 SHA-256：{id}"),
                                Extracting(id) => format!("正在解包依赖：{id}"),
                                AwaitingAuthorization(id) => {
                                    format!("原生安装步骤 {id} 正在等待额外授权。")
                                }
                                Installing(purpose) => format!("正在执行安装步骤：{purpose}"),
                                Prepared => "依赖已准备，正在安装插件…".into(),
                            };
                        }
                    }
                })
                .with_installer_prompts(),
            );
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
                    Some(Work::Restart(id)) => Some(id.clone()),
                    _ => None,
                };
                let result = match work {
                    Some(Work::SetServiceProvider {
                        request,
                        owner,
                        scope,
                        contract,
                        provider,
                    }) => {
                        let result = manager.set_service_provider(
                            owner,
                            scope,
                            &contract,
                            provider.as_deref(),
                        );
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
                        let control = output
                            .lock()
                            .unwrap()
                            .install_control
                            .clone()
                            .unwrap_or_default();
                        manager.install_with_control(
                            &package,
                            package.manifest.permissions.clone(),
                            &control,
                        )
                    }
                    Some(Work::Enable(id)) => manager.enable(&id),
                    Some(Work::Restart(id)) => manager.restart_plugin(&id),
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
                let plugin_service_choices = manager.service_choices();
                let mut published = output.lock().unwrap();
                if published.plugin_service_choices != plugin_service_choices {
                    published.plugin_service_choices = plugin_service_choices;
                    published.configuration_revision += 1;
                }
                let services_changed = language_services.len() != published.language_services.len()
                    || language_services.iter().any(|(key, value)| {
                        match (value, published.language_services.get(key)) {
                            (Ok(current), Some(Ok(old))) => !Arc::ptr_eq(current, old),
                            (Err(current), Some(Err(old))) => current != old,
                            _ => true,
                        }
                    });
                if services_changed {
                    // Retired providers must not leave a previous version's ready badge behind.
                    let unchanged = published.language_services.iter().filter_map(|(key, old)| {
                        matches!((old, language_services.get(key)), (Ok(old), Some(Ok(new))) if Arc::ptr_eq(old,new)).then_some(key.clone())
                    }).collect::<std::collections::BTreeSet<_>>();
                    published
                        .service_states
                        .retain(|key, _| unchanged.contains(key));
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
                if lifecycle
                    .as_ref()
                    .is_some_and(|operation| operation.action == LifecycleAction::Install)
                {
                    if let Some(report) = &mut published.installation {
                        report.cancellable = false;
                        report.installed = result.is_ok();
                        report.message = match &result {
                            Err(error) => {
                                format!("安装失败：{error:#}\n可关闭此窗口后重新点击安装重试。")
                            }
                            Ok(())
                                if manager.installed.get(&report.id).is_some_and(|entry| {
                                    !entry.manifest.language_servers.is_empty()
                                }) =>
                            {
                                "插件已安装；等待语言服务选择与启动…".into()
                            }
                            Ok(()) => "插件安装完成。".into(),
                        };
                    }
                    published.install_control = None;
                    let failures = published
                        .language_services
                        .iter()
                        .filter_map(|(key, service)| {
                            service
                                .as_ref()
                                .err()
                                .map(|error| format!("{key}：准备失败：{error}"))
                        })
                        .collect::<Vec<_>>();
                    if let Some(report) = &mut published.installation {
                        for failure in failures
                            .iter()
                            .filter(|line| line.starts_with(&format!("{}/", report.id)))
                        {
                            report.message.push_str(&format!("\n{failure}"));
                        }
                    }
                }
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
                published.diagnostics = manager
                    .installed
                    .keys()
                    .map(|id| (id.clone(), manager.diagnostics(id)))
                    .collect();
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
            Self::Restart => "重启插件",
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
        self.cancel_installation();
        let _ = self.tx.send(Work::Shutdown(None));
    }
}
