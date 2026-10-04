//! Run controls: saved configurations, their launches, and the sessions they produced.
//!
//! The editor owns the configuration and the visible session list; the execution itself belongs to
//! whichever compatible provider the runtime selects. Nothing here inspects a program name to decide
//! behavior: a configuration is either valid, or it is not.
use crate::extensions::HostRunSnapshot;
use editor_core::{RunConfig, RunConfigSet, RunTarget};
use std::collections::BTreeMap;

mod ui;
pub use ui::RunConfigForm;
pub(crate) use ui::RunMenu;
mod sequence;
pub use sequence::{RunSequence, SequenceAction, SequenceStep, StepOutcome, StepState};
#[cfg(test)]
mod run_ui_tests;
#[cfg(test)]
mod tests;

/// Which part of a launch a prepared step belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepKind {
    /// A build action: it produces something the program or a later step consumes.
    Build,
    /// A step that must succeed before the program itself starts.
    Prelaunch,
    /// The program the user asked to run.
    Program,
}

impl StepKind {
    /// The word shown for this phase in status and failure text.
    pub fn label(self) -> &'static str {
        match self {
            Self::Build => "构建",
            Self::Prelaunch => "启动前",
            Self::Program => "程序",
        }
    }
}

/// One action a launch performs, in the order it must happen.
///
/// A step carries the request the provider will receive, so the sequence neither re-composes a
/// command between steps nor depends on the configuration still being stored when it runs.
#[derive(Clone, Debug)]
pub struct PreparedStep {
    pub kind: StepKind,
    /// Name shown while the step runs, and in the failure that stops the sequence.
    pub name: String,
    /// The configuration whose stored definition produced this step, for de-duplication.
    pub config: String,
    pub request: plugin_runtime::RunRequest,
}

/// Every action one launch performs, in order, ending with the program itself.
///
/// The plan is computed once, before anything starts, so a failure while preparing a later step can
/// never happen halfway through a sequence the user already saw begin.
#[derive(Clone, Debug)]
pub struct RunPlan {
    pub steps: Vec<PreparedStep>,
}

impl RunPlan {
    /// The step at this position, if the sequence still has one.
    pub fn step(&self, index: usize) -> Option<&PreparedStep> {
        self.steps.get(index)
    }

    /// Whether this plan starts a program at the end of its sequence.
    pub fn launches_program(&self) -> bool {
        self.steps
            .last()
            .is_some_and(|step| step.kind == StepKind::Program)
    }
}

/// What a launch request should do, decided before any work is requested.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchPlan {
    /// A session for this literal command is already retained; reveal it instead of starting again.
    Existing { session: u64 },
    /// Ask the runtime to start this configuration.
    Start {
        config: RunConfig,
        /// Absolute working directory, or the workspace root when the configuration omits one.
        directory: Option<String>,
        /// Request label shown by the provider for this session.
        name: String,
    },
    /// The configuration cannot be launched as stored; the reason is shown instead of guessing.
    Invalid { message: String },
}

/// A launch that has been requested from the runtime but has no published session identity yet.
#[derive(Clone, Debug)]
pub struct PendingRun {
    pub config: String,
    pub request_id: u64,
}

/// A stop this editor requested, identified so only its own answer is reported.
#[derive(Clone, Debug)]
pub struct PendingStop {
    pub config: String,
    pub session: u64,
    pub request_id: u64,
}

/// One execution this editor requested, joined to the configuration that produced it.
#[derive(Clone, Debug)]
pub struct RunSession {
    /// Runtime session identity.
    pub id: u64,
    /// Configuration identity at start time; later edits do not retarget a running session.
    pub config: String,
    /// Provider package identity, reported for transparency.
    pub plugin: String,
    pub state: plugin_runtime::ExecutionState,
    pub provider_session: Option<String>,
    /// Provider-reported failure, when the launch did not reach a running program.
    pub failure: Option<String>,
}

impl RunSession {
    /// Whether the session is still a launch or a running program rather than a finished result.
    pub fn is_active(&self) -> bool {
        match self.state {
            plugin_runtime::ExecutionState::Starting | plugin_runtime::ExecutionState::Running => {
                true
            }
            plugin_runtime::ExecutionState::Failed => false,
        }
    }
}

