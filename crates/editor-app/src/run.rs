//! Run controls: saved configurations, their launches, and the sessions they produced.
//!
//! The editor owns the configuration and the visible session list; the execution itself belongs to
//! whichever compatible provider the runtime selects. Nothing here inspects a program name to decide
//! behavior: a configuration is either valid, or it is not.
use crate::extensions::HostRunSnapshot;
use editor_core::{RunConfig, RunConfigSet, RunTarget};
use rust_i18n::t;
use std::collections::BTreeMap;

mod debug_presentation;
mod provider_preparation;
pub(crate) mod ui;
pub use ui::RunConfigForm;
pub(crate) use ui::RunMenu;
#[cfg(test)]
pub(crate) use ui::{RunField, StepEdit};
mod sequence;
#[cfg(test)]
pub use sequence::StepState;
pub use sequence::{RunSequence, SequenceAction, StepOutcome};
#[cfg(test)]
mod regression_tests;
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
    pub fn label(self) -> String {
        match self {
            Self::Build => t!("run.phase_build").to_string(),
            Self::Prelaunch => t!("run.phase_prelaunch").to_string(),
            Self::Program => t!("run.phase_program").to_string(),
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
    /// Provider build replaces a placeholder; final launch requires its actual artifact receipt.
    pub preparation: Option<(String, String)>,
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
    /// Whether this plan starts a program at the end of its sequence.
    ///
    /// Kept as a question about the last step rather than a stored flag: the sequence used to keep a
    /// copy, and removing the copy is what showed the flag was never read.
    #[cfg(test)]
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
            plugin_runtime::ExecutionState::Starting
            | plugin_runtime::ExecutionState::Running
            | plugin_runtime::ExecutionState::Stopping
            | plugin_runtime::ExecutionState::Terminating => true,
            plugin_runtime::ExecutionState::Failed | plugin_runtime::ExecutionState::Exited => {
                false
            }
        }
    }
}

/// Most actions one launch prepares, including the program itself.
///
/// This is the bound a sequence is refused past, so a mistaken reference chain cannot turn one click
/// into an unbounded number of programs.
pub const MAX_PREPARED_STEPS: usize = editor_core::MAX_RUN_STEPS + 1;

/// Saved configurations plus everything this editor has launched from them.
///
/// Cloning is what lets a dialog edit a snapshot of the stored configurations without holding a
/// borrow of the editor that owns them; it is not a second source of run state.
#[derive(Clone, Debug)]
pub struct RunControls {
    /// Bounded output histories keyed by authenticated preparation requests, never by plugin panel.
    provider_preparations: BTreeMap<u64, provider_preparation::ProviderPreparationView>,
    preparation_output: Option<u64>,
    pub(super) preparation_output_open: bool,
    /// Replacement intent retains the original preparation and launch/debug mode until actual cleanup.
    preparation_reruns: BTreeMap<String, (u64, bool)>,
    configs: RunConfigSet,
    /// Identity assigned to the next local session, used only before the runtime answers.
    next_request: u64,
    pending: Vec<PendingRun>,
    /// Stop requests awaiting their provider's answer, keyed by the configuration they stop.
    stops: Vec<PendingStop>,
    /// Explicit replacements wait on the original session, independent of subsequent selection.
    reruns: BTreeMap<String, u64>,
    /// The exact historical or active session most recently selected for location.
    location: Option<(u64, u64, String)>,
    sessions: BTreeMap<u64, RunSession>,
    /// Preparation in progress, keyed by the configuration whose launch or build owns it.
    ///
    /// One configuration prepares once at a time: a second click while its own build is running must
    /// not start a parallel preparation of the same code.
    sequences: BTreeMap<String, RunSequence>,
    /// Session identities that belong to a preparation step, keyed by session.
    ///
    /// A step's program is an ordinary session, but it is preparation rather than something the user
    /// started, so this is what tells the two apart when the runtime publishes state.
    step_sessions: BTreeMap<u64, (String, usize)>,
    /// Status queries awaiting their provider's answer, keyed by the request identity.
    polls: Vec<PendingPoll>,
    /// The step each staged request prepares, keyed by the request identity.
    ///
    /// The request identity is this editor's own, so a session is joined to its step by the identity
    /// the launch was requested under rather than by anything a provider reports back.
    step_requests: BTreeMap<u64, (String, usize)>,
    /// Storage directory used for host-local configuration files.
    root: Option<std::path::PathBuf>,
    /// Workspace directory holding this project's shared file, when the workspace has one.
    project: Option<std::path::PathBuf>,
    /// The project's shared entries as the file currently holds them.
    shared: editor_core::SharedSet,
    /// The targets the installed plugins last offered for this workspace.
    discovered: Vec<plugin_schema::DiscoveredTarget>,
    /// Whether a discovery has run at all, so an empty list is not mistaken for "not yet asked".
    discovery_ran: bool,
    /// A failed source preserves prior candidates but blocks launches until repaired discovery.
    discovery_failures: BTreeMap<String, String>,
    declarative_discovery_error: Option<String>,
    /// An invocation nonce distinguishes asynchronous discovery from an abandoned workspace form.
    discovery_request: Option<u64>,
    /// Which provider a debug launch would use, or the reason there is none.
    debug_availability: Option<Result<String, String>>,
    /// Every debug session this editor is running, one per configuration.
    debug_sessions: editor_core::DebugSessions,
    /// Whether a control action is in flight for the selected session.
    ///
    /// While one is outstanding the session's state is about to change, so the controls are withheld
    /// rather than letting a second click race the answer that has not arrived.
    debug_action_in_flight: std::collections::BTreeSet<String>,
    /// Provider epochs distinguish repeated stops at the same source line and reject old state replies.
    debug_epochs: std::collections::BTreeMap<String, u64>,
    /// Actual owning provider is immutable for a target, independent of later default changes.
    debug_owners: std::collections::BTreeMap<String, String>,
    /// Replacing a debug target waits for the original target's actual final observation.
    debug_reruns: std::collections::BTreeMap<String, String>,
    /// Source often arrives after the stopped receipt; locate each reported pause at most once.
    debug_position_epochs: std::collections::BTreeMap<String, u64>,
    /// Frozen final debug requests wait until every ordinary preparation step actually succeeds.
    debug_preparations: std::collections::BTreeMap<String, serde_json::Value>,
    /// Resolution may fill only the original frozen final binding, not a later edited configuration.
    debug_preparation_bindings: std::collections::BTreeMap<String, (String, String)>,
    /// Whether the editor should move to where the target next stops.
    ///
    /// True from the start of a session, because a breakpoint hit is a pause the user did not ask
    /// for and wants to be shown; false after the user resumes or steps, because then they are
    /// driving and the caret should stay where they put it.
    debug_position_followed: bool,
    /// Debug requests this editor has sent and not yet answered, oldest first.
    ///
    /// A debug call is asynchronous, so the scope a request belongs to is recorded with it: an answer
    /// is applied to the pause it was asked about, and one that arrives after that pause ended is
    /// reported rather than applied.
    debug_requests: Vec<PendingDebugRequest>,
    /// Strictly increasing even after completion, so a delayed response cannot name a newer request.
    next_debug_request: u64,
    /// What the selected debug provider declared it can do.
    ///
    /// Defaulting to nothing is deliberate: an ability the host has not been told about is not one it
    /// may offer, so a provider that has not been asked leaves its controls disabled with a reason.
    debug_capabilities: editor_core::DebugCapabilities,
    /// Live targets use the declaration of their original owner; defaults only choose new launches.
    debug_provider_capabilities: std::collections::BTreeMap<String, editor_core::DebugCapabilities>,
    /// Which configured breakpoints the running provider could actually bind.
    ///
    /// The set is per session, not per configuration: the same position may be bindable under one
    /// target and not another, so this is cleared when a session begins and replaced by each answer.
    /// A position absent from here has not been reported either way, which is not the same as unverified.
    debug_breakpoints_verified: BTreeMap<String, Vec<(String, u32, bool)>>,
    /// Set when the stored file could not be read or written; shown instead of silently defaulting.
    pub error: Option<String>,
}

/// What one plugin's removal, disablement or update would do to the sessions it is serving.
///
/// Collected before anything destructive happens, so the user decides with the affected sessions
/// named rather than discovering afterwards that a program or a debug session was taken away. Both
/// kinds are reported because a plugin may serve either, and a session belongs to the plugin that
/// answered it rather than to the one that happens to be installed now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PluginSessionImpact {
    /// Running programs, by configuration name, in the order the sessions were started.
    pub running: Vec<String>,
    /// Debug sessions, by configuration name.
    pub debugging: Vec<String>,
}

impl PluginSessionImpact {
    /// Whether this plugin is serving anything that would be taken away.
    pub fn is_empty(&self) -> bool {
        self.running.is_empty() && self.debugging.is_empty()
    }

    /// What to tell the user before the change, or `None` when nothing is affected.
    pub fn summary(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let mut parts = Vec::new();
        if !self.running.is_empty() {
            parts.push(
                t!(
                    "run.impact_running",
                    count = self.running.len(),
                    names = self.running.join(", ")
                )
                .to_string(),
            );
        }
        if !self.debugging.is_empty() {
            parts.push(
                t!(
                    "run.impact_debugging",
                    count = self.debugging.len(),
                    names = self.debugging.join(", ")
                )
                .to_string(),
            );
        }
        Some(parts.join("；"))
    }
}

/// One debug request this editor is waiting on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingDebugRequest {
    /// Identity the editor assigns, so an answer is joined to the request it answers.
    pub id: u64,
    /// Which debug method was asked for.
    pub method: DebugMethod,
    /// The pause the answer will describe, captured when the request was sent.
    pub scope: editor_core::PauseScope,
    /// The frame a variable request asked about.
    pub frame: Option<u32>,
    /// The owning configuration, including inspection requests whose pause numbers may coincide.
    pub config: Option<String>,
}

/// The debug methods this editor calls, so an answer can be joined to what it answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugMethod {
    /// Begin a session. The only method that is not about a pause, because it is what starts one.
    Start,
    /// Set the configuration's breakpoints, which belongs to the session rather than to any pause.
    Breakpoints,
    Frames,
    Variables,
    /// Move the paused target, in one of the three directions.
    Step(editor_core::DebugStep),
    /// State-changing controls retain their request/configuration association outside a pause.
    Control(DebugControl),
}

/// Typed control intent prevents the shared request ID zero from targeting a different session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugControl {
    Resume,
    Pause,
    Stop,
    /// Explicit host job revocation may upgrade a normal stop already awaiting adapter completion.
    Force,
}

/// One frame row of the debug panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugFrameRow {
    /// Identity the view attaches to the row, so a click can be traced back to this frame.
    pub selector: String,
    /// What the row reads: the frame's name and the location it would take the user to.
    pub label: String,
    pub frame: u32,
    /// Whether this is the frame whose variables are shown.
    pub selected: bool,
}

/// One variable row of the debug panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugVariableRow {
    pub selector: String,
    /// The provider's own rendering of the value, shown as given.
    pub label: String,
}

/// Everything the debug panel shows about the selected session.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DebugPanelRows {
    pub frames: Vec<DebugFrameRow>,
    /// The variables of the selected frame, empty when no frame is selected or none were read.
    pub variables: Vec<DebugVariableRow>,
    /// Where the selected frame is, as the location a user would be taken to.
    pub location: Option<String>,
    /// Whether another session is stopped, which is what keeps the view from being moved.
    pub another_paused: bool,
}

/// What one discovery run did to the stored configurations.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiscoveryReport {
    /// Configurations whose target changed and needs the user's repair confirmation, by name.
    pub repaired: Vec<String>,
    /// Targets no configuration claims, which the user may confirm.
    pub offered: Vec<String>,
    /// Configurations whose target is no longer offered, by identity and name.
    pub missing: Vec<(String, String)>,
}

/// A status query this editor made about one preparation step.
#[derive(Clone, Debug)]
pub struct PendingPoll {
    pub config: String,
    pub index: usize,
    /// The session the query is about.
    ///
    /// Stored rather than derived: a poll is recorded before the provider answers, and this is what
    /// identifies the program the answer describes if the sequence's own bookkeeping moves on in the
    /// meantime. Nothing reads it today — the step is matched by request identity instead — and it is
    /// kept because a status query without its subject is not a description of anything.
    #[allow(dead_code)]
    pub session: u64,
    pub request_id: u64,
}

/// Controls without a storage directory, used while a workspace has no host-local state yet.
impl Default for RunControls {
    fn default() -> Self {
        Self {
            provider_preparations: Default::default(),
            preparation_output: None,
            preparation_output_open: false,
            preparation_reruns: Default::default(),
            configs: RunConfigSet::default(),
            next_request: 0,
            pending: Vec::new(),
            stops: Vec::new(),
            reruns: BTreeMap::new(),
            location: None,
            sessions: BTreeMap::new(),
            sequences: BTreeMap::new(),
            step_sessions: BTreeMap::new(),
            polls: Vec::new(),
            step_requests: BTreeMap::new(),
            root: None,
            project: None,
            shared: editor_core::SharedSet::default(),
            discovered: Vec::new(),
            discovery_ran: false,
            discovery_failures: BTreeMap::new(),
            declarative_discovery_error: None,
            discovery_request: None,
            debug_availability: None,
            debug_sessions: editor_core::DebugSessions::default(),
            debug_action_in_flight: Default::default(),
            debug_epochs: Default::default(),
            debug_owners: Default::default(),
            debug_reruns: Default::default(),
            debug_position_epochs: Default::default(),
            debug_preparations: Default::default(),
            debug_preparation_bindings: Default::default(),
            debug_position_followed: true,
            debug_requests: Vec::new(),
            next_debug_request: 0,
            debug_capabilities: editor_core::DebugCapabilities::default(),
            debug_provider_capabilities: Default::default(),
            debug_breakpoints_verified: BTreeMap::new(),
            error: None,
        }
    }
}

