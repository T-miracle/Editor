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

/// What one provider reported about the program it started.
///
/// This mirrors the provider's own words rather than the host's interpretation: `Running` is an
/// observation with no exit seen, and `Unknown` means the provider could not answer at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunStatus {
    Running,
    /// The program ended, with the status the provider observed when it has one.
    Ended {
        code: Option<u32>,
    },
    /// The provider terminated the program rather than the program ending on its own.
    Terminated,
    /// The provider could not report, or the session is gone without a status.
    Unknown,
}

impl RunStatus {
    /// Read the provider's answer, accepting only the states the contract defines.
    pub fn from_value(value: &serde_json::Value) -> Self {
        match value.get("state").and_then(serde_json::Value::as_str) {
            Some("running") => Self::Running,
            Some("exited") => Self::Ended {
                code: value
                    .get("code")
                    .and_then(serde_json::Value::as_u64)
                    .map(|code| code as u32),
            },
            // A program that ended without a status of its own is still an end, but the caller must
            // not read a success into it.
            Some("ended") => Self::Ended { code: None },
            _ => Self::Unknown,
        }
    }
}

pub enum Work {
    /// Provider choices are explicit host actions, never executable project configuration.
    /// Ask the runtime which providers it has for the run execution contract.
    ///
    /// A plain read: it opens no session and changes no selection, so it is safe to ask whenever a
    /// page that shows providers becomes visible.
    ListRunProviders,
    /// Call one method on the selected debug provider, on behalf of the editor.
    ///
    /// The request identity is the editor's own, so the answer is joined to what it answers; the
    /// method and arguments are the debug contract's, so nothing here decides what a call means.
    DebugCall {
        request: u64,
        method: String,
        arguments: serde_json::Value,
    },
    /// Record the execution provider a workspace's launches should use.
    ///
    /// Applying a choice never touches a session that is already running: a launch that has started
    /// keeps the provider that started it.
    SetRunProvider {
        provider: Option<String>,
    },
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
    /// The editor's run controls start a program through the public execution contract.
    ///
    /// The request already carries literal arguments and an absolute directory; the worker only asks
    /// the runtime, which selects a compatible provider by contract rather than by plugin identity.
    StartRun {
        request: plugin_runtime::RunRequest,
        config: String,
        /// Identity of the launch that requested this start, so the answer joins its own request.
        request_id: u64,
    },
    /// Stop the program one host session owns, through the provider that started it.
    ///
    /// The worker never touches a provider's private process handle; it asks the session's own
    /// provider, which is the participant that owns the program.
    StopRun {
        session: u64,
        config: String,
        /// Identity of the stop that requested this, so its answer reaches the requester that asked.
        request_id: u64,
    },
    /// Ask a session's own provider whether its program is still running.
    ///
    /// A preparation step's completion condition is an observed exit, so the host asks the provider
    /// that owns the program rather than inferring an end from elapsed time or from output.
    PollRun {
        session: u64,
        config: String,
        /// Identity of the request that is waiting on this answer.
        request_id: u64,
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
            // A status query observes a program another owner already has, so it claims no plugin.
            Self::PollRun { .. } => None,
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
            // A status query observes a program another owner already has, so it claims no plugin.
            Self::PollRun { .. } => None,
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
    /// Kept independently of live instances and manager windows for this editor process only.
    pub logs: plugin_runtime::logs::RuntimeLogs,
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
    /// One entry per execution this editor started, joined to the configuration that produced it.
    ///
    /// A snapshot is a view of the runtime's session, never a second process model: the worker
    /// republishes it and the UI joins it to the saved configuration.
    pub host_executions: Vec<HostRunSnapshot>,
    /// Result of a start request the worker could not even queue, keyed by launch identity.
    pub run_errors: Vec<(String, u64, String)>,
    /// Answers to stop requests this editor made, keyed by stop identity.
    pub stop_results: Vec<(String, u64, Result<(), String>)>,
    /// What a provider reported about one session's program, keyed by the request that asked.
    ///
    /// The answer is an observation, so a state of `Running` here means the program exists and its
    /// exit has not been seen — never that it is expected to end.
    pub run_status: Vec<(String, u64, RunStatus)>,
    /// The execution providers the runtime has, and which one a launch here would use.
    ///
    /// Descriptive only: publishing this never changes a selection, and a launch is not delayed by
    /// waiting for it. `None` means no listing has been asked for since the last lifecycle change.
    pub run_providers: Option<Vec<plugin_runtime::ProviderCandidate>>,
    /// Which debug provider a debug launch would use, or the reason there is none.
    ///
    /// Held apart from the execution listing because the two are asked different questions: this one
    /// answers "can a debug session start here at all", which is what an entry point must know before
    /// it offers debugging.
    pub debug_availability: Option<Result<String, String>>,
    /// What the debug provider that would serve a session says it can do.
    ///
    /// Availability answers whether a session can start; this answers what it can do once paused, which
    /// is what decides whether the panel may ask for a stack at all. Both come from the same selected
    /// provider, so a control and the call it stages cannot disagree about whether the call is offered.
    pub debug_abilities: Option<plugin_runtime::DebugAbilities>,
    /// Answers to debug calls this editor made, keyed by the request that asked.
    ///
    /// A failed call is reported as a failure rather than as an empty answer: a provider that could
    /// not report frames has said nothing about the target.
    pub debug_answers: Vec<(u64, DebugAnswerMessage)>,
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
/// One answer to one debug call, or the reason there is none.
#[derive(Clone, Debug)]
pub enum DebugAnswerMessage {
    Frames(Vec<plugin_runtime::DebugFrame>),
    Variables(Vec<plugin_runtime::DebugVariable>),
    /// A step's answer, which is the session's new state rather than a view of a pause.
    State(plugin_runtime::DebugSession),
    /// The positions a provider could bind, which is where a breakpoint became real or did not.
    Breakpoints(Vec<plugin_runtime::DebugBreakpoint>),
    /// The provider reported a failure, with its own account of what went wrong.
    Failed(String),
}

/// One transient operation error retains its target; manager failures cannot become another plugin's log.
#[derive(Clone, Debug)]
pub(super) struct OperationStatus {
    pub plugin: Option<String>,
    pub message: String,
}

/// A published execution session, carrying only what the run controls display or join.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostRunSnapshot {
    /// Runtime session identity.
    pub id: u64,
    /// Configuration that requested this session, from the launching editor.
    pub config: String,
    /// Launch identity, so an answer is adopted only by the request that produced it.
    pub request_id: u64,
    /// Resolved provider package identity, reported rather than used for routing.
    pub plugin: String,
    pub state: plugin_runtime::ExecutionState,
    /// Provider-reported session identity, when its answer carried one.
    pub provider_session: Option<String>,
    /// Provider-reported failure, retained as the visible result of the launch.
    pub failure: Option<String>,
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