/// Saved configurations plus everything this editor has launched from them.
#[derive(Debug)]
pub struct RunControls {
    configs: RunConfigSet,
    /// Identity assigned to the next local session, used only before the runtime answers.
    next_request: u64,
    pending: Vec<PendingRun>,
    /// Stop requests awaiting their provider's answer, keyed by the configuration they stop.
    stops: Vec<PendingStop>,
    sessions: BTreeMap<u64, RunSession>,
    /// Storage directory used for host-local configuration files.
    root: Option<std::path::PathBuf>,
    /// Set when the stored file could not be read or written; shown instead of silently defaulting.
    pub error: Option<String>,
}

/// Controls without a storage directory, used while a workspace has no host-local state yet.
impl Default for RunControls {
    fn default() -> Self {
        Self {
            configs: RunConfigSet::default(),
            next_request: 0,
            pending: Vec::new(),
            stops: Vec::new(),
            sessions: BTreeMap::new(),
            root: None,
            error: None,
        }
    }
}

impl RunControls {
    /// Load the configurations stored for one workspace, reporting an unreadable file.
    pub fn load(workspace: &str, root: Option<std::path::PathBuf>) -> Self {
        let mut controls = Self {
            root: root.clone(),
            ..Self::default()
        };
        let Some(root) = root else {
            return controls;
        };
        match editor_core::load(&root, workspace) {
            Ok(configs) => controls.configs = configs,
            // An unreadable file is never replaced by defaults: the user's previous configurations
            // must survive until they decide what to do with the file.
            Err(error) => controls.error = Some(error.to_string()),
        }
        controls
    }

    /// A stored configuration by identity, used when the form reopens an existing entry.
    pub fn configuration(&self, id: &str) -> Option<&RunConfig> {
        self.configs.find(id)
    }

    pub fn configurations(&self) -> &[RunConfig] {
        &self.configs.configurations
    }

    pub fn selected(&self) -> Option<&RunConfig> {
        self.configs.selected()
    }

    /// Select a stored configuration and remember the choice for the next start.
    pub fn select(&mut self, id: &str, workspace: &str) -> bool {
        let selected = self.configs.select(id);
        if selected {
            // Selection is persisted, so reopening the editor keeps the same visible target.
            let _ = self.persist(workspace);
        }
        selected
    }

    /// Save or replace one configuration and persist the result.
    pub fn upsert(&mut self, configuration: RunConfig, workspace: &str) -> Result<(), String> {
        // A rejected configuration is reported here as well as returned, so the visible error and the
        // refused edit cannot disagree.
        if let Err(error) = self.configs.upsert(configuration) {
            let message = error.to_string();
            self.error = Some(message.clone());
            return Err(message);
        }
        self.persist(workspace)
    }

    pub fn remove(&mut self, id: &str, workspace: &str) -> Result<(), String> {
        self.configs.remove(id);
        // A removed configuration's finished sessions stay visible; running ones are not hidden.
        self.persist(workspace)
    }

    /// Identity for a new configuration in this workspace.
    pub fn generate_id(&self, workspace: &str) -> String {
        self.configs.generate_id(workspace)
    }

    fn persist(&mut self, workspace: &str) -> Result<(), String> {
        let Some(root) = self.root.clone() else {
            return Ok(());
        };
        match editor_core::save(&root, workspace, &self.configs) {
            Ok(()) => {
                self.error = None;
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                self.error = Some(message.clone());
                Err(message)
            }
        }
    }

    /// All sessions this editor knows about, newest request last.
    pub fn sessions(&self) -> Vec<RunSession> {
        self.sessions.values().cloned().collect()
    }

    /// Active sessions only, which is what the top bar offers to stop or reveal.
    pub fn active_sessions(&self) -> Vec<RunSession> {
        self.sessions
            .values()
            .filter(|session| session.is_active())
            .cloned()
            .collect()
    }

    /// Sessions belonging to one configuration, so the menu can group running work by target.
    pub fn sessions_for(&self, config: &str) -> Vec<RunSession> {
        self.sessions
            .values()
            .filter(|session| session.config == config)
            .cloned()
            .collect()
    }