impl RunControls {
    /// Load the configurations stored for one workspace, reporting an unreadable file.
    #[cfg(test)]
    pub fn load(workspace: &str, root: Option<std::path::PathBuf>) -> Self {
        Self::load_with_project(workspace, root, None)
    }

    /// Load this machine's configurations and merge the project's shared ones into them.
    ///
    /// The project file is read through the same validation the form uses, so a hand-edited file that
    /// breaks a rule is reported here rather than at launch time; a file that cannot be read leaves
    /// the machine's own configurations in place instead of replacing them.
    pub fn load_with_project(
        workspace: &str,
        root: Option<std::path::PathBuf>,
        project: Option<std::path::PathBuf>,
    ) -> Self {
        let mut controls = Self {
            root: root.clone(),
            project: project.clone(),
            ..Self::default()
        };
        let Some(root) = root else {
            return controls;
        };
        let local = match editor_core::load(&root, workspace) {
            Ok(configs) => configs,
            // An unreadable file is never replaced by defaults: the user's previous configurations
            // must survive until they decide what to do with the file.
            Err(error) => {
                controls.error = Some(error.to_string());
                return controls;
            }
        };
        controls.configs = local;
        let Some(project) = project else {
            return controls;
        };
        match editor_core::load_shared(&project) {
            Ok(shared) => {
                controls.shared = shared.clone();
                controls.configs = editor_core::merge(&project, &controls.configs, &shared);
            }
            Err(error) => {
                controls.configs = editor_core::merge(
                    &project,
                    &controls.configs,
                    &editor_core::SharedSet::default(),
                );
                controls.error = Some(error.to_string());
            }
        }
        controls
    }

    /// Whether a preparation can still query its provider for the actual result of this session.
    ///
    /// An `Exited` snapshot does not carry an exit code and can arrive before the dedicated status
    /// reply. Keep polling it until that reply settles the step; only a failed session is unqueryable.
    pub fn session_can_report_result(&self, session: u64) -> bool {
        self.sessions
            .get(&session)
            .is_some_and(|session| !matches!(session.state, plugin_runtime::ExecutionState::Failed))
    }

    /// Choose the next preparation action while preserving an actual runtime failure's diagnostic.
    /// Snapshot failure is more specific than an unavailable exit-code reply and must stay visible.
    pub fn preparation_action(&self, configuration: &str) -> Option<SequenceAction> {
        let sequence = self.preparation(configuration)?;
        if let Some(session) = sequence.current_session()
            && let Some(message) = self
                .sessions
                .get(&session)
                .and_then(|session| session.failure.as_deref())
        {
            let name = sequence
                .current_step()
                .map(|step| step.name.as_str())
                .unwrap_or(configuration);
            return Some(SequenceAction::Blocked {
                reason: t!("run.step_failed", name = name, message = message).to_string(),
            });
        }
        Some(sequence.next_action(
            |session| self.session_known(session),
            |session| self.session_can_report_result(session),
        ))
    }

    /// Whether a session is known at all, so a preparation can tell "not yet published" from "gone".
    pub fn session_known(&self, session: u64) -> bool {
        self.sessions.contains_key(&session)
    }

    /// Remember that one preparation step has been asked for, and under which launch identity.
    ///
    /// This happens as the request is staged, so a step is owned from the moment it is requested
    /// rather than from the moment its session appears.
    pub fn note_step_request(&mut self, config: &str, index: usize, request_id: u64) {
        if let Some(sequence) = self.sequences.get_mut(config) {
            sequence.requested(index, request_id);
        }
        self.step_requests
            .insert(request_id, (config.to_owned(), index));
    }

