//! A single worker owns plugin stores; the UI thread never compiles or executes WASM.
use plugin_runtime::{Installed, Manager, Package, plugin_protocol::*};
#[cfg(test)]
mod bundled_tests;
mod preparation;
mod runner;
#[cfg(test)]
mod worker_tests;
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
    /// Shipped discovery never starts guests or forces the manager window open.
    InspectBundle(super::bundled::Request),
    /// Only the matching native confirmation can submit the retained immutable package.
    InstallBundle(super::bundled::Candidate),
    /// Refusal is a host-owned identity choice, independent of version digest or removable plugin data.
    DeclineBundle(super::bundled::Candidate),
    Enable(String),
    Restart(String),
    Disable(String),
    /// Persist a per-workspace override without changing the global default.
    SetProjectEnabled(String, bool),
    /// Only an explicit host-local choice can lift workspace restrictions.
    SetTrust(bool),
    Uninstall(String, bool),
    /// Native callbacks retain the incarnation that created them, even if a replacement reuses node IDs.
    Event(String, u64, Option<String>, api::Notification),
    /// Bytes offered by a native user gesture remain host-owned; only opaque metadata crosses WASM.
    ImageInput {
        plugin: String,
        panel: String,
        epoch: u64,
        document: api::DocumentVersion,
        selection: api::TextRange,
        origin: plugin_runtime::HostImageOrigin,
        images: Vec<plugin_runtime::HostImageInput>,
        /// Conservatively reserves one 32 MiB batch until the manager adopts or rejects its bytes.
        reservation: ImageOfferReservation,
    },
    /// Host-originated commands target a plugin directly, even while its panel is hidden.
    Invoke {
        plugin: String,
        command: String,
        arguments: serde_json::Value,
    },
    Shutdown(Option<futures::channel::oneshot::Sender<()>>),
}
impl Work {
    /// Preserve plugin ownership before dispatch consumes the work; manager-wide actions have no owner.
    fn plugin_id(&self) -> Option<&str> {
        match self {
            Self::SetSetting { plugin, .. }
            | Self::Invoke { plugin, .. }
            | Self::ImageInput { plugin, .. } => Some(plugin),
            Self::Install(package) => Some(&package.manifest.id),
            Self::InstallBundle(candidate) | Self::DeclineBundle(candidate) => {
                Some(&candidate.package.manifest.id)
            }
            Self::Enable(id)
            | Self::Restart(id)
            | Self::Disable(id)
            | Self::SetProjectEnabled(id, _)
            | Self::Uninstall(id, _)
            | Self::Event(id, ..) => Some(id),
            _ => None,
        }
    }
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
            Self::InstallBundle(candidate) => Some(OperationProgress {
                id: candidate.package.manifest.id.clone(),
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
    /// True only after successful private-store recovery and the actor's first complete entry publication.
    pub ready: bool,
    /// A single first-use reply is consumed only by the main editor owner; it grants no installation rights.
    pub bundle_reply: Option<super::bundled::Reply>,
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
    pub views: BTreeMap<String, Arc<ui::Document>>,
    /// Each scene's full-color image operations are ready before the UI observes that scene.
    pub images: super::images::SceneImages,
    pub pending: Option<Package>,
    pub status: Option<OperationStatus>,
    pub progress: Option<OperationProgress>,
    pub generation: u64,
    /// Changes when a plugin instance is replaced, even by the same package digest.
    pub instance_epochs: BTreeMap<String, u64>,
    pub processes: BTreeMap<String, usize>,
}
/// One transient operation error retains its target; manager failures cannot become another plugin's log.
#[derive(Clone, Debug)]
pub(super) struct OperationStatus {
    pub plugin: Option<String>,
    pub message: String,
}
/// The channel disconnect also shuts down when the last UI owner is released.
pub(super) struct Worker {
    pub tx: mpsc::Sender<Work>,
    pub state: Arc<Mutex<Published>>,
    /// UI publication is masked immediately, including results queued before revocation.
    pub trusted: Arc<std::sync::atomic::AtomicBool>,
    /// Two native batches bound preparation and the otherwise unbounded command channel to 64 MiB.
    image_offers: Arc<std::sync::atomic::AtomicUsize>,
    #[cfg(test)]
    pub recorded: Mutex<mpsc::Receiver<Work>>,
}

/// Queue ownership is released on rejection, worker shutdown or completion of native-to-manager transfer.
pub(super) struct ImageOfferReservation(Arc<std::sync::atomic::AtomicUsize>);

impl Drop for ImageOfferReservation {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}

impl Worker {
    /// Reserve before copying clipboard pixels or reading external files, never after enqueueing them.
    pub(super) fn reserve_image_offer(&self) -> Option<ImageOfferReservation> {
        use std::sync::atomic::Ordering;
        self.image_offers
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                (used < 2).then_some(used + 1)
            })
            .ok()?;
        Some(ImageOfferReservation(self.image_offers.clone()))
    }
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
                (trusted
                    && entry.compatibility_error().is_none()
                    && (entry.enabled || entry.project_enabled_in(&environment.workspace)))
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
        let installation = match &work {
            Work::Install(package) => Some(package),
            Work::InstallBundle(candidate) => Some(candidate.package.as_ref()),
            _ => None,
        };
        if let Some(package) = installation {
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
                            report.cancellable =
                                !matches!(stage, Prepared | Migrating | Committing | Committed);
                            report.message = match stage {
                                Migrating => "正在隔离副本中迁移插件数据…".into(),
                                Committing => "正在提交插件版本和数据…".into(),
                                Committed => "插件版本和数据已提交。".into(),
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
            state.status = Some(OperationStatus {
                plugin: None,
                message: "插件后台服务不可用".into(),
            });
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
            trusted: Arc::new(std::sync::atomic::AtomicBool::new(trusted)),
            image_offers: Default::default(),
            recorded: Mutex::new(rx),
        }
    }
    #[cfg(not(test))]
    pub fn start(root: PathBuf, environment: Environment, trusted: bool) -> Self {
        Self::start_background(root, environment, trusted)
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