    /// A running program for this configuration, if one is known.
    ///
    /// Starting a second program for the same configuration is never implicit: a duplicate click
    /// resolves to the session that already exists.
    pub fn running_for(&self, config: &str) -> Option<RunSession> {
        self.sessions
            .values()
            .find(|session| session.config == config && session.is_active())
            .cloned()
    }

    /// Decide what a launch means before any work is requested from the runtime.
    ///
    /// A configuration that is already starting or running resolves to its session; an invalid one
    /// reports why; otherwise the runtime is asked to start it.
    pub fn plan_launch(&self, id: &str, workspace_root: &str) -> LaunchPlan {
        let Some(config) = self.configs.find(id) else {
            return LaunchPlan::Invalid {
                message: "运行配置不存在，请重新选择".into(),
            };
        };
        if let Err(error) = config.validate() {
            return LaunchPlan::Invalid {
                message: error.to_string(),
            };
        }
        if let Some(session) = self.running_for(id) {
            return LaunchPlan::Existing {
                session: session.id,
            };
        }
        // A configuration without a directory launches from the workspace root; a stored directory is
        // absolute, so it never depends on where the editor was started.
        let directory = config
            .directory
            .clone()
            .or_else(|| Some(workspace_root.to_owned()));
        LaunchPlan::Start {
            config: config.clone(),
            directory,
            name: config.name.clone(),
        }
    }

    /// Literal program arguments for the planned request, including interpreter script mode.
    pub fn request_for(plan: &LaunchPlan) -> Option<plugin_runtime::RunRequest> {
        let LaunchPlan::Start {
            config,
            directory,
            name,
        } = plan
        else {
            return None;
        };
        Some(plugin_runtime::RunRequest {
            program: config.target.executable().to_owned(),
            args: config.literal_arguments(),
            cwd: directory.clone(),
            name: Some(name.clone()),
            // Environment entries belong to this launch; the host passes them through untouched.
            // The configuration's tool directories become a leading PATH, so its own tools are found
            // first without changing the search order of anything else on this machine.
            env: editor_core::launch_environment(&config.env, &config.tool_paths)
                .into_iter()
                .map(|(name, value)| plugin_runtime::RunEnvEntry { name, value })
                .collect(),
        })
    }

    /// Every action a launch performs for one configuration, in order: build, then each pre-launch
    /// step, then the program.
    ///
    /// A step that names another configuration expands that configuration's build actions here, once,
    /// so the sequence cannot change halfway through a launch. `limit` bounds a plan without a
    /// program, which is what a build-only request produces.
    pub fn prepare(&self, id: &str, workspace_root: &str, limit: usize) -> Result<RunPlan, String> {
        let config = self
            .configs
            .configurations
            .iter()
            .find(|config| config.id == id)
            .ok_or_else(|| "运行配置不存在，请重新选择".to_owned())?;
        let mut steps = Vec::new();
        // A configuration's own build actions come first: a pre-launch step may then rely on them.
        for step in &config.build {
            steps.push(self.action_step(
                config,
                StepKind::Build,
                &step.name,
                &step.target,
                workspace_root,
            )?);
        }
        for step in &config.prelaunch {
            match &step.target {
                editor_core::StepTarget::Action { target } => {
                    steps.push(self.action_step(
                        config,
                        StepKind::Prelaunch,
                        &step.name,
                        &editor_core::StepTarget::Action {
                            target: target.clone(),
                        },
                        workspace_root,
                    )?);
                }
                editor_core::StepTarget::Build { config: name } => {
                    // A reference is resolved to the referenced configuration's own actions, so the
                    // command exists once and an edit to it takes effect on the next launch.
                    let referenced = self
                        .configs
                        .configurations
                        .iter()
                        .find(|candidate| candidate.name == *name)
                        .ok_or_else(|| {
                            format!("启动前步骤 {} 引用的构建配置不存在：{name}", step.name)
                        })?;
                    if referenced.id == config.id {
                        return Err(format!(
                            "启动前步骤 {} 不能引用当前配置自身的构建",
                            step.name
                        ));
                    }
                    for action in &referenced.build {
                        steps.push(self.action_step(
                            referenced,
                            StepKind::Prelaunch,
                            &format!("{} · {}", step.name, action.name),
                            &action.target,
                            workspace_root,
                        )?);
                    }
                }
            }
        }
        if limit == 0 || steps.len() > limit {
            return Err(format!("一次启动的准备步骤不能超过 {limit} 个"));
        }
        Ok(RunPlan { steps })
    }