    /// Whether a status query for this step is already outstanding.
    pub fn has_poll(&self, config: &str, index: usize) -> bool {
        self.polls
            .iter()
            .any(|poll| poll.config == config && poll.index == index)
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
            let _ = self.persist_local(workspace);
        }
        selected
    }

    /// Save or replace one configuration and persist the result.
    pub fn upsert(&mut self, configuration: RunConfig, workspace: &str) -> Result<(), String> {
        // A rejected configuration is reported here as well as returned, so the visible error and the
        // refused edit cannot disagree.
        let previous = self.configs.clone();
        if let Err(error) = self.configs.upsert(configuration.clone()) {
            let message = error.to_string();
            self.error = Some(message.clone());
            return Err(message);
        }
        let result = self.persist_configuration(&configuration, workspace);
        if result.is_err() {
            // A refused edit cannot become the in-memory command a subsequent click executes.
            self.configs = previous;
        }
        result
    }

    /// Remove one configuration from wherever it was stored.
    pub fn remove(&mut self, id: &str, workspace: &str) -> Result<(), String> {
        let shared = if self
            .shared
            .configurations
            .iter()
            .any(|entry| entry.id == id)
        {
            if let Some(project) = self.project.clone() {
                // Preserve other entries that may have been edited outside this editor.
                let mut shared =
                    editor_core::load_shared(&project).map_err(|error| error.to_string())?;
                shared.configurations.retain(|entry| entry.id != id);
                Some(shared)
            } else {
                None
            }
        } else {
            None
        };
        let previous = self.configs.clone();
        self.configs.remove(id);
        // A removed configuration's finished sessions stay visible; running ones are not hidden.
        let result = self.persist_edit(workspace, shared);
        if result.is_err() {
            self.configs = previous;
        }
        result
    }

    /// Confirm one discovered candidate and store it as an editable configuration.
    ///
    /// This is the only way a discovery adds anything: the user confirms it, and it becomes an
    /// ordinary configuration from that moment on. Confirming the same target twice resolves to the
    /// configuration that already exists instead of saving a second copy.
    pub fn confirm_target(&mut self, target_id: &str, workspace: &str) -> Result<String, String> {
        // A stored identity remains selectable after a refresh reports it missing; launches
        // still validate the current catalog, and confirmation must never make a duplicate.
        if let Some(existing) = self
            .configs
            .configurations
            .iter()
            .find(|config| config.from_target.as_deref() == Some(target_id))
        {
            let id = existing.id.clone();
            self.select(&id, workspace);
            return Ok(id);
        }
        let candidate = self
            .discovered
            .iter()
            .find(|target| target.id == target_id)
            .ok_or_else(|| format!("{} ({target_id})", t!("run.target_missing")))?;
        if let Some(existing) = self
            .configs
            .configurations
            .iter()
            .find(|config| config.claims_target(candidate))
        {
            let id = existing.id.clone();
            self.select(&id, workspace);
            return Ok(id);
        }
        let target = self
            .discovered
            .iter()
            .find(|target| target.id == target_id)
            .cloned()
            .ok_or_else(|| t!("run.target_not_discovered", id = target_id).to_string())?;
        let id = self.generate_id(workspace);
        // The target's display label is the natural name, so the user recognizes what they confirmed
        // and can rename it in the form immediately afterwards.
        let configuration =
            editor_core::configuration_for(&target, id.clone(), target.label.clone());
        self.upsert(configuration, workspace)?;
        self.select(&id, workspace);
        Ok(id)
    }

    /// Identity for a new configuration in this workspace.
    pub fn generate_id(&self, workspace: &str) -> String {
        self.configs.generate_id(workspace)
    }

    /// Write this machine's own record of every configuration it knows about.
    ///
    /// The record is complete rather than partial, so this file is always readable on its own. What is
    /// shared is the project's file: a shared entry's portable half is read from there on every load,
    /// so this copy cannot drift the configuration that actually runs.
    fn persist_local(&mut self, workspace: &str) -> Result<(), String> {
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

    /// Write the project's shared file, creating it only when something is actually shared.
    fn persist_shared(&mut self, shared: &editor_core::SharedSet) -> Result<(), String> {
        let Some(project) = self.project.clone() else {
            return Ok(());
        };
        let path = editor_core::project_path(&project);
        // Nothing is written into a project until a user shares something with it, so a workspace
        // that never shared has no `.editor` directory at all.
        if shared.configurations.is_empty() && !path.exists() {
            return Ok(());
        }
        // Once the project has a file it stays a current document: unsharing the last entry removes it
        // rather than leaving an entry that would reappear on the next load.
        match editor_core::save_shared(&project, shared) {
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

    /// Write one configuration to the place its sharing choice selects.
    ///
    /// Sharing moves the portable half into the project's file and leaves this machine's own values
    /// here; unsharing removes it from the project's file, so a configuration that is no longer shared
    /// stops being read by anyone else's editor. Neither direction deletes what the user typed.
    fn persist_configuration(&mut self, config: &RunConfig, workspace: &str) -> Result<(), String> {
        let changes_shared = !config.local
            || self
                .shared
                .configurations
                .iter()
                .any(|entry| entry.id == config.id);
        let shared = if changes_shared && let Some(project) = self.project.clone() {
            // Only an explicit shared edit writes the project. Read the current file first, so
            // unrelated external edits survive and malformed/newer files are never overwritten.
            let mut shared = match editor_core::load_shared(&project) {
                Ok(shared) => shared,
                Err(error) => {
                    let message = error.to_string();
                    self.error = Some(message.clone());
                    return Err(message);
                }
            };
            if config.local {
                shared.configurations.retain(|entry| entry.id != config.id);
            } else {
                shared.upsert(editor_core::SharedConfig::from_config(config, &project));
            }
            Some(shared)
        } else {
            None
        };
        self.persist_edit(workspace, shared)
    }

    /// Commit local values before publishing a project edit, restoring them if publication fails.
    ///
    /// The shared file is the final commit boundary: local I/O failure must never publish a command
    /// the form reports as rejected. Both files use atomic replacement. A second I/O failure during
    /// rollback is reported explicitly rather than claiming a two-file filesystem transaction.
    fn persist_edit(
        &mut self,
        workspace: &str,
        shared: Option<editor_core::SharedSet>,
    ) -> Result<(), String> {
        let Some(shared) = shared else {
            return self.persist_local(workspace);
        };
        let previous_local = (|| {
            shared.validate().map_err(|error| error.to_string())?;
            self.root
                .as_ref()
                .map(|root| editor_core::load(root, workspace).map_err(|error| error.to_string()))
                .transpose()
        })()
        .inspect_err(|message| self.error = Some(message.clone()))?;
        self.persist_local(workspace)?;
        if let Err(message) = self.persist_shared(&shared) {
            let rollback = self
                .root
                .as_ref()
                .zip(previous_local.as_ref())
                .map(|(root, previous)| editor_core::save(root, workspace, previous))
                .transpose();
            let message = match rollback {
                Ok(_) => message,
                Err(error) => t!(
                    "run.local_restore_failed",
                    error = message,
                    restore = error.to_string()
                )
                .to_string(),
            };
            self.error = Some(message.clone());
            return Err(message);
        }
        self.shared = shared;
        Ok(())
    }

    /// Read one current project snapshot before planning any side effects.
    ///
    /// Cached project entries supply local overrides only. Hand edits, deletion and parse errors in
    /// the open window have the same meaning as reopening it; preparation uses this immutable set.
    fn configurations_for_launch(&self) -> Result<RunConfigSet, String> {
        let Some(project) = &self.project else {
            return Ok(self.configs.clone());
        };
        let shared = editor_core::load_shared(project).map_err(|error| error.to_string())?;
        Ok(editor_core::merge(project, &self.configs, &shared))
    }

    /// All sessions this editor knows about, newest request last.
    pub fn sessions(&self) -> Vec<RunSession> {
        self.sessions.values().cloned().collect()
    }
    /// Location has its own request identity; an ended session is still a valid selection.
    pub fn begin_location(&mut self, session: u64, config: &str) -> u64 {
        self.next_request += 1;
        self.location = Some((session, self.next_request, config.into()));
        self.next_request
    }
    /// Accept only the newest selection reply, including honest expiry of retained output.
    pub fn finish_location(&mut self, session: u64, request: u64) -> bool {
        if !self
            .location
            .as_ref()
            .is_some_and(|pending| pending.0 == session && pending.1 == request)
        {
            return false;
        }
        let (_, _, config) = self.location.take().unwrap();
        self.selected()
            .is_some_and(|selected| selected.id == config)
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
        if let Some(session) = self.running_for(id) {
            // A repeat locates the original launch snapshot even while its shared definition is
            // being edited or has been removed; only a new execution reads the current document.
            return LaunchPlan::Existing {
                session: session.id,
            };
        }
        let configs = match self.configurations_for_launch() {
            Ok(configs) => configs,
            Err(message) => return LaunchPlan::Invalid { message },
        };
        let Some(config) = configs.find(id) else {
            return LaunchPlan::Invalid {
                message: t!("run.configuration_missing").to_string().into(),
            };
        };
        if let Err(error) = config.validate() {
            return LaunchPlan::Invalid {
                message: error.to_string(),
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

    /// The configuration's own build actions, and nothing else.
    ///
    /// This is what the Build control runs: a build never executes a pre-launch step, because those
    /// exist to prepare a launch rather than to produce the build's output.
    pub fn prepare_build(
        &self,
        id: &str,
        workspace_root: &str,
        limit: usize,
    ) -> Result<RunPlan, String> {
        let configs = self.configurations_for_launch()?;
        let config = configs
            .configurations
            .iter()
            .find(|config| config.id == id)
            .ok_or_else(|| t!("run.configuration_missing").to_string().to_owned())?;
        self.validate_build_configuration(config)?;
        let mut steps = Vec::new();
        for step in &config.build {
            steps.push(self.action_step(
                config,
                StepKind::Build,
                &step.name,
                &step.target,
                workspace_root,
            )?);
        }
        if steps.is_empty() {
            return Err(t!("run.no_build_actions").to_string().to_owned());
        }
        if limit == 0 || steps.len() > limit {
            return Err(t!("run.build_limit", limit = limit.to_string()).to_string());
        }
        Ok(RunPlan { steps })
    }

    /// Direct and referenced builds share target validity, matching preparation and empty-list rules.
    /// The caller supplies its single launch snapshot so file edits never split admission from execution.
    fn validate_build_configuration(&self, config: &RunConfig) -> Result<(), String> {
        config.validate().map_err(|error| error.to_string())?;
        if let Some(error) = self.discovery_blocker(config) {
            return Err(error);
        }
        if self.configuration_target_missing(config) {
            return Err(t!("run.target_missing").into());
        }
        if let Some(binding)=provided_binding(&config.target) && !config.build.iter().any(|step|
            matches!(&step.target,editor_core::StepTarget::Action {target} if provided_binding(target)==Some(binding.clone()))) {
            return Err(t!("run.provider_build_missing").into());
        }
        if config.build.is_empty() {
            return Err(t!("run.no_build_actions").into());
        }
        Ok(())
    }

    /// Every action a launch performs for one configuration, in order: build, then each pre-launch
    /// step, then the program.
    ///
    /// A step that names another configuration expands that configuration's build actions here, once,
    /// so the sequence cannot change halfway through a launch. `limit` bounds the preparation, which
    /// is what a launch computes before its program is appended.
    pub fn prepare(&self, id: &str, workspace_root: &str, limit: usize) -> Result<RunPlan, String> {
        let configs = self.configurations_for_launch()?;
        self.prepare_from(&configs, id, workspace_root, limit)
    }

    /// Expand preparation from one snapshot so external edits cannot change a plan halfway through.
    fn prepare_from(
        &self,
        configs: &RunConfigSet,
        id: &str,
        workspace_root: &str,
        limit: usize,
    ) -> Result<RunPlan, String> {
        let config = configs
            .configurations
            .iter()
            .find(|config| config.id == id)
            .ok_or_else(|| t!("run.configuration_missing").to_string().to_owned())?;
        if let Some(error) = self.discovery_blocker(config) {
            return Err(error);
        }
        if self.configuration_target_missing(config) {
            return Err(t!("run.target_missing").into());
        }
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
                    let referenced = configs
                        .configurations
                        .iter()
                        .find(|candidate| candidate.name == *name)
                        .ok_or_else(|| {
                            t!("run.reference_missing", step = &step.name, name = name).to_string()
                        })?;
                    if referenced.id == config.id {
                        return Err(t!("run.reference_self", step = &step.name).to_string());
                    }
                    self.validate_build_configuration(referenced)?;
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
            return Err(t!("run.preparation_limit", limit = limit.to_string()).to_string());
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
        let configs = self.configurations_for_launch()?;
        self.prepare_launch_from(&configs, id, workspace_root, limit)
    }

    /// Keep the final program and its preparation bound to the same validated project document.
    fn prepare_launch_from(
        &self,
        configs: &RunConfigSet,
        id: &str,
        workspace_root: &str,
        limit: usize,
    ) -> Result<RunPlan, String> {
        // The program is one of the bounded actions, so preparation may use one fewer. A plan that is
        // only the program is always allowed, which is what a configuration without steps produces.
        let mut plan =
            self.prepare_from(configs, id, workspace_root, limit.saturating_sub(1).max(1))?;
        let config = configs
            .configurations
            .iter()
            .find(|config| config.id == id)
            .ok_or_else(|| t!("run.configuration_missing").to_string().to_owned())?;
        let directory = config
            .directory
            .clone()
            .or_else(|| Some(workspace_root.to_owned()));
        plan.steps.push(PreparedStep {
            kind: StepKind::Program,
            name: config.name.clone(),
            config: config.id.clone(),
            preparation: provided_binding(&config.target),
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
            return Err(t!("run.step_not_action", name = name).to_string());
        };
        let directory = config
            .directory
            .clone()
            .or_else(|| Some(workspace_root.to_owned()));
        Ok(PreparedStep {
            kind,
            name: name.to_owned(),
            config: config.id.clone(),
            preparation: provided_binding(target),
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
            || self.provider_preparations.values().any(|view| {
                view.config == config
                    && matches!(
                        view.snapshot.state,
                        plugin_runtime::ExecutionState::Stopping
                            | plugin_runtime::ExecutionState::Terminating
                    )
            })
            || self.sessions.values().any(|session| {
                session.config == config
                    && matches!(
                        session.state,
                        plugin_runtime::ExecutionState::Stopping
                            | plugin_runtime::ExecutionState::Terminating
                    )
            })
    }

    /// Remember one explicit rerun; a repeated click cannot replace its original exit barrier.
    pub fn wait_to_rerun(&mut self, config: &str, session: u64) {
        self.reruns.entry(config.into()).or_insert(session);
    }

    /// Return replacements once their original program is confirmed ended, or a diagnostic on failure.
    pub fn take_ready_reruns(&mut self) -> Vec<(String, Result<(), String>)> {
        let ready = self
            .reruns
            .iter()
            .filter_map(|(config, id)| {
                let session = self.sessions.get(id)?;
                if session.is_active() {
                    return None;
                }
                let result = if session.state == plugin_runtime::ExecutionState::Exited {
                    Ok(())
                } else {
                    Err(session
                        .failure
                        .clone()
                        .unwrap_or_else(|| t!("run.old_exit_unknown").into()))
                };
                Some((config.clone(), result))
            })
            .collect::<Vec<_>>();
        for (config, _) in &ready {
            self.reruns.remove(config);
        }
        ready
    }

    /// A user-requested stop or leave cancels replacement intent without cancelling the exit itself.
    pub fn cancel_rerun(&mut self, config: &str) {
        self.reruns.remove(config);
        self.preparation_reruns.remove(config);
    }

    /// Stop intent belongs to the whole preparation: even a clean exit cannot launch its next step.
    pub fn request_configuration_stop(&mut self, config: &str) {
        self.cancel_prepared_debug(config);
        if let Some(sequence) = self.sequences.get_mut(config) {
            sequence.request_stop();
        }
    }

    /// Whether leaving now would abandon managed work: a running session or an unanswered start.
    ///
    /// This is what the window asks before closing, so leaving with active programs is a decision the
    /// user makes rather than something that happens while they are editing.
    pub fn has_work_in_flight(&self) -> bool {
        !self.pending.is_empty()
            || self.sequences.values().any(RunSequence::is_active)
            || self
                .provider_preparations
                .values()
                .any(|view| view.snapshot.state.is_active())
            || !self.stops.is_empty()
            || !self.reruns.is_empty()
            || !self.debug_reruns.is_empty()
            || !self.preparation_reruns.is_empty()
            || self.sessions.values().any(|session| session.is_active())
            || self.debug_sessions.entries().any(|(_, session)| {
                matches!(
                    session.state(),
                    editor_core::DebugSessionState::Starting
                        | editor_core::DebugSessionState::Running
                        | editor_core::DebugSessionState::Paused { .. }
                )
            })
            || self
                .debug_requests
                .iter()
                .any(|request| request.method == DebugMethod::Start)
    }

    /// Every session that would be left behind, for the confirmation text.
    pub fn active_session_ids(&self) -> Vec<u64> {
        self.sessions
            .values()
            .filter(|session| session.is_active())
            .map(|session| session.id)
            .collect()
    }

    /// Count actual and pending work for the leave prompt, including paused debugger targets.
    pub fn active_work_count(&self) -> usize {
        let running = self
            .sessions
            .values()
            .filter(|session| session.is_active())
            .count();
        let preparing = self
            .pending
            .iter()
            .filter(|pending| {
                !self
                    .sessions
                    .values()
                    .any(|session| session.config == pending.config && session.is_active())
            })
            .count();
        let debugging = self
            .debug_sessions
            .entries()
            .filter(|(_, session)| {
                matches!(
                    session.state(),
                    editor_core::DebugSessionState::Starting
                        | editor_core::DebugSessionState::Running
                        | editor_core::DebugSessionState::Paused { .. }
                )
            })
            .count();
        running + preparing + debugging
    }

    /// A selected configuration has one target, regardless of ordinary or debug launch mode.
    pub fn debug_target_active(&self, config: &str) -> bool {
        self.debug_sessions.session(config).is_some_and(|session| {
            matches!(
                session.state(),
                editor_core::DebugSessionState::Starting
                    | editor_core::DebugSessionState::Running
                    | editor_core::DebugSessionState::Paused { .. }
            )
        })
    }

    /// Keep the old host identity in the replacement barrier; a new selection cannot release it.
    pub fn wait_to_debug_again(&mut self, config: &str) {
        if let Some(session) = self
            .debug_sessions
            .session(config)
            .and_then(|session| session.provider_session())
        {
            self.debug_reruns.insert(config.into(), session.into());
        }
    }

    /// Consume each requested replacement once, only after the original target ended or was revoked.
    pub fn take_ready_debug_reruns(&mut self) -> Vec<String> {
        let ready = self
            .debug_reruns
            .iter()
            .filter_map(|(config, id)| {
                self.debug_sessions
                    .session(config)
                    .filter(|session| {
                        session.provider_session() == Some(id.as_str())
                            && matches!(
                                session.state(),
                                editor_core::DebugSessionState::Exited
                                    | editor_core::DebugSessionState::Failed { .. }
                            )
                    })
                    .map(|_| config.clone())
            })
            .collect::<Vec<_>>();
        for config in &ready {
            self.debug_reruns.remove(config);
        }
        ready
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

    /// Whether a preparation has finished: everything it prepared completed, and the program it ends
    /// with — when it has one — is the session now running.
    ///
    /// This is what the native acceptance waits on, so readiness is read from the sequence's own
    /// state rather than inferred from how many sessions happen to exist.
    #[cfg(test)]
    pub fn preparation_complete(&self, config: &str) -> bool {
        let Some(sequence) = self.sequences.get(config) else {
            return true;
        };
        if sequence.blocked_by().is_some() {
            return false;
        }
        // A finished preparation whose last step is the program it launched is complete even though
        // the sequence still owns that program: the launch succeeded, which is what was asked.
        match sequence.steps().last().map(|step| &step.state) {
            Some(StepState::Running { .. }) => true,
            Some(_) => false,
            None => !sequence.is_active(),
        }
    }

    /// Whether a preparation has stopped for good, with the reason it stopped.
    #[cfg(test)]
    pub fn preparation_blocked(&self, config: &str) -> Option<String> {
        self.sequences.get(config).and_then(|sequence| {
            (!sequence.is_active())
                .then(|| sequence.blocked_by().map(str::to_owned))
                .flatten()
        })
    }

    /// Whether a launch request for this configuration is still awaiting its session identity.
    pub fn is_pending(&self, config: &str) -> bool {
        self.pending.iter().any(|pending| pending.config == config)
    }

    /// Whether this configuration is preparing: running its own build or a pre-launch step.
    ///
    /// A configuration prepares once at a time, and the Build control is disabled while its own
    /// sequence runs, so two clicks cannot build the same code in parallel.
    pub fn is_preparing(&self, config: &str) -> bool {
        self.sequences
            .get(config)
            .is_some_and(RunSequence::is_active)
    }

    /// The preparation now running for a configuration, for status text.
    pub fn preparation(&self, config: &str) -> Option<&RunSequence> {
        self.sequences.get(config)
    }

    /// Settle the original provider step and fill only that launch's frozen final debug request.
    pub fn provider_prepared(
        &mut self,
        config: &str,
        index: usize,
        request: u64,
        result: Result<&str, &str>,
    ) -> bool {
        // Provider preparations finish without HostRunSnapshot. Even a stopped/late receipt retires
        // only its own wait; it must never leave a ghost start or clear a newer request.
        self.pending
            .retain(|pending| pending.config != config || pending.request_id != request);
        if let Some(view) = self.provider_preparations.get_mut(&request) {
            if view.snapshot.state.is_active() {
                view.snapshot.state = if result.is_ok() {
                    plugin_runtime::ExecutionState::Exited
                } else {
                    plugin_runtime::ExecutionState::Failed
                };
            }
        }
        let Some(sequence) = self.sequences.get_mut(config) else {
            return false;
        };
        let own_build = sequence
            .current_step()
            .is_some_and(|step| step.config == config)
            && sequence.planned_preparation(index) == self.debug_preparation_bindings.get(config);
        if !sequence.provider_prepared(index, request, result.clone()) {
            return false;
        }
        if own_build
            && let Ok(program) = result
            && let Some(arguments) = self.debug_preparations.get_mut(config)
        {
            arguments["program"] = serde_json::json!(program);
        }
        true
    }

    /// The step now running for a configuration, as `阶段 名称`, for status text.
    pub fn preparing_step(&self, config: &str) -> Option<String> {
        self.sequences.get(config).and_then(|sequence| {
            sequence
                .current_step()
                .map(|step| format!("{} {}", step.kind.label(), step.name))
        })
    }

    /// Why preparing this configuration would not work, or `None` when it would.
    ///
    /// The Build control asks this before it is enabled, so a disabled button always has a reason a
    /// user can read rather than a control that silently does nothing.
    pub fn preparation_error(&self, config: &str) -> Option<String> {
        if self.configs.find(config).is_none() {
            return Some(t!("run.configuration_missing").to_string().into());
        }
        self.prepare_build(config, "", MAX_PREPARED_STEPS).err()
    }

    /// Begin preparing a configuration, using the launch identity the caller already reserved.
    ///
    /// The plan is stored with the sequence, so nothing about a running preparation changes when the
    /// configuration is edited during it.
    pub fn begin_sequence(&mut self, config: &str, plan: RunPlan, request_id: u64) {
        self.sequences
            .insert(config.to_owned(), RunSequence::new(config, &plan));
        // The plan owns its waiting steps; only staged effect requests belong in `pending`.
        // Keeping the reserved plan identity would strand a ghost start after a real build exits.
        self.pending
            .retain(|pending| pending.config != config || pending.request_id != request_id);
    }

    /// Freeze the debug target while ordinary build/prelaunch sessions run, without an ordinary
    /// execution of the final program. The same saved launch plan supplies argv, env and cwd.
    pub fn begin_debug_preparation(
        &mut self,
        config: &str,
        mut plan: RunPlan,
        workspace: &str,
    ) -> Result<(), String> {
        let target = plan
            .steps
            .pop()
            .filter(|step| step.kind == StepKind::Program)
            .ok_or_else(|| "Missing final debug program".to_owned())?;
        let mut arguments =
            serde_json::to_value(target.request).map_err(|error| error.to_string())?;
        let definition = self
            .configuration(config)
            .ok_or_else(|| "Missing debug configuration".to_owned())?;
        let breakpoints = definition
            .breakpoints
            .entries()
            .iter()
            .map(|point| {
                let path = std::path::Path::new(&point.source);
                let path = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    std::path::Path::new(workspace).join(path)
                };
                serde_json::json!({"source":path.display().to_string(),"line":point.line})
            })
            .collect::<Vec<_>>();
        arguments["breakpoints"] = serde_json::json!(breakpoints);
        if let Some(binding) = target.preparation {
            self.debug_preparation_bindings
                .insert(config.into(), binding);
        }
        self.debug_preparations.insert(config.into(), arguments);
        let request = self.begin(config);
        self.begin_sequence(config, plan, request);
        Ok(())
    }
    /// Only the completed sequence may consume this once; a stopped preparation discards it.
    pub fn take_prepared_debug(&mut self, config: &str) -> Option<serde_json::Value> {
        if self.sequences.get(config)?.next_action(|_| true, |_| true) != SequenceAction::Done {
            return None;
        }
        let arguments = self.debug_preparations.remove(config)?;
        self.debug_preparation_bindings.remove(config);
        self.sequences.remove(config);
        self.pending.retain(|request| request.config != config);
        Some(arguments)
    }
    /// A failed build is reported as a failed debug preparation, with no adapter or ordinary target.
    pub fn fail_prepared_debug(&mut self, config: &str, reason: &str) {
        self.debug_preparation_bindings.remove(config);
        if self.debug_preparations.remove(config).is_some() {
            self.note_debug_state(
                config,
                editor_core::DebugSessionState::Failed {
                    reason: reason.into(),
                },
            );
        }
    }
    /// Cancellation seals both ordinary preparation and its not-yet-created debug target.
    pub fn cancel_prepared_debug(&mut self, config: &str) {
        self.debug_preparation_bindings.remove(config);
        if self.debug_preparations.remove(config).is_some() {
            self.note_debug_state(config, editor_core::DebugSessionState::Exited);
        }
    }

    /// Replace one entry of a configuration's program step for this launch only.
    ///
    /// The configuration's own entries stay authoritative for everything else, so a launch-time
    /// override cannot silently drop a tool directory or a variable the user stored.
    pub fn override_program_environment(
        &mut self,
        config: &str,
        entry: &plugin_runtime::RunEnvEntry,
    ) {
        let Some(sequence) = self.sequences.get_mut(config) else {
            return;
        };
        let Some(index) = sequence.steps().len().checked_sub(1) else {
            return;
        };
        sequence.override_environment(index, entry);
    }

    /// Begin a build-only sequence for a configuration, using an identity the caller reserved.
    ///
    /// A build is its own session: it is visible, stoppable and locatable in the run controls exactly
    /// like a launch, but its sequence never contains a program step.
    pub fn begin_build(&mut self, config: &str, plan: &RunPlan, request_id: u64) {
        self.sequences.insert(
            config.to_owned(),
            RunSequence::build_only(config, &plan.steps),
        );
        // The first actual step receives its own request; the reservation is not an effect.
        self.pending
            .retain(|pending| pending.config != config || pending.request_id != request_id);
    }

    /// Join published sessions to the preparation steps that requested them.
    ///
    /// A step owns its session from the moment the runtime reports it, not from the moment its
    /// provider confirms the program: waiting for confirmation would leave the step unowned while a
    /// second click could request it again. The step's request identity is checked against the
    /// session's, so a session that belongs to a different launch is never adopted.
    pub fn adopt_step_sessions(&mut self, published: &[(u64, u64, Option<String>)]) {
        for (session, request_id, provider_session) in published {
            let Some((config, index)) = self.step_by_request(*request_id) else {
                continue;
            };
            let Some(sequence) = self.sequences.get_mut(&config) else {
                continue;
            };
            // Publications repeat for ended sessions. Adopt once while the original request waits;
            // replaying an old snapshot must never turn a succeeded/stopped step back into Running.
            if !sequence.steps().get(index).is_some_and(|step|matches!(step.state,sequence::StepState::Starting {request} if request==*request_id)) {continue;}
            sequence.started(index, *session, provider_session.clone());
            self.step_sessions
                .insert(*session, (config.to_owned(), index));
        }
    }

    /// The preparation step whose request identity matches, if any.
    fn step_by_request(&self, request_id: u64) -> Option<(String, usize)> {
        self.step_requests.get(&request_id).cloned()
    }

    /// Record the session the runtime published for one step of a configuration's preparation.
    pub fn sequence_started(
        &mut self,
        config: &str,
        index: usize,
        session: u64,
        provider_session: Option<String>,
    ) {
        if let Some(sequence) = self.sequences.get_mut(config) {
            sequence.started(index, session, provider_session);
        }
        self.step_sessions
            .insert(session, (config.to_owned(), index));
    }

    /// The preparation step a session belongs to, when it is preparation rather than a program.
    pub fn step_of(&self, session: u64) -> Option<(&str, usize)> {
        self.step_sessions
            .get(&session)
            .map(|(config, index)| (config.as_str(), *index))
    }

    /// Forget the sessions of a finished preparation, so a later launch cannot adopt them.
    pub fn forget_step_sessions(&mut self, config: &str) {
        let finished = self
            .sequences
            .get(config)
            .is_none_or(|sequence| !sequence.is_active());
        if finished {
            self.step_sessions.retain(|_, (owner, _)| owner != config);
            self.polls.retain(|poll| poll.config != config);
        }
    }

    /// Record a status query this editor is about to make for one preparation step.
    pub fn begin_poll(&mut self, config: &str, index: usize, session: u64) -> u64 {
        self.next_request += 1;
        self.polls.push(PendingPoll {
            config: config.to_owned(),
            index,
            session,
            request_id: self.next_request,
        });
        self.next_request
    }

    /// Accept one provider answer about a preparation step, feeding it into the sequence waiting.
    ///
    /// An answer belonging to a query this editor did not make is ignored, so one window never
    /// advances another window's preparation.
    pub fn reconcile_run_status(
        &mut self,
        published: &[(String, u64, crate::extensions::RunStatus)],
    ) -> Vec<(String, usize, StepOutcome)> {
        let mut applied = Vec::new();
        for (config, request_id, status) in published {
            let Some(position) = self
                .polls
                .iter()
                .position(|poll| poll.config == *config && poll.request_id == *request_id)
            else {
                continue;
            };
            let poll = self.polls.remove(position);
            // A program still running is not progress: the sequence keeps waiting for its end.
            let outcome = match status {
                crate::extensions::RunStatus::Running => continue,
                crate::extensions::RunStatus::Ended { code: Some(code) } => {
                    StepOutcome::Exited { code: *code }
                }
                crate::extensions::RunStatus::Ended { code: None } => StepOutcome::Unknown,
                crate::extensions::RunStatus::Terminated => StepOutcome::Terminated,
                crate::extensions::RunStatus::Unknown => StepOutcome::Unknown,
            };
            let advanced = self
                .sequences
                .get_mut(config)
                .is_some_and(|sequence| sequence.observe(poll.index, outcome.clone()));
            if advanced {
                applied.push((config.clone(), poll.index, outcome));
            }
        }
        applied
    }

    /// Record that one step of a configuration's preparation could not start at all.
    pub fn sequence_start_failed(&mut self, config: &str, index: usize, reason: &str) {
        let request = self
            .sequences
            .get(config)
            .and_then(RunSequence::pending_provider_request);
        if let Some(sequence) = self.sequences.get_mut(config) {
            sequence.start_failed(index, reason);
        }
        if let Some(request) = request {
            self.pending
                .retain(|pending| pending.config != config || pending.request_id != request);
            if let Some(view) = self.provider_preparations.get_mut(&request) {
                view.snapshot.state = plugin_runtime::ExecutionState::Failed;
                view.snapshot.output = reason.into();
            }
        }
    }

    /// Feed one observed program end into the sequence waiting on it.
    ///
    /// Returns whether the observation advanced or blocked a sequence, so a late answer for a step
    /// the sequence already left is not reported as progress.
    pub fn observe_step(&mut self, config: &str, index: usize, outcome: StepOutcome) -> bool {
        self.sequences
            .get_mut(config)
            .is_some_and(|sequence| sequence.observe(index, outcome))
    }

    /// Ask every preparing configuration to stop, blocking its launch.
    pub fn stop_preparations(&mut self) -> Vec<(String, Option<u64>)> {
        self.reruns.clear();
        let mut stopped = Vec::new();
        for (config, sequence) in &mut self.sequences {
            if !sequence.is_active() {
                continue;
            }
            sequence.request_stop();
            stopped.push((config.clone(), sequence.current_session()));
        }
        stopped
    }

    /// Record that a session a preparation was stopping has ended.
    ///
    /// The sequence was already blocked by the stop request, so this only closes the step that owned
    /// the session: no later step can start, and the preparation stops reporting work in progress.
    pub fn note_preparation_stopped(&mut self, session: u64) -> Option<String> {
        let (config, _) = self.step_sessions.get(&session)?.clone();
        let sequence = self.sequences.get_mut(&config)?;
        sequence.stopped(session);
        self.poll_config(&config);
        Some(config)
    }

    /// Only observed ended/revoked sessions finish a stopped preparation; a stop acknowledgement cannot.
    pub fn finish_stopped_preparations(&mut self) -> Vec<String> {
        let ended = self
            .step_sessions
            .iter()
            .filter_map(|(id, (config, _))| {
                (self
                    .sequences
                    .get(config)
                    .is_some_and(RunSequence::is_stopping)
                    && self
                        .sessions
                        .get(id)
                        .is_some_and(|session| !session.is_active()))
                .then_some(*id)
            })
            .collect::<Vec<_>>();
        ended
            .into_iter()
            .filter_map(|id| self.note_preparation_stopped(id))
            .collect()
    }

    /// Drop every status query belonging to one configuration's preparation.
    fn poll_config(&mut self, config: &str) {
        self.polls.retain(|poll| poll.config != config);
    }

    /// Record which provider a debug launch would use, or why there is none.
    ///
    /// The run controls keep this answer rather than asking the host at click time, so a refused
    /// debug click can explain itself without a round trip and cannot be mistaken for a plain run.
    pub fn note_debug_availability(&mut self, availability: Result<String, String>) {
        self.debug_availability = Some(availability);
    }

    /// Replace the stored configurations, for a check that needs a state the save path would refuse.
    ///
    /// Editing a configuration normally validates it first, which is right; this exists so a check
    /// can still ask what the controls do with a configuration that is already stored and unusable.
    #[cfg(test)]
    pub fn replace_configs(&mut self, configs: editor_core::RunConfigSet) {
        self.configs = configs;
    }

    /// Whether a debug launch could start right now, and the reason it cannot.
    pub fn debug_availability(&self) -> Result<&str, String> {
        match &self.debug_availability {
            Some(Ok(provider)) => Ok(provider.as_str()),
            Some(Err(reason)) => Err(reason.clone()),
            // Before the host has answered, nothing is offered: an entry point may not assume a
            // capability it has not been told about, and it may not fall back to running plainly.
            None => Err(t!("run.debug_unconfirmed").to_string()),
        }
    }

    /// Record what one configuration's debug session now is, as its provider reported it.
    ///
    /// State is per session, so a pause in one configuration never looks like a pause in another, and
    /// a state that is no longer a pause ends that session's inspection data.
    pub fn note_debug_state(&mut self, config: &str, state: editor_core::DebugSessionState) {
        let mut session = self
            .debug_sessions
            .session(config)
            .cloned()
            .unwrap_or_default();
        session.note_state(state.clone());
        self.debug_sessions.insert(config, session);
    }

    /// Note that the next pause is one the user asked for by moving the target themselves.
    ///
    /// Resume and the three step directions are the user driving the target, so the editor does not
    /// chase them around the file: the caret stays where they put it. A pause nobody asked for — a
    /// breakpoint hit — is where following the target is what the user wants.
    pub fn note_debug_moved_by_user(&mut self) {
        self.debug_position_followed = false;
    }

    /// Whether the editor should move to the location of the pause that has just arrived.
    ///
    /// Reading this clears it, so a later state report about an already located pause cannot move the
    /// caret a second time.
    pub fn take_debug_position_to_follow(&mut self) -> bool {
        std::mem::take(&mut self.debug_position_followed)
    }

    /// Note that a session has begun, which is the first pause worth following.
    pub fn note_debug_session_begun(&mut self, config: &str) {
        self.debug_position_followed = true;
        // Nothing has been reported about this session's breakpoints yet, and the previous session's
        // answer described a different target.
        self.debug_breakpoints_verified.remove(config);
    }

    /// Record the positions the provider could bind, replacing whatever the last answer said.
    ///
    /// Replacing rather than merging is what makes a removed breakpoint stop being reported as bound:
    /// the provider's answer describes the set it was just asked about, not a history of it.
    pub fn note_debug_breakpoints(
        &mut self,
        config: &str,
        bound: impl IntoIterator<Item = (String, u32, bool)>,
    ) {
        self.debug_breakpoints_verified
            .insert(config.into(), bound.into_iter().collect());
    }

    /// A receipt names its original target even if the user has selected another paused session.
    /// An abandoned request cannot replace evidence for a later incarnation of the same config.
    pub fn apply_debug_breakpoint_answer(
        &mut self,
        request: u64,
        bound: impl IntoIterator<Item = (String, u32, bool)>,
    ) -> Result<(), editor_core::InspectionError> {
        let index = self
            .debug_requests
            .iter()
            .position(|pending| pending.id == request && pending.method == DebugMethod::Breakpoints)
            .ok_or(editor_core::InspectionError::NoSession)?;
        let pending = self.debug_requests.remove(index);
        let config = pending
            .config
            .ok_or(editor_core::InspectionError::NoSession)?;
        if self.debug_session_of(&config).is_none() {
            return Err(editor_core::InspectionError::NoSession);
        }
        self.note_debug_breakpoints(&config, bound);
        Ok(())
    }

    /// Whether the provider could bind this position: `None` when no answer has mentioned it.
    ///
    /// Distinct from `Some(false)` on purpose: a breakpoint that was asked about and refused is a
    /// problem to fix, while one that no answer has covered is simply not described yet.
    pub fn debug_breakpoint_verified(&self, source: &str, line: u32) -> Option<bool> {
        let config = self
            .debug_session()
            .map(|(config, _)| config)
            .or_else(|| self.selected().map(|config| config.id.as_str()))?;
        self.debug_breakpoints_verified
            .get(config)?
            .iter()
            .find(|(known, known_line, _)| known_line == &line && known == source)
            .map(|(_, _, verified)| *verified)
    }

    /// The selected configuration's breakpoints in the order they were set.
    ///
    /// The panel needs the positions themselves to say which of them the provider bound, and the
    /// editable field cannot carry that: it is parsed back on every keystroke, so an annotation in it
    /// would have to survive `parse_breakpoints`. This is the same list the field renders, read
    /// separately for the part that is evidence rather than input.
    pub fn debug_breakpoint_positions(&self) -> Vec<(String, u32)> {
        self.debug_session()
            .and_then(|(config, _)| self.configuration(config))
            .or_else(|| self.selected())
            .map(|config| {
                config
                    .breakpoints
                    .entries()
                    .iter()
                    .map(|entry| (entry.source.clone(), entry.line))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// End one configuration's debug session, handing the panel to another if there is one.
    pub fn end_debug_session(&mut self, config: &str) {
        // Restarting a configuration must not resurrect replies from the previous incarnation.
        self.debug_requests
            .retain(|pending| pending.config.as_deref() != Some(config));
        self.debug_sessions.remove(config);
        self.debug_breakpoints_verified.remove(config);
    }

    /// The selected debug session, if one is selected and still running.
    pub fn debug_session(&self) -> Option<(&str, &editor_core::DebugSession)> {
        self.debug_sessions.current()
    }

    /// The debug session one configuration is running, if any.
    pub fn debug_session_of(&self, config: &str) -> Option<&editor_core::DebugSession> {
        self.debug_sessions.session(config)
    }

    /// Select the debug session the panel acts on, refusing a configuration with no session.
    pub fn select_debug_session(&mut self, config: &str) -> bool {
        self.debug_sessions.select(config)
    }

    /// Whether every debug session this editor has, for a panel that lists them.
    pub fn debug_sessions(&self) -> impl Iterator<Item = (&str, &editor_core::DebugSession)> {
        self.debug_sessions.entries()
    }

    /// The state of the selected debug session, or disconnected when there is none.
    ///
    /// A panel asks this rather than holding its own copy, so there is one answer about what the
    /// selected session is.
    pub fn debug_state(&self) -> editor_core::DebugSessionState {
        self.debug_sessions
            .current()
            .map(|(_, session)| session.state().clone())
            .unwrap_or_default()
    }

    /// Begin a pause for the selected session and hand out its scope.
    ///
    /// The scope is what an inspection answer is later checked against, so a late one about a pause
    /// that has ended is refused instead of replacing the view.
    pub fn begin_debug_pause(&mut self) -> Option<editor_core::PauseScope> {
        let (config, _) = self.debug_sessions.current()?;
        let config = config.to_owned();
        self.debug_sessions
            .session_mut(&config)
            .map(|session| session.begin_pause())
    }

    /// Apply a provider's frame report to the selected session's current pause.
    pub fn apply_debug_frames(
        &mut self,
        scope: editor_core::PauseScope,
        frames: Vec<editor_core::StackFrame>,
    ) -> Result<(), editor_core::InspectionError> {
        let (config, _) = self
            .debug_sessions
            .current()
            .ok_or(editor_core::InspectionError::NoSession)?;
        let config = config.to_owned();
        self.debug_sessions
            .session_mut(&config)
            .expect("the session was just selected")
            .set_frames(scope, frames)
    }

    /// Apply a provider's variable report for one frame.
    pub fn apply_debug_variables(
        &mut self,
        scope: editor_core::PauseScope,
        frame: u32,
        variables: Vec<editor_core::DebugVariable>,
    ) -> Result<(), editor_core::InspectionError> {
        let (config, _) = self
            .debug_sessions
            .current()
            .ok_or(editor_core::InspectionError::NoSession)?;
        let config = config.to_owned();
        self.debug_sessions
            .session_mut(&config)
            .expect("the session was just selected")
            .set_variables(scope, frame, variables)
    }

    /// Select a frame of the selected session's pause, which is what locates the source.
    pub fn select_debug_frame(&mut self, frame: u32) -> Result<(), editor_core::InspectionError> {
        let (config, _) = self
            .debug_sessions
            .current()
            .ok_or(editor_core::InspectionError::NoSession)?;
        let config = config.to_owned();
        self.debug_sessions
            .session_mut(&config)
            .expect("the session was just selected")
            .select_frame(frame)
    }

    /// Where the selected session's selected frame is, which is the location to reveal.
    pub fn debug_location(&self) -> Option<(&str, u32)> {
        self.debug_sessions
            .current()
            .and_then(|(_, session)| session.pause().selected_location())
    }

    /// The frames of the selected session's pause, empty when nothing has been reported.
    pub fn debug_frames(&self) -> &[editor_core::StackFrame] {
        self.debug_sessions
            .current()
            .map(|(_, session)| session.pause().frames())
            .unwrap_or_default()
    }

    /// The variables of one frame of the selected session's pause.
    pub fn debug_variables(&self, frame: u32) -> &[editor_core::DebugVariable] {
        self.debug_sessions
            .current()
            .map(|(_, session)| session.pause().variables_of(frame))
            .unwrap_or_default()
    }

    /// The selected session's current pause, if it has one.
    pub fn debug_pause_scope(&self) -> Option<editor_core::PauseScope> {
        self.debug_sessions
            .current()
            .and_then(|(_, session)| session.pause().scope())
    }
    /// The sessions one plugin is currently serving, which a lifecycle change would take away.
    ///
    /// Reported for confirmation only: this asks nothing, stops nothing and changes no selection. A
    /// session is attributed to the plugin that answered it, so a plugin that has since been replaced
    /// is still credited with what it is running.
    pub fn plugin_session_impact(
        &self,
        plugin: &str,
        _debug_provider: Option<&str>,
    ) -> PluginSessionImpact {
        let names = |config: &str| {
            self.configs
                .find(config)
                .map(|configuration| configuration.name.clone())
                .unwrap_or_else(|| config.to_owned())
        };
        let mut running = self
            .sessions
            .iter()
            .filter(|(_, session)| session.plugin == plugin && session.is_active())
            .map(|(_, session)| names(&session.config))
            .collect::<Vec<_>>();
        // A preparation owns its original provider even when the saved configuration was edited.
        running.extend(
            self.provider_preparations
                .values()
                .filter(|view| view.snapshot.provider == plugin && view.snapshot.state.is_active())
                .map(|view| names(&view.config)),
        );
        running.sort();
        running.dedup();
        // A default switch affects future launches; impact belongs to each target's original owner.
        let debugging = self
            .debug_sessions
            .entries()
            .filter(|(config, session)| {
                self.debug_owners
                    .get(*config)
                    .is_some_and(|owner| owner == plugin)
                    && matches!(
                        session.state(),
                        editor_core::DebugSessionState::Starting
                            | editor_core::DebugSessionState::Running
                            | editor_core::DebugSessionState::Paused { .. }
                    )
            })
            .map(|(config, _)| names(config))
            .collect();
        PluginSessionImpact { running, debugging }
    }

    /// Record that a debug request was sent, and hand back its identity.
    ///
    /// The scope is captured here rather than by the caller, so a request can only ever be joined to
    /// the pause that was current when it was sent.
    pub fn begin_debug_request(&mut self, method: DebugMethod, frame: Option<u32>) -> Option<u64> {
        let config = if method == DebugMethod::Start {
            self.configs.selected.clone()?
        } else if method == DebugMethod::Breakpoints && self.debug_sessions.current().is_none() {
            // A breakpoint definition can be recorded before any provider session or pause exists.
            self.configs.selected.clone()?
        } else {
            self.debug_sessions.current()?.0.to_owned()
        };
        // Starting is about no pause: it is what begins the session whose pauses are inspected later.
        // Setting breakpoints is likewise about the session, not about a moment in it — a user may set
        // one while the target runs, and it has to reach the debugger before the next stop. Everything
        // else names the pause it describes, so an answer can be refused once it is over.
        let scope = match self.debug_pause_scope() {
            Some(scope) => scope,
            None if matches!(
                method,
                DebugMethod::Start | DebugMethod::Breakpoints | DebugMethod::Control(_)
            ) =>
            {
                editor_core::PauseScope::starting()
            }
            None => return None,
        };
        // One request per method per pause: asking twice would leave two answers racing to describe
        // the same pause, and the later one would win for no reason the user could see.
        if self.debug_requests.iter().any(|request| {
            request.config.as_deref() == Some(config.as_str())
                && request.scope == scope
                && request.method == method
                && request.frame == frame
        }) {
            return None;
        }
        self.next_debug_request = self.next_debug_request.checked_add(1)?;
        let id = self.next_debug_request;
        // Starting a session is what creates it, so the session exists from the moment it is asked
        // for: the answer has somewhere to land, and a configuration that cannot be named has no
        // session to begin.
        if method == DebugMethod::Start {
            self.debug_sessions
                .insert(&config, editor_core::DebugSession::default());
        }
        self.debug_requests.push(PendingDebugRequest {
            id,
            method,
            scope,
            frame,
            config: Some(config),
        });
        Some(id)
    }

    /// Apply an answer to the request it answers, refusing one whose pause is over.
    ///
    /// This is the only way an answer reaches the view, so a late result cannot replace a newer one
    /// and a result for a request this editor never sent cannot arrive at all.
    pub fn apply_debug_answer(
        &mut self,
        request: u64,
        frames: Option<Vec<editor_core::StackFrame>>,
        variables: Option<Vec<editor_core::DebugVariable>>,
    ) -> Result<(), editor_core::InspectionError> {
        let Some(index) = self
            .debug_requests
            .iter()
            .position(|pending| pending.id == request)
        else {
            // An answer to nothing is an answer this editor did not ask for.
            return Err(editor_core::InspectionError::NoSession);
        };
        let pending = self.debug_requests.remove(index);
        if pending.config.as_deref() != self.debug_sessions.selected() {
            return Err(editor_core::InspectionError::WrongSession);
        }
        // The answer is applied to the pause it was asked about, which is what refuses a late one.
        match pending.method {
            // A session that has started is recorded with the provider's own identity, so every later
            // call names the session the provider knows rather than one the host made up.
            DebugMethod::Start => Err(editor_core::InspectionError::NoSession),
            DebugMethod::Breakpoints => Err(editor_core::InspectionError::NoSession),
            DebugMethod::Frames => self.apply_debug_frames(
                pending.scope,
                frames.ok_or(editor_core::InspectionError::NoSession)?,
            ),
            DebugMethod::Variables => self.apply_debug_variables(
                pending.scope,
                pending
                    .frame
                    .ok_or(editor_core::InspectionError::NoSession)?,
                variables.ok_or(editor_core::InspectionError::NoSession)?,
            ),
            // A step's answer is its new state, which `apply_debug_step` takes; a view arriving for a
            // step is not an answer to what was asked.
            DebugMethod::Step(_) | DebugMethod::Control(_) => {
                Err(editor_core::InspectionError::NoSession)
            }
        }
    }

    /// Turn a provider's own failure into the reason a request did not produce a view.
    ///
    /// The provider's message is kept, because it is the only account of what went wrong; the
    /// editor's own vocabulary is used only for the cases its message makes unambiguous.
    fn inspection_failure(message: String) -> editor_core::InspectionError {
        if message.contains("没有正在检查的调试会话") {
            return editor_core::InspectionError::NoSession;
        }
        if message.contains("目标未暂停") {
            return editor_core::InspectionError::NotPaused;
        }
        if message.contains("该暂停已结束") {
            return editor_core::InspectionError::StalePause;
        }
        editor_core::InspectionError::Provider(message)
    }

    /// Record the provider's own identity for one configuration's debug session.
    pub fn note_debug_provider_session(&mut self, config: &str, session: &str) {
        if let Some(session_state) = self.debug_sessions.session_mut(config) {
            session_state.note_provider_session(session);
        }
    }

    /// An early creation identity belongs to the configuration captured by its start request.
    pub fn note_debug_connecting(
        &mut self,
        request: u64,
        session: &str,
    ) -> Result<(), editor_core::InspectionError> {
        let config = self
            .debug_requests
            .iter()
            .find(|pending| pending.id == request && pending.method == DebugMethod::Start)
            .and_then(|pending| pending.config.clone())
            .ok_or(editor_core::InspectionError::NoSession)?;
        self.note_debug_provider_session(&config, session);
        self.note_debug_state(&config, editor_core::DebugSessionState::Starting);
        Ok(())
    }
    /// A state reply completes its own action even after selection changes. Epoch checks keep a
    /// late resume/start reply from overwriting a newer real stop at the same source line.
    pub fn apply_debug_state_reply(
        &mut self,
        request: u64,
        report: &plugin_runtime::DebugSession,
    ) -> Result<(), editor_core::InspectionError> {
        let index = self
            .debug_requests
            .iter()
            .position(|pending| pending.id == request)
            .ok_or(editor_core::InspectionError::NoSession)?;
        let pending = self.debug_requests.remove(index);
        let config = pending
            .config
            .ok_or(editor_core::InspectionError::NoSession)?;
        if !matches!(
            pending.method,
            DebugMethod::Start | DebugMethod::Step(_) | DebugMethod::Control(_)
        ) {
            return Err(editor_core::InspectionError::NoSession);
        }
        self.debug_action_in_flight.remove(&config);
        if pending.method == DebugMethod::Start {
            self.note_debug_provider_session(&config, &report.session);
        }
        if self
            .debug_session_of(&config)
            .and_then(|session| session.provider_session())
            != Some(report.session.as_str())
        {
            return Err(editor_core::InspectionError::WrongSession);
        }
        self.observe_debug_report(report);
        Ok(())
    }
    /// Consume an actual provider observation, beginning inspection only at a new pause epoch.
    /// Return whether this is a newly stopped selected session so the UI can navigate once.
    pub fn observe_debug_report(&mut self, report: &plugin_runtime::DebugSession) -> bool {
        let Some(config) = self
            .debug_sessions
            .entries()
            .find(|(_, session)| session.provider_session() == Some(report.session.as_str()))
            .map(|(config, _)| config.to_owned())
        else {
            return false;
        };
        let epoch = report.pause.unwrap_or(0);
        if let Some(provider) = &report.provider {
            self.debug_owners.insert(config.clone(), provider.clone());
        }
        if self
            .debug_epochs
            .get(&config)
            .is_some_and(|previous| *previous > epoch)
        {
            return false;
        }
        // Control receipts omit location fields. Preserve a richer observation from the same
        // epoch rather than erasing the source after its asynchronous location reply arrives.
        let previous = self
            .debug_sessions
            .session(&config)
            .map(|session| session.state().clone());
        let same_pause = self.debug_epochs.get(&config) == Some(&epoch);
        let prior_location = if same_pause {
            match previous {
                Some(editor_core::DebugSessionState::Paused {
                    reason,
                    source,
                    line,
                }) => Some((reason, source, line)),
                _ => None,
            }
        } else {
            None
        };
        let state = match report.state {
            plugin_runtime::DebugState::Starting => editor_core::DebugSessionState::Starting,
            plugin_runtime::DebugState::Running => editor_core::DebugSessionState::Running,
            plugin_runtime::DebugState::Exited => editor_core::DebugSessionState::Exited,
            plugin_runtime::DebugState::Failed => editor_core::DebugSessionState::Failed {
                reason: report
                    .reason
                    .clone()
                    .unwrap_or_else(|| "Debug session failed".into()),
            },
            plugin_runtime::DebugState::Paused => editor_core::DebugSessionState::Paused {
                reason: report
                    .reason
                    .clone()
                    .or_else(|| prior_location.as_ref().and_then(|prior| prior.0.clone())),
                source: report
                    .source
                    .clone()
                    .or_else(|| prior_location.as_ref().map(|prior| prior.1.clone()))
                    .unwrap_or_default(),
                line: report
                    .line
                    .or_else(|| prior_location.as_ref().map(|prior| prior.2))
                    .unwrap_or(0),
            },
        };
        let session = self.debug_sessions.session_mut(&config).unwrap();
        let new_pause = report.state == plugin_runtime::DebugState::Paused
            && (self.debug_epochs.get(&config) != Some(&epoch)
                || !matches!(
                    session.state(),
                    editor_core::DebugSessionState::Paused { .. }
                ));
        session.note_state(state);
        if new_pause {
            session.begin_pause();
        }
        self.debug_epochs.insert(config.clone(), epoch);
        let location_ready = report.state == plugin_runtime::DebugState::Paused
            && report
                .source
                .as_ref()
                .is_some_and(|source| !source.is_empty())
            && report.line.is_some_and(|line| line > 0)
            && self.debug_position_epochs.get(&config) != Some(&epoch);
        if location_ready {
            self.debug_position_epochs.insert(config.clone(), epoch);
        }
        location_ready
            && self.debug_sessions.selected() == Some(config.as_str())
            && !self.debug_sessions.another_is_paused(&config)
    }

    /// The selected session's provider identity, which every call names.
    pub fn debug_provider_session(&self) -> Option<String> {
        self.debug_sessions
            .current()
            .and_then(|(_, session)| session.provider_session())
            .map(str::to_owned)
    }

    /// Public pause epoch associated with the selected inspection; frame IDs alone are not stable.
    pub fn debug_pause_epoch(&self) -> Option<u64> {
        self.debug_sessions
            .selected()
            .and_then(|config| self.debug_epochs.get(config))
            .copied()
    }

    /// Record the launch-time owner before any lifecycle prompt; later defaults cannot reattribute it.
    pub fn note_debug_provider_owner(&mut self, config: &str, provider: &str) {
        self.debug_owners.insert(config.into(), provider.into());
    }

    /// Note that one debug control action was sent, and is waiting for its answer.
    pub fn note_debug_action(&mut self) {
        if let Some(config) = self.debug_sessions.selected() {
            self.debug_action_in_flight.insert(config.into());
        }
    }

    /// Note that the outcome of a debug control action is known, whichever way it went.
    pub fn note_debug_action_finished(&mut self) {
        if let Some(config) = self.debug_sessions.selected() {
            self.debug_action_in_flight.remove(config);
        }
    }

    /// Whether a request this editor sent begins a session rather than asking about one.
    pub fn debug_request_is_start(&self, request: u64) -> bool {
        self.debug_requests
            .iter()
            .any(|pending| pending.id == request && pending.method == DebugMethod::Start)
    }

    /// Whether a debug control action is still waiting for its answer.
    pub fn debug_action_pending(&self) -> bool {
        self.debug_sessions
            .selected()
            .is_some_and(|config| self.debug_action_in_flight.contains(config))
    }

    /// Apply a start's answer, which establishes the session the provider now owns.
    ///
    /// The identity and the state are both the provider's: the host records what it was told rather
    /// than claiming a session it did not receive.
    pub fn apply_debug_start(
        &mut self,
        request: u64,
        session: &str,
        state: editor_core::DebugSessionState,
    ) -> Result<(), editor_core::InspectionError> {
        let Some(index) = self
            .debug_requests
            .iter()
            .position(|pending| pending.id == request)
        else {
            return Err(editor_core::InspectionError::NoSession);
        };
        let pending = self.debug_requests.remove(index);
        if pending.method != DebugMethod::Start {
            // An answer to one question is not an answer to another.
            return Err(editor_core::InspectionError::NoSession);
        }
        // The configuration was recorded when the request was sent, so the answer lands on the
        // session that asked rather than on whichever one happens to be selected now.
        let Some(config) = pending.config else {
            return Err(editor_core::InspectionError::NoSession);
        };
        let config = config.as_str();
        self.debug_sessions
            .session_mut(config)
            .ok_or(editor_core::InspectionError::NoSession)?
            .note_provider_session(session);
        self.note_debug_state(config, state);
        Ok(())
    }

    /// The arguments a debug provider is asked to start one configuration with.
    ///
    /// Assembled from the configuration itself — the program, its arguments, its working directory,
    /// its environment and the breakpoints it has set — so a debug launch says what a run would say
    /// plus where to stop. The breakpoints are grouped by source, which is how the contract asks for
    /// them, and a configuration with none simply omits the field.
    pub fn debug_launch_request(
        &self,
        id: &str,
        workspace_root: &str,
    ) -> Option<serde_json::Value> {
        let configs = self.configurations_for_launch().ok()?;
        let configuration = configs.find(id)?;
        let plan = self
            .prepare_launch_from(&configs, id, workspace_root, MAX_PREPARED_STEPS)
            .ok()?;
        let program = plan.steps.last()?;
        let mut arguments = serde_json::json!({
            "program": program.request.program,
            "args": program.request.args,
        });
        let object = arguments.as_object_mut()?;
        if let Some(cwd) = &program.request.cwd {
            object.insert("cwd".into(), serde_json::json!(cwd));
        }
        if let Some(name) = &program.request.name {
            object.insert("name".into(), serde_json::json!(name));
        }
        if !program.request.env.is_empty() {
            object.insert("env".into(), serde_json::json!(program.request.env));
        }
        if !configuration.breakpoints.is_empty() {
            object.insert(
                "breakpoints".into(),
                serde_json::json!(
                    configuration
                        .breakpoints
                        .entries()
                        .iter()
                        .map(|breakpoint| serde_json::json!({
                            "source": breakpoint.source,
                            "line": breakpoint.line,
                        }))
                        .collect::<Vec<_>>()
                ),
            );
        }
        Some(arguments)
    }

    /// Begin the debug session for one configuration, so its identity can be recorded when it answers.
    pub fn begin_debug_session(&mut self, config: &str) {
        // A restarted target owns a new epoch sequence; old numeric epochs cannot shadow it.
        self.debug_epochs.remove(config);
        self.debug_position_epochs.remove(config);
        self.debug_owners.remove(config);
        self.debug_sessions
            .insert(config, editor_core::DebugSession::default());
    }

    /// Start receipts use the frozen preparation owner, even if the user selected another config.
    pub fn begin_debug_start_request(&mut self, config: &str) -> Option<u64> {
        if self.debug_requests.iter().any(|pending| {
            pending.config.as_deref() == Some(config) && pending.method == DebugMethod::Start
        }) {
            return None;
        }
        self.next_debug_request = self.next_debug_request.checked_add(1)?;
        let request = self.next_debug_request;
        self.debug_requests.push(PendingDebugRequest {
            id: request,
            config: Some(config.into()),
            method: DebugMethod::Start,
            scope: editor_core::PauseScope::starting(),
            frame: None,
        });
        Some(request)
    }

    /// Actual failure clears this action and marks its own start, without failing a different selection.
    pub fn fail_debug_reply(
        &mut self,
        request: u64,
        reason: String,
    ) -> Result<(), editor_core::InspectionError> {
        let index = self
            .debug_requests
            .iter()
            .position(|pending| pending.id == request)
            .ok_or(editor_core::InspectionError::NoSession)?;
        let pending = self.debug_requests.remove(index);
        let config = pending
            .config
            .ok_or(editor_core::InspectionError::NoSession)?;
        self.debug_action_in_flight.remove(&config);
        if pending.method == DebugMethod::Start {
            self.note_debug_state(&config, editor_core::DebugSessionState::Failed { reason });
        }
        Ok(())
    }

    /// Apply a step's answer, which is the session's new state rather than a view of a pause.
    ///
    /// A state that is paused begins a new pause, so the old frames and variables stop describing a
    /// moment the target has left; anything else simply ends the old one. The state is never
    /// inferred: it is what the provider reported.
    pub fn apply_debug_step(
        &mut self,
        request: u64,
        state: editor_core::DebugSessionState,
    ) -> Result<(), editor_core::InspectionError> {
        let Some(index) = self
            .debug_requests
            .iter()
            .position(|pending| pending.id == request)
        else {
            return Err(editor_core::InspectionError::NoSession);
        };
        let pending = self.debug_requests.remove(index);
        let DebugMethod::Step(_) = pending.method else {
            // An answer to one question is not an answer to another.
            return Err(editor_core::InspectionError::NoSession);
        };
        let config = pending
            .config
            .ok_or(editor_core::InspectionError::NoSession)?;
        // The step is only applied to the pause it was asked about: a step whose answer arrived after
        // another pause began describes a moment that is already over.
        let session = self
            .debug_sessions
            .session_mut(&config)
            .ok_or(editor_core::InspectionError::NoSession)?;
        // A control response changes its owning session, even when another session is selected.
        // Selection controls presentation only; the captured pause still guards stale replies.
        session.pause().accepts(pending.scope)?;
        let paused = matches!(state, editor_core::DebugSessionState::Paused { .. });
        session.note_state(state);
        if paused {
            session.begin_pause();
        }
        Ok(())
    }

    /// Drop a request whose answer arrived malformed, so it is not awaited forever.
    pub fn abandon_debug_request(&mut self, request: u64) {
        self.debug_requests.retain(|pending| pending.id != request);
    }

    /// Record that a request failed, releasing it so it is not awaited forever.
    ///
    /// A failure is not an empty answer: a provider that could not report frames has said nothing
    /// about the target, and reporting that as "no frames" would describe a stack the host invented.
    pub fn fail_debug_request(&mut self, request: u64) -> Result<(), editor_core::InspectionError> {
        let Some(index) = self
            .debug_requests
            .iter()
            .position(|pending| pending.id == request)
        else {
            return Err(editor_core::InspectionError::NoSession);
        };
        let pending = self.debug_requests.remove(index);
        // The pause is still checked, so a failure about a pause that has ended is not reported
        // against the one the user is looking at.
        if pending.config.as_deref() != self.debug_sessions.selected() {
            return Err(editor_core::InspectionError::WrongSession);
        }
        self.debug_sessions
            .current()
            .ok_or(editor_core::InspectionError::NoSession)
            .and_then(|(_, session)| session.pause().accepts(pending.scope))
    }

    /// How many debug requests are still unanswered.
    pub fn pending_debug_requests(&self) -> usize {
        self.debug_requests.len()
    }

    /// The inspection rows the panel shows for the selected session.
    ///
    /// Described here rather than inside the view, so what the panel presents can be checked without a
    /// window: a frame row carries the location a user would be taken to, and a variable row carries
    /// the provider's own rendering of the value. Only the selected session's data is described, so a
    /// row can never belong to a session the user is not looking at.
    pub fn debug_panel_rows(&self) -> DebugPanelRows {
        // Protocol bounds limit the data; native scrolling keeps every reported row reachable.
        let frames = self
            .debug_frames()
            .iter()
            .map(|frame| DebugFrameRow {
                selector: format!("run-debug-frame-{}", frame.id),
                label: format!("{}  {}:{}", frame.name, frame.source, frame.line),
                frame: frame.id,
                selected: Some(frame.id) == self.selected_debug_frame(),
            })
            .collect::<Vec<_>>();
        let variables = self
            .selected_debug_frame()
            .map(|frame| {
                self.debug_variables(frame)
                    .iter()
                    .map(|variable| DebugVariableRow {
                        selector: format!("run-debug-variable-{frame}-{}", variable.name),
                        // The value is the provider's rendering and is shown as given.
                        label: format!("{} = {}", variable.name, variable.value),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        DebugPanelRows {
            frames,
            variables,
            location: self
                .debug_location()
                .map(|(source, line)| format!("{source}:{line}")),
            // A panel that is about to move the view asks this first: another session being stopped
            // means the user is reading something else.
            another_paused: self
                .debug_sessions
                .current()
                .map(|(config, _)| self.debug_sessions.another_is_paused(config))
                .unwrap_or(false),
        }
    }

    /// The frame the selected session's panel shows as selected.
    pub fn selected_debug_frame(&self) -> Option<u32> {
        self.debug_sessions
            .current()
            .and_then(|(_, session)| session.pause().selected_frame())
    }

    /// Which debug actions the panel may offer, each with the reason it may not.
    ///
    /// Both facts are assembled here so the panel and the launch path cannot disagree: availability
    /// decides starting, the selected session's own state decides the rest.
    pub fn debug_controls(&self) -> editor_core::DebugControls {
        // An action that is already in flight is not joined by a second one: the session's state is
        // about to change, so a click now would race the answer that has not arrived. Every control
        // carries that reason, including the ones the state alone would have allowed.
        if self.debug_action_pending() {
            let busy = || Err(t!("run.debug_busy").to_string().to_owned());
            return editor_core::DebugControls {
                start: busy(),
                resume: busy(),
                pause: busy(),
                stop: busy(),
                step: [
                    editor_core::DebugStep::Into,
                    editor_core::DebugStep::Over,
                    editor_core::DebugStep::Out,
                ]
                .into_iter()
                .map(|kind| (kind, busy()))
                .collect(),
            };
        }
        let state = self.debug_state();
        let availability = self.debug_availability();
        editor_core::DebugControls::derive_with(
            availability
                .as_ref()
                .map(|provider| *provider)
                .map_err(String::as_str),
            &state,
            self.selected_debug_capabilities(),
            debug_presentation::control_reason,
        )
    }

    /// Record what the selected debug provider declared it can do.
    pub fn note_debug_capabilities(&mut self, capabilities: editor_core::DebugCapabilities) {
        self.debug_capabilities = capabilities;
        if let Ok(provider) = self.debug_availability() {
            self.debug_provider_capabilities
                .insert(provider.to_owned(), capabilities);
        }
    }

    /// Replace the live registry atomically; an absent declaration cannot silently inherit a default.
    pub fn note_all_debug_capabilities(
        &mut self,
        capabilities: std::collections::BTreeMap<String, editor_core::DebugCapabilities>,
    ) {
        self.debug_provider_capabilities = capabilities;
    }

    /// Inspection and optional actions follow the selected target's original provider.
    pub fn selected_debug_capabilities(&self) -> editor_core::DebugCapabilities {
        self.debug_sessions
            .selected()
            .and_then(|config| self.debug_owners.get(config))
            .map(|owner| {
                self.debug_provider_capabilities
                    .get(owner)
                    .copied()
                    .unwrap_or_default()
            })
            .unwrap_or(self.debug_capabilities)
    }

    /// Whether this configuration may be debugged, and the reason it may not.
    ///
    /// Both halves matter: a configuration that cannot run cannot be debugged either, and a debug
    /// click that cannot proceed is refused with a reason rather than quietly behaving like Run.
    pub fn debug_blocker(&self, id: &str) -> Option<String> {
        if let Err(reason) = self.debug_availability() {
            return Some(reason.to_owned());
        }
        self.launch_blocker(id)
    }

    /// Why this configuration cannot be launched, or `None` when it can.
    ///
    /// This is the same validation the launch path performs, so a control that offers a launch and a
    /// launch that refuses cannot disagree about whether the configuration is usable. A
    /// configuration that does not exist is not launchable either: that is a reason, not a pass.
    pub fn launch_blocker(&self, id: &str) -> Option<String> {
        if self.running_for(id).is_some() {
            // Locating owned work does not launch the edited shared definition.
            return None;
        }
        let configs = match self.configurations_for_launch() {
            Ok(configs) => configs,
            Err(message) => return Some(message),
        };
        let Some(configuration) = configs.find(id) else {
            return Some(t!("run.no_configuration").into());
        };
        // The blocker and the ensuing launch must inspect the same newly loaded shared target.
        if let Some(error) = self.discovery_blocker(configuration) {
            return Some(error);
        }
        if self.configuration_target_missing(configuration) {
            return Some(t!("run.target_missing").into());
        }
        configuration
            .validate()
            .err()
            .map(|error| error.to_string())
    }

    /// Stage the choice of execution provider for one configuration.
    ///
    /// The choice is recorded on the configuration, so a later switch of the scope's default cannot
    /// silently retarget it, and the same choice is applied to the scope so every other launch path
    /// agrees with what the dialog shows.
    pub fn choose_provider(
        &mut self,
        id: &str,
        provider: Option<&str>,
        workspace: &str,
    ) -> Result<(), String> {
        let Some(mut configuration) = self.configs.find(id).cloned() else {
            return Err(t!("run.configuration_unknown", id = id).to_string());
        };
        configuration.provider = provider.map(str::to_owned);
        self.upsert(configuration, workspace)
    }

    /// Why this configuration's chosen provider cannot run, or `None` when it can.
    ///
    /// A configuration that follows the default is never in this state: no explicit request was
    /// made, so there is nothing to be missing.
    pub fn provider_unavailable(
        &self,
        id: &str,
        providers: &[plugin_runtime::ProviderCandidate],
    ) -> Option<String> {
        let chosen = self.configs.find(id)?.provider.as_deref()?;
        match providers
            .iter()
            .find(|candidate| candidate.plugin == chosen)
        {
            None => Some(t!("run.provider_not_installed", provider = chosen).to_string()),
            Some(candidate) => candidate.unavailable.clone(),
        }
    }

    /// Drop finished preparations, keeping only what is still running.
    pub fn prune_sequences(&mut self) {
        self.sequences.retain(|_, sequence| sequence.is_active());
    }

    /// Reconcile stored configurations with the targets the installed plugins now offer.
    ///
    /// A changed target is offered for repair; a missing target is reported; an unclaimed target is
    /// offered for confirmation. This read never changes an executable or writes a configuration.
    /// Merge only failed sources from the prior catalog; a successful empty source is definitive.
    pub(crate) fn accept_target_catalog(
        &mut self,
        mut catalog: crate::extensions::TargetCatalog,
    ) -> Result<(DiscoveryReport, String), String> {
        catalog.candidates.extend(
            self.discovered
                .iter()
                .filter(|target| {
                    catalog.failed.contains_key(&target.provider)
                        || (catalog.declarative_error.is_some()
                            && !target.fields.contains_key("provider_binding"))
                })
                .cloned(),
        );
        catalog.candidates.sort_by(|a, b| a.id.cmp(&b.id));
        catalog.candidates.dedup_by(|a, b| a.id == b.id);
        if catalog.candidates.len() > 128 {
            return Err(t!("run.target_catalog_limit").into());
        }
        let mut diagnostics = catalog
            .failed
            .iter()
            .map(|(provider, error)| format!("{provider}: {error}"))
            .collect::<Vec<_>>();
        diagnostics.extend(catalog.declarative_error.iter().cloned());
        self.discovery_failures = catalog.failed;
        self.declarative_discovery_error = catalog.declarative_error;
        let report = self.reconcile_discovered(&catalog.candidates);
        self.note_discovery();
        Ok((report, diagnostics.join("\n")))
    }

    /// Failure to inspect a source is not proof its targets disappeared; it is a repairable blocker.
    fn discovery_blocker(&self, config: &RunConfig) -> Option<String> {
        if let RunTarget::Provided { provider, .. } = &config.target {
            return self
                .discovery_failures
                .get(provider)
                .map(|error| format!("{provider}: {error}"));
        }
        config
            .from_target
            .as_ref()
            .and(self.declarative_discovery_error.clone())
    }

    pub fn reconcile_discovered(
        &mut self,
        targets: &[plugin_schema::DiscoveredTarget],
    ) -> DiscoveryReport {
        let outcome = editor_core::reconcile(&self.configs, targets);
        let repaired = outcome
            .updated
            .iter()
            .filter_map(|id| self.configs.find(id).map(|stored| stored.name.clone()))
            .collect();
        self.discovered = targets.to_vec();
        DiscoveryReport {
            repaired,
            offered: outcome.offered,
            missing: outcome
                .missing
                .into_iter()
                .filter_map(|id| {
                    self.configs
                        .find(&id)
                        .map(|config| (id.clone(), config.name.clone()))
                })
                .collect(),
        }
    }

    /// Apply the repair explicitly chosen in the menu, preserving the user's other configuration values.
    pub fn repair_target(&mut self, id: &str, workspace: &str) -> Result<(), String> {
        let configs = self.configurations_for_launch()?;
        let stored = configs
            .find(id)
            .ok_or_else(|| t!("run.target_config_missing").to_string())?;
        let target = self
            .discovered
            .iter()
            .find(|target| stored.claims_target(target))
            .ok_or_else(|| t!("run.target_missing").to_string())?;
        self.upsert(editor_core::repair(stored, target), workspace)
    }

    /// Rebind a disappeared target only to the candidate explicitly chosen in the native menu.
    pub fn repair_target_with(
        &mut self,
        id: &str,
        target: &str,
        workspace: &str,
    ) -> Result<(), String> {
        let configs = self.configurations_for_launch()?;
        let config = configs
            .find(id)
            .ok_or_else(|| t!("run.target_config_missing").to_string())?;
        let candidate = self
            .discovered
            .iter()
            .find(|candidate| candidate.id == target)
            .ok_or_else(|| t!("run.target_missing").to_string())?;
        self.upsert(editor_core::repair(config, candidate), workspace)
    }

    /// The currently proposed executable, shown in the confirmation action before it is applied.
    fn changed_target(&self, id: &str) -> Option<&plugin_schema::DiscoveredTarget> {
        let config = self.configs.find(id)?;
        self.discovered.iter().find(|target| {
            config.from_target.as_deref() == Some(target.id.as_str())
                && config.target.executable() != target.program
        })
    }

    /// The targets the last discovery offered, for the entry point that lists them.
    pub fn discovered_targets(&self) -> &[plugin_schema::DiscoveredTarget] {
        &self.discovered
    }

    /// Whether this configuration's target is no longer offered by any installed plugin.
    ///
    /// A configuration with no target link is never invalid this way, and neither is anything before
    /// a discovery has run: a target is only missing relative to a discovery that looked for it.
    pub fn target_missing(&self, id: &str) -> bool {
        self.configs
            .find(id)
            .is_some_and(|config| self.configuration_target_missing(config))
    }

    /// Evaluate a target from the caller's snapshot; launch paths pass freshly loaded shared data.
    fn configuration_target_missing(&self, config: &RunConfig) -> bool {
        if !self.discovery_ran {
            return false;
        }
        if config.from_target.is_none() && !matches!(config.target, RunTarget::Provided { .. }) {
            return false;
        }
        !self
            .discovered
            .iter()
            .any(|target| config.claims_target(target))
    }

    /// Configurations whose discovered target is no longer offered, by identity and name.
    pub fn invalid_targets(&self) -> Vec<(String, String)> {
        self.configs
            .configurations
            .iter()
            .filter(|config| self.target_missing(&config.id))
            .map(|config| (config.id.clone(), config.name.clone()))
            .collect()
    }

    /// Whether any discovery has run, so an empty list means "nothing found" rather than "not yet".
    pub fn discovery_ran(&self) -> bool {
        self.discovery_ran
    }

    /// Record that a discovery ran, so the entry point can tell the two empty states apart.
    pub fn note_discovery(&mut self) {
        self.discovery_ran = true;
    }

    /// Nonces survive workspace/model replacement by coming from one process-wide checked counter.
    pub fn begin_discovery(&mut self) -> Result<u64, String> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let nonce = NEXT
            .fetch_update(
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
                |value| value.checked_add(1),
            )
            .map_err(|_| "Discovery identity exhausted")?;
        self.discovery_request = Some(nonce);
        Ok(nonce)
    }
    /// Only the latest explicit invocation may reconcile a catalog; each result settles once.
    pub fn finish_discovery(&mut self, request: u64) -> bool {
        if self.discovery_request != Some(request) {
            return false;
        }
        self.discovery_request = None;
        true
    }

    /// The plan a launch of this configuration performs, or why it cannot be launched.
    pub fn launch_plan(&self, config: &str, workspace_root: &str) -> Result<RunPlan, String> {
        self.prepare_launch(config, workspace_root, MAX_PREPARED_STEPS)
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
    /// A discovered candidate the user may confirm; confirming stores it as a configuration.
    Target {
        id: String,
        label: String,
        /// The provider's own type, shown so two providers look different in the same list.
        target_type: String,
    },
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
                label: t!("run.menu_none").to_string().into(),
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
        for (request, view) in &self.provider_preparations {
            let name = self
                .configs
                .find(&view.config)
                .map(|config| config.name.as_str())
                .unwrap_or(&view.config);
            entries.push(RunMenuEntry::Action {
                id: format!("run-preparation-{request}"),
                label: format!(
                    "{name} · {} · {}",
                    view.name,
                    session_state_word(view.snapshot.state)
                ),
                enabled: true,
            });
        }
        entries.push(RunMenuEntry::Separator);
        if self.configs.configurations.is_empty() {
            entries.push(RunMenuEntry::Action {
                id: "run-empty".into(),
                label: t!("run.menu_empty").to_string().into(),
                enabled: false,
            });
        } else {
            for config in &self.configs.configurations {
                let selected = self.configs.selected.as_deref() == Some(config.id.as_str());
                let mut label = if selected {
                    format!("{} ✓", config.name)
                } else {
                    config.name.clone()
                };
                // A discovered configuration whose target disappeared is marked where it is chosen,
                // not only when it is launched and fails.
                if self.target_missing(&config.id) {
                    label.push_str(&t!("run.menu_invalid"));
                }
                entries.push(RunMenuEntry::Configuration {
                    id: config.id.clone(),
                    label,
                });
            }
        }
        entries.push(RunMenuEntry::Separator);
        entries.push(RunMenuEntry::Action {
            id: "run-edit".into(),
            label: t!("run.menu_edit").to_string().into(),
            enabled: self.configs.selected.is_some(),
        });
        entries.push(RunMenuEntry::Action {
            id: "run-new".into(),
            label: t!("run.menu_new").to_string().into(),
            enabled: true,
        });
        // Discovery is always offered: a workspace with no provider says so when it is asked, which
        // is more useful than a permanently unavailable entry that never explains itself.
        entries.push(RunMenuEntry::Action {
            id: "run-discover".into(),
            label: if self.discovered.is_empty() {
                t!("run.menu_discover").to_string().to_owned()
            } else {
                t!("run.menu_candidates", count = self.discovered.len()).to_string()
            },
            enabled: true,
        });
        // Candidates nobody has confirmed yet, offered after the stored configurations so the list a
        // user already knows stays where it was.
        for target in &self.discovered {
            if self
                .configs
                .configurations
                .iter()
                .any(|config| config.claims_target(target))
            {
                continue;
            }
            entries.push(RunMenuEntry::Target {
                id: target.id.clone(),
                label: target.label.clone(),
                target_type: target.target_type.clone(),
            });
        }
        // A configuration whose target is gone says so, so a failed launch is not the first hint.
        if let Some(config) = self
            .selected()
            .filter(|config| self.target_missing(&config.id))
        {
            for target in &self.discovered {
                entries.push(RunMenuEntry::Action {
                    id: format!(
                        "run-rebind-{}",
                        serde_json::to_string(&(&config.id, &target.id)).unwrap()
                    ),
                    label: format!("{} → {}", config.name, target.label),
                    enabled: true,
                });
            }
        }
        for (config, name) in self.invalid_targets() {
            entries.push(RunMenuEntry::Action {
                id: format!("run-repair-{config}"),
                label: t!("run.menu_repair", name = name).to_string(),
                enabled: true,
            });
        }
        for config in &self.configs.configurations {
            if let Some(target) = self.changed_target(&config.id) {
                entries.push(RunMenuEntry::Action {
                    id: format!("run-apply-repair-{}", config.id),
                    label: t!(
                        "run.target_confirm_repair",
                        name = &config.name,
                        program = &target.program
                    )
                    .to_string(),
                    enabled: true,
                });
            }
        }
        entries
    }
}

/// The state word shown beside a session in the dropdown and in the title bar.
pub fn session_state_word(state: plugin_runtime::ExecutionState) -> String {
    match state {
        plugin_runtime::ExecutionState::Starting => t!("run.state_starting").to_string(),
        plugin_runtime::ExecutionState::Running => t!("run.state_running").to_string(),
        plugin_runtime::ExecutionState::Stopping => t!("run.state_stopping").to_string(),
        plugin_runtime::ExecutionState::Terminating => t!("run.state_terminating").to_string(),
        plugin_runtime::ExecutionState::Failed | plugin_runtime::ExecutionState::Exited => {
            t!("run.state_ended").to_string()
        }
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
    /// Where the configuration being edited came from, so an edit writes back to that place.
    pub source: editor_core::RunConfigSource,
    /// The discovered target this configuration came from, when a plugin offered it.
    pub from_target: Option<String>,
    /// Structured provider bindings survive edits; changing the program field explicitly detaches.
    pub provided: Option<RunTarget>,
    /// Provider build actions remain structured while the editable list shows ordinary user commands.
    pub provider_build: Vec<(usize, editor_core::RunStep)>,
    /// Opaque prelaunch actions retain their original ordering without exposing private binding data.
    pub provider_prelaunch: Vec<(usize, editor_core::RunStep)>,
    /// Unedited literal argument lists keep empty and multi-line values when reopened.
    pub original_arguments: Option<Vec<String>>,
    /// The execution provider this configuration asks for, or `None` to follow the default.
    ///
    /// The choice belongs to the configuration, so a later switch of the default does not silently
    /// retarget a configuration that asked for a specific provider.
    pub provider: Option<String>,
    /// Whether the user chose to share this configuration with the project.
    pub share: bool,
    /// Build actions as `名称 = 程序或解释器 | 参数 | 脚本`, one per line.
    pub build: String,
    /// Steps that run in order before the program, in the same line form as the build actions.
    pub prelaunch: String,
    /// Breakpoints as `源文件:行号`, one per line.
    pub breakpoints: String,
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
                    RunTarget::Program { args, .. }
                    | RunTarget::Script { args, .. }
                    | RunTarget::Provided { args, .. } => args.join("\n"),
                },
                script: match &config.target {
                    RunTarget::Script { script, .. } => script.clone(),
                    RunTarget::Program { .. } | RunTarget::Provided { .. } => String::new(),
                },
                directory: config.directory.clone().unwrap_or_default(),
                environment: render_environment(&config.env),
                tool_paths: config.tool_paths.join("\n"),
                source: config.source,
                from_target: config.from_target.clone(),
                provided: matches!(config.target, RunTarget::Provided { .. })
                    .then(|| config.target.clone()),
                provider_build: config
                    .build
                    .iter()
                    .enumerate()
                    .filter(|(_, step)| {
                        matches!(
                            &step.target,
                            editor_core::StepTarget::Action {
                                target: RunTarget::Provided { .. }
                            }
                        )
                    })
                    .map(|(index, step)| (index, step.clone()))
                    .collect(),
                provider_prelaunch: config
                    .prelaunch
                    .iter()
                    .enumerate()
                    .filter(|(_, step)| {
                        matches!(
                            &step.target,
                            editor_core::StepTarget::Action {
                                target: RunTarget::Provided { .. }
                            }
                        )
                    })
                    .map(|(index, step)| (index, step.clone()))
                    .collect(),
                // Match exactly the editable argv field; Shell appends its script only at launch.
                original_arguments: Some(match &config.target {
                    RunTarget::Program { args, .. }
                    | RunTarget::Script { args, .. }
                    | RunTarget::Provided { args, .. } => args.clone(),
                }),
                provider: config.provider.clone(),
                breakpoints: render_breakpoints(&config.breakpoints),
                share: !config.local,
                build: render_steps(
                    &config
                        .build
                        .iter()
                        .filter(|step| {
                            !matches!(
                                &step.target,
                                editor_core::StepTarget::Action {
                                    target: RunTarget::Provided { .. }
                                }
                            )
                        })
                        .cloned()
                        .collect::<Vec<_>>(),
                ),
                prelaunch: render_steps(
                    &config
                        .prelaunch
                        .iter()
                        .filter(|step| {
                            !matches!(
                                &step.target,
                                editor_core::StepTarget::Action {
                                    target: RunTarget::Provided { .. }
                                }
                            )
                        })
                        .cloned()
                        .collect::<Vec<_>>(),
                ),
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
                source: editor_core::RunConfigSource::Local,
                from_target: None,
                provided: None,
                provider_build: vec![],
                provider_prelaunch: vec![],
                original_arguments: None,
                provider: None,
                breakpoints: String::new(),
                // Sharing is an explicit choice; a new configuration starts on this machine only.
                share: false,
                build: String::new(),
                prelaunch: String::new(),
            },
        }
    }

    /// Insert each opaque provider action at its original position, preserving earlier generators.
    /// Detaching the final target removes only its own auto-build, never unrelated provider actions.
    fn restore_opaque_steps(
        &self,
        mut steps: Vec<editor_core::RunStep>,
        opaque: &[(usize, editor_core::RunStep)],
        detach: bool,
    ) -> Vec<editor_core::RunStep> {
        for (index, step) in opaque {
            if detach
                && matches!(&step.target,editor_core::StepTarget::Action {target} if provided_binding(target)==self.provided.as_ref().and_then(provided_binding))
            {
                continue;
            }
            steps.insert((*index).min(steps.len()), step.clone());
        }
        steps
    }

    /// Build the configuration this draft describes.
    ///
    /// The mode decides which fields mean what: program mode keeps a literal argv and never composes
    /// a command line, shell mode passes the script text to the named interpreter as one argument.
    /// A malformed environment line is reported here, so an unusable entry never reaches a launch.
    pub fn to_config(&self) -> Result<RunConfig, String> {
        let arguments = if let Some(original) = &self.original_arguments
            && original.join("\n") == self.arguments
        {
            original.clone()
        } else {
            self.arguments
                .lines()
                .map(str::to_owned)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
        };
        let keeps_provider = !self.shell
            && self
                .provided
                .as_ref()
                .is_some_and(|target| target.executable() == self.program.trim());
        let target = if keeps_provider {
            let mut target = self.provided.clone().unwrap();
            if let RunTarget::Provided { args, .. } = &mut target {
                *args = arguments;
            }
            target
        } else if self.shell {
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
            build: self.restore_opaque_steps(
                parse_steps(&self.build)?,
                &self.provider_build,
                !keeps_provider,
            ),
            prelaunch: self.restore_opaque_steps(
                parse_steps(&self.prelaunch)?,
                &self.provider_prelaunch,
                false,
            ),
            source: self.source,
            // Editing a configuration by hand keeps the target it came from, so a later discovery
            // still recognizes it as its own rather than offering to add a second copy.
            from_target: self.from_target.clone(),
            provider: self.provider.clone(),
            // A stop location the user wrote is theirs; a line that cannot be parsed is refused by
            // the form before it reaches here, so this only converts the accepted text.
            breakpoints: parse_breakpoints(&self.breakpoints)?,
            local: !self.share,
        })
    }
}

impl RunConfigDraft {
    /// The text one editable list currently holds, addressed by the field that edits it.
    ///
    /// This keeps the row controls independent of which list they belong to: the caller passes the
    /// field it is rendering and gets that list back.
    pub fn field_text(&self, field: crate::run::ui::RunField) -> String {
        match field {
            crate::run::ui::RunField::Build => self.build.clone(),
            crate::run::ui::RunField::Prelaunch => self.prelaunch.clone(),
            _ => String::new(),
        }
    }
}

/// Extract a generic preparation binding; no provider fields or languages are interpreted here.
fn provided_binding(target: &RunTarget) -> Option<(String, String)> {
    if let RunTarget::Provided {
        provider, binding, ..
    } = target
    {
        Some((provider.clone(), binding.clone()))
    } else {
        None
    }
}

mod step_text;
pub use step_text::{parse_steps, render_steps};

/// The actions one list holds, as editable lines.
///
/// Blank lines are not actions, so they are left out of the list the controls act on: moving or
/// deleting a row must address the action the user sees rather than the spacing around it.
pub fn step_lines(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect()
}

/// Write editable lines back into the field's text.
pub fn join_step_lines(lines: &[String]) -> String {
    lines.join("\n")
}

/// Move one action one place earlier or later in its list.
///
/// Returns the new text, or `None` when the move would go past an end — a control that cannot move a
/// row stays disabled instead of silently doing nothing.
pub fn move_step(text: &str, index: usize, up: bool) -> Option<String> {
    let mut lines = step_lines(text);
    if index >= lines.len() {
        return None;
    }
    let target = if up {
        index.checked_sub(1)?
    } else {
        let next = index + 1;
        if next >= lines.len() {
            return None;
        }
        next
    };
    lines.swap(index, target);
    Some(join_step_lines(&lines))
}

/// Remove one action from its list.
pub fn remove_step(text: &str, index: usize) -> Option<String> {
    let mut lines = step_lines(text);
    if index >= lines.len() {
        return None;
    }
    lines.remove(index);
    Some(join_step_lines(&lines))
}

/// Add a line for the next action, written as a comment so it is a template rather than an action.
///
/// The comment is a real line the user can edit in place, yet it stays out of the parsed actions: an
/// empty line would simply vanish, and a bare name would make the configuration invalid until it was
/// finished.
pub fn add_step(text: &str) -> String {
    let mut lines = step_lines(text);
    lines.push(t!("run.steps_format_header").to_string());
    join_step_lines(&lines)
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
            return Err(t!("run.environment_syntax", line = line).to_string());
        };
        let name = name.trim();
        if name.is_empty() {
            return Err(t!("run.environment_name_missing", line = line).to_string());
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

/// Parse breakpoints written as `源文件:行号`, one per line.
///
/// The colon form is used rather than two fields so a whole list can be pasted at once. A Windows
/// drive letter is not confused with the separator, because the line number is what follows the last
/// colon and has to be a number.
pub fn parse_breakpoints(text: &str) -> Result<editor_core::RunBreakpoints, String> {
    let mut breakpoints = editor_core::RunBreakpoints::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((source, number)) = line.rsplit_once(':') else {
            return Err(t!("run.breakpoint_syntax", line = line).to_string());
        };
        let Ok(number) = number.trim().parse::<u32>() else {
            return Err(t!("run.breakpoint_line_invalid", line = line).to_string());
        };
        match breakpoints.insert(source.trim(), number) {
            Ok(()) => {}
            // The same location twice is a correction the user already made, not a second breakpoint.
            Err(editor_core::BreakpointError::AlreadySet) => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(breakpoints)
}

/// Render breakpoints back into the one-per-line form the field edits.
pub fn render_breakpoints(breakpoints: &editor_core::RunBreakpoints) -> String {
    breakpoints
        .entries()
        .iter()
        .map(|entry| format!("{}:{}", entry.source, entry.line))
        .collect::<Vec<_>>()
        .join("\n")
}