    /// Every action a launch performs, ending with the program itself.
    ///
    /// The program is appended here rather than in [`Self::prepare`], so a build-only request reuses
    /// the same preparation rules without ever reaching a program step.
    pub fn prepare_launch(
        &self,
        id: &str,
        workspace_root: &str,
        limit: usize,
    ) -> Result<RunPlan, String> {
        let mut plan = self.prepare(id, workspace_root, limit - 1)?;
        let config = self
            .configs
            .configurations
            .iter()
            .find(|config| config.id == id)
            .ok_or_else(|| "运行配置不存在，请重新选择".to_owned())?;
        let directory = config
            .directory
            .clone()
            .or_else(|| Some(workspace_root.to_owned()));
        plan.steps.push(PreparedStep {
            kind: StepKind::Program,
            name: config.name.clone(),
            config: config.id.clone(),
            request: plugin_runtime::RunRequest {
                program: config.target.executable().to_owned(),
                args: config.literal_arguments(),
                cwd: directory,
                name: Some(config.name.clone()),
                env: editor_core::launch_environment(&config.env, &config.tool_paths)
                    .into_iter()
                    .map(|(name, value)| plugin_runtime::RunEnvEntry { name, value })
                    .collect(),
            },
        });
        Ok(plan)
    }

    /// Build one prepared step for an action, applying the configuration's own launch context.
    fn action_step(
        &self,
        config: &RunConfig,
        kind: StepKind,
        name: &str,
        target: &editor_core::StepTarget,
        workspace_root: &str,
    ) -> Result<PreparedStep, String> {
        let editor_core::StepTarget::Action { target } = target else {
            return Err(format!("步骤 {name} 不是可直接执行的动作"));
        };
        let directory = config
            .directory
            .clone()
            .or_else(|| Some(workspace_root.to_owned()));
        Ok(PreparedStep {
            kind,
            name: name.to_owned(),
            config: config.id.clone(),
            request: plugin_runtime::RunRequest {
                program: target.executable().to_owned(),
                args: target.arguments().into_iter().map(str::to_owned).collect(),
                cwd: directory,
                name: Some(name.to_owned()),
                // A step runs with the same environment the program will have, so a build and the
                // program it prepares cannot disagree about which tools they are using.
                env: editor_core::launch_environment(&config.env, &config.tool_paths)
                    .into_iter()
                    .map(|(name, value)| plugin_runtime::RunEnvEntry { name, value })
                    .collect(),
            },
        })
    }

    /// Record that a start was requested; the runtime answers with the session identity later.
    pub fn begin(&mut self, config: &str) -> u64 {
        self.next_request += 1;
        self.pending.push(PendingRun {
            config: config.to_owned(),
            request_id: self.next_request,
        });
        self.next_request
    }

    /// Record that a stop was requested for one running session.
    pub fn begin_stop(&mut self, config: &str, session: u64) -> u64 {
        self.next_request += 1;
        self.stops.push(PendingStop {
            config: config.to_owned(),
            session,
            request_id: self.next_request,
        });
        self.next_request
    }

    /// Whether a stop for this configuration is still awaiting its provider's answer.
    pub fn is_stopping(&self, config: &str) -> bool {
        self.stops.iter().any(|stop| stop.config == config)
    }

    /// Whether leaving now would abandon managed work: a running session or an unanswered start.
    ///
    /// This is what the window asks before closing, so leaving with active programs is a decision the
    /// user makes rather than something that happens while they are editing.
    pub fn has_work_in_flight(&self) -> bool {
        !self.pending.is_empty()
            || !self.stops.is_empty()
            || self.sessions.values().any(|session| session.is_active())
    }

    /// Every session that would be left behind, for the confirmation text.
    pub fn active_session_ids(&self) -> Vec<u64> {
        self.sessions
            .values()
            .filter(|session| session.is_active())
            .map(|session| session.id)
            .collect()
    }

    /// Accept the answers to stop requests this editor made; each answer is reported once.
    ///
    /// An answer belonging to a stop this editor did not request is ignored, so one window never
    /// reports another window's result.
    pub fn reconcile_stops(
        &mut self,
        published: &[(String, u64, Result<(), String>)],
    ) -> Vec<(u64, Result<(), String>)> {
        let mut reported = Vec::new();
        for (config, request_id, result) in published {
            let Some(index) = self
                .stops
                .iter()
                .position(|stop| stop.config == *config && stop.request_id == *request_id)
            else {
                continue;
            };
            let stop = self.stops.remove(index);
            reported.push((stop.session, result.clone()));
        }
        reported
    }

    /// Accept the session identities the runtime published for the requests this editor made.
    ///
    /// A launch with no matching pending request is not adopted: another window's session must not
    /// appear as this one's result. Published state is authoritative for sessions already known here.
    pub fn reconcile(&mut self, published: &[HostRunSnapshot]) {
        let by_id = published
            .iter()
            .map(|snapshot| (snapshot.id, snapshot))
            .collect::<BTreeMap<_, _>>();
        for (id, session) in &mut self.sessions {
            if let Some(snapshot) = by_id.get(id) {
                session.state = snapshot.state;
                session.provider_session = snapshot.provider_session.clone();
                session.failure = snapshot.failure.clone();
            }
        }
        let adoptable = published
            .iter()
            .filter(|snapshot| {
                self.pending.iter().any(|pending| {
                    pending.config == snapshot.config && pending.request_id == snapshot.request_id
                })
            })
            .map(|snapshot| (snapshot.config.clone(), snapshot.request_id))
            .collect::<Vec<_>>();
        for (config, request_id) in adoptable {
            let Some(snapshot) = published
                .iter()
                .find(|snapshot| snapshot.config == config && snapshot.request_id == request_id)
            else {
                continue;
            };
            // A queued start stays pending: the session exists, but the provider has not yet
            // confirmed a running program, so the launch is still in flight.
            if snapshot.state != plugin_runtime::ExecutionState::Starting {
                self.pending.retain(|pending| {
                    !(pending.config == config && pending.request_id == request_id)
                });
            }
            self.sessions.insert(
                snapshot.id,
                RunSession {
                    id: snapshot.id,
                    config: snapshot.config.clone(),
                    plugin: snapshot.plugin.clone(),
                    state: snapshot.state,
                    provider_session: snapshot.provider_session.clone(),
                    failure: snapshot.failure.clone(),
                },
            );
        }
    }

    /// Whether a launch request for this configuration is still awaiting its session identity.
    pub fn is_pending(&self, config: &str) -> bool {
        self.pending.iter().any(|pending| pending.config == config)
    }
}

/// One entry of the unified run dropdown, described before it is handed to the menu component.
///
/// The description comes from the run state alone, so the grouping a user sees can be checked
/// without depending on how the menu component happens to paint itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunMenuEntry {
    /// A session, revealed by selecting it.
    Session { id: u64, label: String },
    /// A saved configuration, which becomes the next launch target when selected.
    Configuration { id: String, label: String },
    /// An action that changes the configuration list rather than starting anything.
    Action {
        id: String,
        label: String,
        enabled: bool,
    },
    /// A group boundary between sessions, configurations and the edit entries.
    Separator,
}

impl RunControls {
    /// Describe the unified dropdown: sessions first, then saved configurations, then the edits.
    pub fn menu_entries(&self) -> Vec<RunMenuEntry> {
        let mut entries = Vec::new();
        if self.sessions.is_empty() {
            entries.push(RunMenuEntry::Action {
                id: "run-none".into(),
                label: "(没有运行中的会话)".into(),
                enabled: false,
            });
        } else {
            for session in self.sessions.values() {
                // The provider's own session word is what the user can recognise in its own panel.
                let label = format!(
                    "{} · {}",
                    session
                        .provider_session
                        .clone()
                        .unwrap_or_else(|| session.id.to_string()),
                    session_state_word(session.state)
                );
                entries.push(RunMenuEntry::Session {
                    id: session.id,
                    label,
                });
            }
        }
        entries.push(RunMenuEntry::Separator);
        if self.configs.configurations.is_empty() {
            entries.push(RunMenuEntry::Action {
                id: "run-empty".into(),
                label: "(尚未保存运行配置)".into(),
                enabled: false,
            });
        } else {
            for config in &self.configs.configurations {
                let selected = self.configs.selected.as_deref() == Some(config.id.as_str());
                entries.push(RunMenuEntry::Configuration {
                    id: config.id.clone(),
                    label: if selected {
                        format!("{} ✓", config.name)
                    } else {
                        config.name.clone()
                    },
                });
            }
        }
        entries.push(RunMenuEntry::Separator);
        entries.push(RunMenuEntry::Action {
            id: "run-edit".into(),
            label: "编辑所选配置…".into(),
            enabled: self.configs.selected.is_some(),
        });
        entries.push(RunMenuEntry::Action {
            id: "run-new".into(),
            label: "新建运行配置…".into(),
            enabled: true,
        });
        entries.push(RunMenuEntry::Action {
            id: "run-discover".into(),
            label: "发现配置（待插件贡献）".into(),
            enabled: false,
        });
        entries
    }
}

/// The state word shown beside a session in the dropdown and in the title bar.
pub fn session_state_word(state: plugin_runtime::ExecutionState) -> &'static str {
    match state {
        plugin_runtime::ExecutionState::Starting => "启动中",
        plugin_runtime::ExecutionState::Running => "运行中",
        plugin_runtime::ExecutionState::Failed => "已结束",
    }
}

/// A configuration the user is editing before it is saved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunConfigDraft {
    pub id: String,
    pub name: String,
    /// Program mode starts an executable directly; shell mode interprets the script text.
    ///
    /// The two are separate modes rather than one field with quoting rules: a program's arguments
    /// are never joined into a command line, and a script is never split into arguments.
    pub shell: bool,
    pub program: String,
    /// One argument per line, so a value containing spaces is never re-split.
    pub arguments: String,
    /// Script text for shell mode, passed to the interpreter as one final argument.
    pub script: String,
    pub directory: String,
    /// Environment entries as `名称=值`, one per line; values keep everything after the first `=`.
    pub environment: String,
    /// Tool directories searched before the inherited path, one per line.
    pub tool_paths: String,
    /// Build actions as `名称 = 程序或解释器 | 参数 | 脚本`, one per line.
    pub build: String,
    /// Steps that run in order before the program, in the same line form as the build actions.
    pub prelaunch: String,
}

impl RunConfigDraft {
    /// Start a draft from a stored configuration, or an empty draft for a new one.
    pub fn from_config(config: Option<&RunConfig>, id: String) -> Self {
        match config {
            Some(config) => Self {
                id: config.id.clone(),
                name: config.name.clone(),
                shell: matches!(config.target, RunTarget::Script { .. }),
                program: config.target.executable().to_owned(),
                arguments: match &config.target {
                    RunTarget::Program { args, .. } | RunTarget::Script { args, .. } => {
                        args.join("\n")
                    }
                },
                script: match &config.target {
                    RunTarget::Script { script, .. } => script.clone(),
                    RunTarget::Program { .. } => String::new(),
                },
                directory: config.directory.clone().unwrap_or_default(),
                environment: render_environment(&config.env),
                tool_paths: config.tool_paths.join("\n"),
                build: render_steps(&config.build),
                prelaunch: render_steps(&config.prelaunch),
            },
            None => Self {
                id,
                name: String::new(),
                shell: false,
                program: String::new(),
                arguments: String::new(),
                script: String::new(),
                directory: String::new(),
                environment: String::new(),
                tool_paths: String::new(),
                build: String::new(),
                prelaunch: String::new(),
            },
        }
    }

    /// Build the configuration this draft describes.
    ///
    /// The mode decides which fields mean what: program mode keeps a literal argv and never composes
    /// a command line, shell mode passes the script text to the named interpreter as one argument.
    /// A malformed environment line is reported here, so an unusable entry never reaches a launch.
    pub fn to_config(&self) -> Result<RunConfig, String> {
        let arguments = self
            .arguments
            .lines()
            .map(str::to_owned)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        let target = if self.shell {
            RunTarget::Script {
                interpreter: self.program.trim().to_owned(),
                args: arguments,
                script: self.script.clone(),
            }
        } else {
            RunTarget::Program {
                program: self.program.trim().to_owned(),
                args: arguments,
            }
        };
        Ok(RunConfig {
            id: self.id.clone(),
            name: self.name.trim().to_owned(),
            target,
            directory: (!self.directory.trim().is_empty())
                .then(|| self.directory.trim().to_owned()),
            env: parse_environment(&self.environment)?,
            tool_paths: self
                .tool_paths
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect(),
            build: parse_steps(&self.build)?,
            prelaunch: parse_steps(&self.prelaunch)?,
            local: true,
        })
    }
}

/// Read one prepared action per line: `名称 = 程序或解释器 | 参数 | 参数`.
///
/// The name comes first so a failure can say which step stopped the sequence, and arguments stay
/// separate items rather than one command line, exactly as the stored action is defined.
pub fn parse_steps(text: &str) -> Result<Vec<editor_core::RunStep>, String> {
    let mut steps = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((name, rest)) = line.split_once('=') else {
            return Err(format!("步骤需要写成 名称 = 程序 | 参数：{line}"));
        };
        let name = name.trim();
        if name.is_empty() {
            return Err(format!("步骤缺少名称：{line}"));
        }
        let mut parts = rest.split('|').map(str::trim);
        let executable = parts.next().unwrap_or_default();
        if executable.is_empty() {
            return Err(format!("步骤缺少要运行的程序：{line}"));
        }
        if executable.starts_with('@') && executable.trim().len() == 1 {
            return Err(format!("构建引用缺少配置名称：{line}"));
        }
        let arguments = parts
            .map(str::to_owned)
            .filter(|argument| !argument.is_empty())
            .collect::<Vec<_>>();
        // `@名称` runs the build actions of the configuration with that name; anything else starts
        // what it names. A reference is stored as an identity, never as the commands it stands for.
        let target = if let Some(reference) = executable.strip_prefix('@') {
            editor_core::StepTarget::Build {
                config: reference.trim().to_owned(),
            }
        } else {
            editor_core::StepTarget::Action {
                target: RunTarget::Program {
                    program: executable.to_owned(),
                    args: arguments,
                },
            }
        };
        steps.push(editor_core::RunStep {
            name: name.to_owned(),
            target,
        });
    }
    Ok(steps)
}

/// Render stored actions back into the one-per-line form the field edits.
pub fn render_steps(steps: &[editor_core::RunStep]) -> String {
    steps
        .iter()
        .map(|step| match &step.target {
            editor_core::StepTarget::Action { target } => {
                let mut line = format!("{} = {}", step.name, target.executable());
                for argument in target.arguments() {
                    line.push_str(" | ");
                    line.push_str(argument);
                }
                line
            }
            editor_core::StepTarget::Build { config } => format!("{} = @{}", step.name, config),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Read one `NAME=VALUE` entry per line, refusing anything that is not an environment entry.
///
/// Values are kept verbatim after the first `=`, so a value containing `=` or spaces is never
/// split, and an empty value is a real value rather than a missing one.
pub fn parse_environment(text: &str) -> Result<BTreeMap<String, String>, String> {
    let mut entries = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            return Err(format!("环境变量需要写成 名称=值：{line}"));
        };
        let name = name.trim();
        if name.is_empty() {
            return Err(format!("环境变量缺少名称：{line}"));
        }
        entries.insert(name.to_owned(), value.to_owned());
    }
    Ok(entries)
}

/// Render stored entries back into the one-per-line form the field edits.
pub fn render_environment(env: &BTreeMap<String, String>) -> String {
    env.iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("\n")
}
