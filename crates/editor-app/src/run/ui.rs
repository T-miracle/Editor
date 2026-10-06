//! Native run controls: the title-bar group and the B1 configuration dialog.
//!
//! Every control here is a native widget composed from the project's own UI layer; no WebView is
//! used. The group's position, separation and disabled states follow the approved B1 layout, and a
//! control whose capability is not implemented yet is disabled with a visible reason rather than
//! behaving like the control next to it.
use super::{
    LaunchPlan, MAX_PREPARED_STEPS, RunConfigDraft, RunControls, RunMenuEntry, SequenceAction,
    StepKind, add_step, join_step_lines, move_step, remove_step, step_lines,
};
use crate::extensions::HostWork as Work;
use crate::ui::controls::menu::MenuStyle;
use crate::ui::controls::{Button, DialogContent};
// The crate root already selects the same widget and styling traits the rest of the shell uses.
use crate::ui::controls::menu::PopupMenu as NativePopupMenu;
use crate::*;
use gpui_base::input::{InputEvent, InputState, TextareaState};
use gpui_kit::{AnyElement, DismissEvent, WeakEntity, Window, div, px};
use rust_i18n::t;
use sha2::{Digest, Sha256};

mod build_output;
pub(crate) mod panel;

/// Width of the unified run dropdown; it holds session labels with state words beside them.
const RUN_MENU_WIDTH: f32 = 260.;

/// The unified dropdown retaining the component lifetime and its dismissal subscription.
pub(crate) struct RunMenu {
    pub popup: Entity<NativePopupMenu>,
    _dismiss: Subscription,
}

/// A same-window modal entity renders after EditorApp releases its lease.
/// It observes the editor and retained draft; closing revokes nodes without destroying an HWND.
pub(crate) struct RunConfigModal {
    owner: WeakEntity<EditorApp>,
    focus: FocusHandle,
    _updates: Vec<Subscription>,
    /// Target confirmation can replace the entire draft, so observation follows its current identity.
    observed_form: Option<Entity<RunConfigForm>>,
    _form_update: Option<Subscription>,
}
impl Render for RunConfigModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(app) = self.owner.upgrade() else {
            return div().into_any_element();
        };
        let Some(form) = app.read(cx).run_form.clone() else {
            return div().into_any_element();
        };
        if self
            .observed_form
            .as_ref()
            .is_none_or(|old| old.entity_id() != form.entity_id())
        {
            self._form_update = Some(cx.observe(&form, |_, _, cx| cx.notify()));
            self.observed_form = Some(form.clone());
        }
        let title = if app
            .read(cx)
            .run_controls
            .configuration(&form.read(cx).draft.id)
            .is_some()
        {
            t!("run.form_edit_title").to_string()
        } else {
            t!("run.form_new_title").to_string()
        };
        let cancel = self.owner.clone();
        let save = self.owner.clone();
        crate::ui::controls::dialog::modal(
            self.focus.clone(),
            title,
            render_run_config_form(&self.owner, DialogContent::new(), cx).into_any_element(),
            move |_, cx| {
                let _ = cancel.update(cx, |state, cx| state.close_run_form(cx));
            },
            move |_, cx| {
                save.update(cx, |state, cx| {
                    state.commit_run_form(cx);
                    state.run_form.is_none()
                })
                .unwrap_or(true)
            },
            window,
            cx,
        )
    }
}

/// The four B1 pages edit one retained draft; changing pages never drops typed values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunConfigTab {
    Basic,
    Build,
    Debug,
    Environment,
}

impl RunConfigTab {
    const ALL: [Self; 4] = [Self::Basic, Self::Build, Self::Debug, Self::Environment];

    fn label(self) -> String {
        match self {
            Self::Basic => t!("run.form_basic").to_string(),
            Self::Build => t!("run.form_build").to_string(),
            Self::Debug => t!("run.form_debug").to_string(),
            Self::Environment => t!("run.form_environment").to_string(),
        }
    }

    fn selector(self) -> &'static str {
        match self {
            Self::Basic => "run-config-tab-basic",
            Self::Build => "run-config-tab-build",
            Self::Debug => "run-config-tab-debug",
            Self::Environment => "run-config-tab-environment",
        }
    }

    fn hint(self) -> String {
        match self {
            Self::Basic => t!("run.hint_basic").to_string(),
            Self::Build => t!("run.hint_build").to_string(),
            Self::Debug => t!("run.hint_debug").to_string(),
            Self::Environment => t!("run.hint_environment").to_string(),
        }
    }

    /// Whether this page edits a stored configuration today.
    fn available(self) -> bool {
        // Every page now edits something: the debug page chooses the execution provider.
        matches!(
            self,
            Self::Basic | Self::Environment | Self::Build | Self::Debug
        )
    }
}

/// One labelled configuration field; scripts and row collections retain multi-line editing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunField {
    Name,
    Program,
    Arguments,
    Script,
    Directory,
    Environment,
    ToolPaths,
    Build,
    Prelaunch,
    /// Breakpoints as `源文件:行号`, one per line.
    Breakpoints,
}

impl RunField {
    const ALL: [Self; 10] = [
        Self::Name,
        Self::Program,
        Self::Arguments,
        Self::Script,
        Self::Directory,
        Self::Environment,
        Self::ToolPaths,
        Self::Build,
        Self::Prelaunch,
        Self::Breakpoints,
    ];

    /// Separate input kinds preserve literal argv lines and real multi-line script/IME editing.
    fn multiline(self) -> bool {
        matches!(
            self,
            Self::Arguments
                | Self::Script
                | Self::Environment
                | Self::ToolPaths
                | Self::Breakpoints
        )
    }
    fn label(self) -> String {
        match self {
            Self::Name => t!("run.field_name").to_string(),
            Self::Program => t!("run.field_program").to_string(),
            // One argument per line keeps a value containing spaces literal.
            Self::Arguments => t!("run.field_arguments").to_string(),
            Self::Script => t!("run.field_script").to_string(),
            Self::Directory => t!("run.field_directory").to_string(),
            Self::Environment => t!("run.field_environment").to_string(),
            Self::ToolPaths => t!("run.field_tool_paths").to_string(),
            Self::Build => t!("run.field_build").to_string(),
            Self::Prelaunch => t!("run.field_prelaunch").to_string(),
            Self::Breakpoints => t!("run.field_breakpoints").to_string(),
        }
    }

    fn selector(self) -> &'static str {
        match self {
            Self::Name => "run-config-name",
            Self::Program => "run-config-program",
            Self::Arguments => "run-config-arguments",
            Self::Script => "run-config-script",
            Self::Directory => "run-config-directory",
            Self::Environment => "run-config-environment",
            Self::ToolPaths => "run-config-tool-paths",
            Self::Build => "run-config-build",
            Self::Prelaunch => "run-config-prelaunch",
            Self::Breakpoints => "run-config-breakpoints",
        }
    }

    fn value(self, draft: &RunConfigDraft) -> String {
        match self {
            Self::Name => draft.name.clone(),
            Self::Program => draft.program.clone(),
            Self::Arguments => draft.arguments.clone(),
            Self::Script => draft.script.clone(),
            Self::Directory => draft.directory.clone(),
            Self::Environment => draft.environment.clone(),
            Self::ToolPaths => draft.tool_paths.clone(),
            Self::Build => draft.build.clone(),
            Self::Prelaunch => draft.prelaunch.clone(),
            Self::Breakpoints => draft.breakpoints.clone(),
        }
    }

    fn apply(self, draft: &mut RunConfigDraft, value: String) {
        match self {
            Self::Name => draft.name = value,
            Self::Program => draft.program = value,
            Self::Arguments => draft.arguments = value,
            Self::Script => draft.script = value,
            Self::Directory => draft.directory = value,
            Self::Environment => draft.environment = value,
            Self::ToolPaths => draft.tool_paths = value,
            Self::Build => draft.build = value,
            Self::Prelaunch => draft.prelaunch = value,
            Self::Breakpoints => draft.breakpoints = value,
        }
    }
}

/// One structural change to a prepared action list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StepEdit {
    Add,
    Remove,
    Up,
    Down,
}

/// State of the configuration dialog, owned by one entity so a draft survives repaints.
///
/// The dialog's renderer in this module reads these fields; the entity exists so the draft, its text
/// editing state and its subscriptions share one lifetime instead of resetting on every frame.
#[allow(dead_code)]
pub struct RunConfigForm {
    tab: RunConfigTab,
    /// The configuration being edited, including its stable identity.
    draft: RunConfigDraft,
    /// Validation or storage message shown above the dialog buttons.
    error: Option<String>,
    /// Editing state for each field, created with the dialog so text never resets per frame.
    inputs: Vec<(RunField, Entity<InputState>)>,
    /// Lists/scripts need actual Enter and multi-line selection, rather than a single-line placeholder.
    textareas: Vec<(RunField, Entity<TextareaState>)>,
    /// One editing state per prepared action, for the lists that are edited row by row.
    ///
    /// A row is its own single-line field so an action can be moved or removed without its text
    /// being re-parsed, and so the row controls always address the action the user sees.
    rows: Vec<(RunField, Entity<InputState>)>,
    /// Subscriptions are retained here; dropping the form releases them with its inputs.
    _subscriptions: Vec<Subscription>,
}

impl RunConfigForm {
    /// Open the dialog on a stored configuration, or on an empty draft that creates a new one.
    pub fn open(
        controls: &RunControls,
        workspace: &str,
        editing: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let id = editing
            .map(str::to_owned)
            .unwrap_or_else(|| controls.generate_id(workspace));
        let existing = editing.and_then(|id| controls.configuration(&id));
        let draft = RunConfigDraft::from_config(existing, id);
        let mut form = Self {
            tab: RunConfigTab::Basic,
            draft,
            error: None,
            inputs: Vec::new(),
            textareas: Vec::new(),
            rows: Vec::new(),
            _subscriptions: Vec::new(),
        };
        for field in RunField::ALL {
            let initial = field.value(&form.draft);
            let label = field.label();
            if field.multiline() {
                let input = cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .rows(3)
                        .default_value(initial)
                        .placeholder(label.to_string())
                });
                form._subscriptions.push(cx.subscribe(
                    &input,
                    |form, _, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change) && form.error.take().is_some() {
                            cx.notify();
                        }
                    },
                ));
                form.textareas.push((field, input));
                continue;
            }
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(initial)
                    .placeholder(label.to_string())
            });
            // A change clears a previous rejection message, because the form is being corrected.
            let subscription = cx.subscribe(&input, move |form, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) && form.error.take().is_some() {
                    cx.notify();
                }
            });
            form._subscriptions.push(subscription);
            form.inputs.push((field, input));
        }
        form.rebuild_rows(window, cx);
        form
    }

    /// Create one editing state per prepared action of each list.
    fn rebuild_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.rows.clear();
        for field in [RunField::Build, RunField::Prelaunch] {
            let lines = step_lines(&self.draft.field_text(field));
            for line in lines {
                let input = cx.new(|cx| InputState::new(window, cx).default_value(line));
                let subscription = cx.subscribe(&input, move |form, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) && form.error.take().is_some() {
                        cx.notify();
                    }
                });
                self._subscriptions.push(subscription);
                self.rows.push((field, input));
            }
        }
    }

    /// The rows of one list, in the order they will run.
    pub(crate) fn rows_of(&self, field: RunField) -> Vec<(usize, Entity<InputState>)> {
        self.rows
            .iter()
            .filter(|(candidate, _)| *candidate == field)
            .enumerate()
            .map(|(index, (_, input))| (index, input.clone()))
            .collect()
    }

    /// Read every row of one list back into the draft, before a structural change is applied.
    fn sync_rows(&mut self, field: RunField, cx: &gpui_kit::App) {
        let lines = self
            .rows_of(field)
            .into_iter()
            .map(|(_, input)| input.read(cx).value().to_string())
            .collect::<Vec<_>>();
        let text = join_step_lines(&lines);
        match field {
            RunField::Build => self.draft.build = text,
            RunField::Prelaunch => self.draft.prelaunch = text,
            _ => {}
        }
    }

    /// Apply a structural change to one list and rebuild its rows.
    ///
    /// The rows are read back first, so a change acts on what the user has typed rather than on the
    /// text that was there when the dialog opened.
    pub(crate) fn edit_rows(
        &mut self,
        field: RunField,
        change: StepEdit,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Rebuilding replaces both lists. Preserve both sets of unsaved native input first.
        self.sync_rows(RunField::Build, cx);
        self.sync_rows(RunField::Prelaunch, cx);
        let current = match field {
            RunField::Build => self.draft.build.clone(),
            RunField::Prelaunch => self.draft.prelaunch.clone(),
            _ => return,
        };
        let updated = match change {
            StepEdit::Add => Some(add_step(&current)),
            StepEdit::Remove => remove_step(&current, index),
            StepEdit::Up => move_step(&current, index, true),
            StepEdit::Down => move_step(&current, index, false),
        };
        let Some(updated) = updated else {
            return;
        };
        match field {
            RunField::Build => self.draft.build = updated,
            RunField::Prelaunch => self.draft.prelaunch = updated,
            _ => {}
        }
        // The row controls belong to the list that was just rearranged, so the states are rebuilt
        // together with it; nothing else reads a row state in between.
        self.rebuild_rows(window, cx);
        cx.notify();
    }

    /// Current text of one field, read from its editing state.
    fn text(&self, field: RunField, cx: &gpui_kit::App) -> String {
        if let Some((_, input)) = self
            .textareas
            .iter()
            .find(|(candidate, _)| *candidate == field)
        {
            return input.read(cx).value().to_string();
        }
        self.inputs
            .iter()
            .find(|(candidate, _)| *candidate == field)
            .map(|(_, input)| input.read(cx).value().to_string())
            .unwrap_or_default()
    }

    /// Copy every field's current text into the draft before it is validated or saved.
    fn collect(&mut self, cx: &gpui_kit::App) {
        for field in RunField::ALL {
            let value = self.text(field, cx);
            field.apply(&mut self.draft, value);
        }
        // The row-edited lists are gathered from their own rows, so what is saved is what is on screen.
        for field in [RunField::Build, RunField::Prelaunch] {
            self.sync_rows(field, cx);
        }
    }
}

/// Structural accessors exist only for native checks, so production builds carry no unused surface.
#[cfg(test)]
impl RunConfigForm {
    /// The tabs this dialog offers, with whether this build implements each one.
    ///
    /// Lets a native check confirm the approved structure without depending on how the strip is
    /// painted in the dialog's own window.
    pub(crate) fn tab_labels(&self) -> Vec<(String, bool)> {
        RunConfigTab::ALL
            .into_iter()
            .map(|tab| (tab.label(), tab.available()))
            .collect()
    }

    /// The labelled fields of the basic page, in the order they are presented.
    pub(crate) fn field_labels(&self) -> Vec<String> {
        RunField::ALL.into_iter().map(RunField::label).collect()
    }

    /// The editing state of one field, so a check can address the field a user types into.
    ///
    /// Composition is delivered to the retained control state rather than to the rendered element,
    /// which is why this returns the state itself and not a node.
    pub(crate) fn field_input(&self, field: RunField) -> Option<Entity<InputState>> {
        self.inputs
            .iter()
            .find(|(candidate, _)| *candidate == field)
            .map(|(_, input)| input.clone())
    }

    /// How many rows one prepared-action list currently shows.
    pub(crate) fn step_row_count(&self, field: RunField) -> usize {
        self.rows_of(field).len()
    }

    /// The text of each row of one prepared-action list, in the order shown.
    pub(crate) fn step_row_values(&self, field: RunField, cx: &gpui_kit::App) -> Vec<String> {
        self.rows_of(field)
            .into_iter()
            .map(|(_, input)| input.read(cx).value().to_string())
            .collect()
    }

    /// Choose whether this configuration is shared with the project, as the footer's control does.
    pub(crate) fn share_with_project(&mut self, share: bool) {
        self.draft.share = share;
        self.error = None;
    }

    /// Whether a save from this form would write only to this machine.
    pub(crate) fn destination_is_local(&self) -> bool {
        !self.draft.share
    }

    /// The draft currently being edited, for checks that read what the dialog would save.
    pub(crate) fn draft(&self) -> &RunConfigDraft {
        &self.draft
    }

    /// The validation or storage message shown above the dialog buttons, if any.
    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The page the dialog currently shows.
    pub(crate) fn tab(&self) -> RunConfigTab {
        self.tab
    }
}

impl EditorApp {
    /// Read published host sessions into the run controls before the frame is painted.
    pub(crate) fn sync_run_controls(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (executions, errors, stops, statuses) = self.extensions.read(cx).take_host_runs();
        for (session, request, result) in self.extensions.read(cx).take_run_locations() {
            if self.run_controls.finish_location(session, request) {
                self.status = match result {
                    Ok(()) => t!("run.located", session = session.to_string()).into(),
                    Err(error) => t!("run.locate_failed", error = error).into(),
                };
            }
        }
        // Target receipts belong to their original workspace and plan, independent of current selection.
        for (workspace, request, result) in self.extensions.read(cx).take_target_discoveries() {
            if workspace != self.workspace_key() || !self.run_controls.finish_discovery(request) {
                continue;
            }
            match result {
                Ok(catalog) => match self.run_controls.accept_target_catalog(catalog) {
                    Ok((report, diagnostics)) => {
                        let summary = t!(
                            "run.target_discovery_summary",
                            count = self.run_controls.discovered_targets().len(),
                            repairs = report.repaired.len(),
                            offered = report.offered.len(),
                            missing = report.missing.len()
                        );
                        self.status = if diagnostics.is_empty() {
                            summary.into()
                        } else {
                            format!("{summary}\n{diagnostics}")
                        };
                    }
                    Err(error) => self.status = error,
                },
                Err(error) => self.status = error,
            }
        }
        for (request, (config, _, snapshot)) in self.extensions.read(cx).take_target_snapshots() {
            self.run_controls
                .observe_provider_preparation(request, &config, snapshot);
        }
        for (config, index, request, result) in self.extensions.read(cx).take_target_preparations()
        {
            if self.run_controls.provider_prepared(
                &config,
                index,
                request,
                result.as_deref().map_err(String::as_str),
            ) {
                if let Err(error) = result {
                    self.status = error;
                }
            }
        }
        // The host's own answer about debugging is carried into the controls, so a control that
        // offers a debug launch and the launch itself cannot disagree about whether there is one.
        if let Some(availability) = self.extensions.read(cx).debug_availability() {
            self.run_controls.note_debug_availability(availability);
        }
        // The abilities are what the panel may offer. Without them every capability defaults to false,
        // which is right for an unknown provider but wrong for a known one.
        if let Some(capabilities) = self.extensions.read(cx).debug_capabilities() {
            self.run_controls.note_debug_capabilities(capabilities);
        }
        self.run_controls
            .note_all_debug_capabilities(self.extensions.read(cx).all_debug_capabilities());
        // Debug answers arrive like every other host publication, and are applied to the pause they
        // were asked about: a late one is reported rather than replacing a newer view.
        for (request, answer) in self.extensions.read(cx).take_debug_answers() {
            use crate::extensions::DebugAnswerMessage;
            let outcome = match answer {
                DebugAnswerMessage::Connecting(session) => {
                    self.run_controls.note_debug_connecting(request, &session)
                }
                DebugAnswerMessage::Frames(frames) => self.run_controls.apply_debug_answer(
                    request,
                    Some(
                        frames
                            .into_iter()
                            .map(|frame| editor_core::StackFrame {
                                id: frame.id,
                                name: frame.name,
                                source: frame.source,
                                line: frame.line,
                            })
                            .collect(),
                    ),
                    None,
                ),
                DebugAnswerMessage::Variables(variables) => self.run_controls.apply_debug_answer(
                    request,
                    None,
                    Some(
                        variables
                            .into_iter()
                            .map(|variable| editor_core::DebugVariable {
                                name: variable.name,
                                value: variable.value,
                            })
                            .collect(),
                    ),
                ),
                // A step is answered by a state, so applying it is what begins the new pause; the
                // provider's word is translated, never a state the host assumed.
                DebugAnswerMessage::State(session) => {
                    self.run_controls.apply_debug_state_reply(request, &session)
                }
                DebugAnswerMessage::Breakpoints(bound) => {
                    // The answer replaces what is known about this session's positions, so a
                    // breakpoint the provider refused stops being reported as bound.
                    self.run_controls.apply_debug_breakpoint_answer(
                        request,
                        bound
                            .into_iter()
                            .map(|entry| (entry.source, entry.line, entry.verified)),
                    )
                }
                // A failed call is released and reported; it is never shown as an empty stack.
                DebugAnswerMessage::Failed(message) => {
                    self.status = message.clone();
                    self.run_controls.fail_debug_reply(request, message)
                }
            };
            if let Err(reason) = outcome {
                self.status = super::debug_presentation::inspection_error(&reason);
            }
        }
        // A pause is where the user wants to see the stack, so the editor asks for it. Without this
        for report in self.extensions.read(cx).take_debug_observations() {
            if self.run_controls.observe_debug_report(&report)
                && let (Some(source), Some(line)) = (report.source.as_deref(), report.line)
                && !source.is_empty()
                && line > 0
            {
                self.open_debug_location(source, line, window, cx);
            }
        }
        // Inspection belongs to the newly observed pause, never to cached output text.
        // the panel renders frames and variables that nobody ever requested.
        self.fetch_debug_inspection(cx);
        if !self.leave_confirmed && !self.shutting_down {
            for configuration in self.run_controls.take_ready_debug_reruns() {
                self.debug_configuration(&configuration, cx);
            }
        }
        // And it is the moment to tell the debugger where to stop: a session that began before the user
        // set its breakpoints would otherwise never learn them, and a removed one would stay set. Only
        // while a session exists, because there is nothing to address before that.
        self.send_debug_breakpoints(cx);

        self.run_controls.reconcile(&executions);
        // A step's session belongs to its preparation as soon as the runtime publishes it, so the
        // sequence owns it before the provider has even confirmed the program.
        let adopted = executions
            .iter()
            .map(|execution| {
                (
                    execution.id,
                    execution.request_id,
                    execution.provider_session.clone(),
                )
            })
            .collect::<Vec<_>>();
        self.run_controls.adopt_step_sessions(&adopted);

        if let Some((_, _, message)) = errors.first() {
            // A refused start is reported where the launch was requested instead of failing silently.
            self.status = message.clone();
        }
        for (session, result) in self.run_controls.reconcile_stops(&stops) {
            // An acknowledgement means the provider was asked, not that the program has exited, so
            // the visible state never claims more than the provider actually reported.
            self.status = match &result {
                Ok(()) => t!("run.stop_requested", session = session).to_string(),
                Err(message) => {
                    t!("run.stop_failed", session = session, message = message).to_string()
                }
            };
        }
        for config in self.run_controls.finish_stopped_preparations() {
            self.finish_preparation(&config, cx);
        }
        // An observed end advances its sequence; a program still running is not progress.
        for (config, _, _outcome) in self.run_controls.reconcile_run_status(&statuses) {
            self.drive_preparation(cx);
            if let Some(sequence) = self.run_controls.preparation(&config)
                && !sequence.is_active()
            {
                let finished = config.clone();
                self.finish_preparation(&finished, cx);
            }
            break;
        }
        self.drive_preparation(cx);
        if !self.leave_confirmed && !self.shutting_down {
            for (config, debug) in self.run_controls.take_ready_preparation_reruns() {
                let workspace = self.workspace_key();
                self.run_controls.select(&config, &workspace);
                if debug {
                    self.debug_configuration(&config, cx);
                } else {
                    self.start_configuration_without_environment(&config, window, cx);
                }
            }
            for (config, outcome) in self.run_controls.take_ready_reruns() {
                match outcome {
                    Ok(()) => self.start_configuration_without_environment(&config, window, cx),
                    Err(reason) => self.status = reason,
                }
            }
        }
    }

    /// Whether this workspace may start programs at all; a restricted workspace never launches.
    pub(crate) fn run_permitted(&self, cx: &Context<Self>) -> bool {
        self.extensions.read(cx).workspace_trusted()
    }

    /// Open the one run dropdown: active sessions, saved configurations, and the edit entries.
    ///
    /// Grouping is what keeps the title bar compact: running work and future launches are separate
    /// lists in one component instead of two permanent selectors.
    pub(crate) fn open_run_menu(
        &mut self,
        position: gpui_kit::Point<gpui_kit::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        // The existing local popup owns retained focus, scrolling and gpui-base item behavior.
        // Its immutable entry map keeps keyboard selection tied to this menu's session snapshot.
        let mut entries = self.run_controls.menu_entries();
        // Replacement remains an explicit command without crowding the four title-bar actions.
        let can_rerun = self.run_controls.selected().is_some_and(|config| {
            self.run_controls.running_for(&config.id).is_some()
                || self.run_controls.is_pending(&config.id)
                || self.run_controls.debug_target_active(&config.id)
        });
        entries.push(RunMenuEntry::Separator);
        entries.push(RunMenuEntry::Action {
            id: "run-rerun".into(),
            label: t!("run.rerun").into(),
            enabled: can_rerun,
        });
        // Force stays reachable for a debugger whose pending disconnect has no ordinary execution state.
        entries.push(RunMenuEntry::Action {
            id: "run-force".into(),
            label: t!("run.force").into(),
            enabled: can_rerun,
        });
        let mut items = Vec::new();
        let mut actions = std::collections::BTreeMap::new();
        let mut separator = false;
        for (index, entry) in entries.into_iter().enumerate() {
            let (label, disabled) = match &entry {
                RunMenuEntry::Separator => {
                    separator = true;
                    continue;
                }
                RunMenuEntry::Target {
                    label, target_type, ..
                } => (format!("{label} · {target_type}"), false),
                RunMenuEntry::Session { label, .. } | RunMenuEntry::Configuration { label, .. } => {
                    (label.clone(), false)
                }
                RunMenuEntry::Action { label, enabled, .. } => (label.clone(), !enabled),
            };
            let id = format!("run-item-{index}");
            items.push(plugin_runtime::plugin_protocol::ui::MenuItem {
                id: id.clone(),
                label,
                disabled,
                separator_before: separator,
            });
            separator = false;
            actions.insert(id, entry);
        }
        let style = MenuStyle::current(cx);
        let popup = cx.new(|cx| {
            NativePopupMenu::new(
                items,
                style,
                position,
                move |action, window, cx| {
                    let _ = owner.update(cx, |app, cx| {
                        let plugin_runtime::plugin_protocol::ui::Action::Select(id) = action else {
                            return;
                        };
                        let Some(entry) = actions.get(&id) else {
                            return;
                        };
                        match entry {
                            RunMenuEntry::Target { id, label, .. } => {
                                let workspace = app.workspace_key();
                                match app.run_controls.confirm_target(id, &workspace) {
                                    Ok(stored) => {
                                        app.status =
                                            t!("run.target_added", name = label.clone()).into();
                                        app.open_run_config_dialog(window, cx, Some(stored));
                                    }
                                    Err(message) => app.status = message,
                                }
                            }
                            RunMenuEntry::Session { id, .. } => {
                                if let Some(config) = app
                                    .run_controls
                                    .sessions()
                                    .into_iter()
                                    .find(|session| session.id == *id)
                                    .map(|session| session.config)
                                {
                                    app.reveal_run_session(*id, &config, window, cx);
                                }
                            }
                            RunMenuEntry::Configuration { id, .. } => {
                                let key = app.workspace_key();
                                app.run_controls.select(id, &key);
                                cx.notify();
                            }
                            RunMenuEntry::Action {
                                id, enabled: true, ..
                            } => match id.as_str() {
                                "run-edit" => {
                                    let editing =
                                        app.run_controls.selected().map(|config| config.id.clone());
                                    app.open_run_config_dialog(window, cx, editing);
                                }
                                id if id.starts_with("run-preparation-") => {
                                    if let Ok(request) =
                                        id.trim_start_matches("run-preparation-").parse()
                                    {
                                        app.run_controls.show_preparation_output(request);
                                        cx.notify();
                                    }
                                }
                                "run-new" => app.open_run_config_dialog(window, cx, None),
                                "run-discover" => app.discover_run_targets(cx),
                                "run-rerun" => app.rerun_selected(window, cx),
                                "run-force" => app.terminate_selected_run(cx),
                                other => {
                                    if let Some(binding) = other.strip_prefix("run-rebind-") {
                                        if let Ok((config, target)) =
                                            serde_json::from_str::<(String, String)>(binding)
                                        {
                                            let workspace = app.workspace_key();
                                            app.status = match app
                                                .run_controls
                                                .repair_target_with(&config, &target, &workspace)
                                            {
                                                Ok(()) => t!("run.target_repaired").into(),
                                                Err(error) => error,
                                            };
                                        }
                                    } else if let Some(id) = other.strip_prefix("run-apply-repair-")
                                    {
                                        let workspace = app.workspace_key();
                                        app.status =
                                            match app.run_controls.repair_target(id, &workspace) {
                                                Ok(()) => t!("run.target_repaired").into(),
                                                Err(message) => message,
                                            };
                                    } else if let Some(id) = other.strip_prefix("run-repair-") {
                                        app.discover_run_targets(cx);
                                        app.status = if app.run_controls.target_missing(id) {
                                            t!("run.target_missing").into()
                                        } else {
                                            t!("run.target_found_confirm").into()
                                        };
                                    }
                                    cx.notify();
                                }
                            },
                            _ => {}
                        }
                    });
                },
                window,
                cx,
            )
            .width(RUN_MENU_WIDTH)
        });
        let popup_id = popup.entity_id();
        let dismiss = cx.subscribe(&popup, move |this, _, _: &DismissEvent, cx| {
            // A delayed dismissal from an older menu must not close a newly opened one.
            if this
                .run_menu
                .as_ref()
                .is_some_and(|menu| menu.popup.entity_id() == popup_id)
            {
                this.run_menu = None;
                cx.notify();
            }
        });
        popup.focus_handle(cx).focus(window, cx);
        self.run_menu = Some(RunMenu {
            popup,
            _dismiss: dismiss,
        });
        cx.notify();
    }

    /// The anchored dropdown overlay; it consumes outside presses and owns no modal window.
    pub(crate) fn render_run_menu(
        &self,
        _window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(menu) = &self.run_menu else {
            return div().into_any_element();
        };
        div()
            .id("run-menu-overlay")
            .debug_selector(|| "run-menu".into())
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.run_menu = None;
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .child(menu.popup.clone())
            .into_any_element()
    }

    /// Answer the window's close request.
    ///
    /// Returning `false` keeps the window open. With managed work in flight the user is asked first,
    /// so an active program is never discarded by a stray close; a confirmed leave stops every
    /// session through its own provider and then shuts the host down.
    pub(crate) fn should_close_window(&mut self, cx: &mut Context<Self>) -> bool {
        if self.leave_confirmed {
            self.shutdown_plugins(cx);
            return true;
        }
        if !self.run_controls.has_work_in_flight() {
            self.shutdown_plugins(cx);
            return true;
        }
        if self.leave_confirm.is_none() {
            self.leave_confirm = Some(self.run_controls.active_session_ids());
        }
        cx.notify();
        false
    }

    /// Leave after stopping every run session through the provider that owns it.
    pub(crate) fn confirm_leave(&mut self, cx: &mut Context<Self>) {
        // A preparation is asked to stop as a sequence, not as a program: its step may not have a
        // session yet, and one asked to stop must not be followed by the program it was preparing.
        let mut already_stopped = Vec::new();
        // A provider build has no interactive execution identity, but owns real native work.
        for request in self.run_controls.active_provider_preparations() {
            let _ = self.extensions.read(cx).stage_host_run(Work::CancelTarget {
                request,
                mode: plugin_runtime::plugin_protocol::process::ExitMode::Graceful,
            });
        }
        for (config, session) in self.run_controls.stop_preparations() {
            let Some(session) = session else {
                continue;
            };
            already_stopped.push(session);
            let request_id = self.run_controls.begin_stop(&config, session);
            let _ = self.extensions.read(cx).stage_host_run(Work::StopRun {
                session,
                config,
                mode: plugin_runtime::plugin_protocol::process::ExitMode::Graceful,
                request_id,
            });
        }
        // Stopping is requested per session; the window does not wait for each answer, because a
        // provider that has already exited cannot answer and the host is leaving anyway. A session
        // already asked to stop is not asked again — by a preparation above, or by an earlier Stop —
        // because two requests for one program are two answers for one stop, and the provider was
        // told once.
        for session in self.run_controls.active_sessions() {
            if already_stopped.contains(&session.id)
                || self.run_controls.is_stopping(&session.config)
            {
                continue;
            }
            let request_id = self.run_controls.begin_stop(&session.config, session.id);
            let _ = self.extensions.read(cx).stage_host_run(Work::StopRun {
                session: session.id,
                config: session.config.clone(),
                mode: plugin_runtime::plugin_protocol::process::ExitMode::Graceful,
                request_id,
            });
        }
        self.leave_confirmed = true;
        self.leave_confirm = None;
        cx.notify();
        // The close is attempted again now that the decision is recorded.
        if let Some(window) = self.main_window {
            let _ = window.update(cx, |_, window, _| window.remove_window());
        }
    }

    /// Keep the current project open and the sessions running.
    pub(crate) fn cancel_leave(&mut self, cx: &mut Context<Self>) {
        // Sessions and plugins are untouched: cancelling means continuing exactly as before.
        self.leave_confirm = None;
        self.leave_confirmed = false;
        cx.notify();
    }

    /// The leave confirmation card, shown above the shell while a decision is pending.
    pub(crate) fn render_leave_confirmation(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let sessions = self.leave_confirm.as_ref()?;
        let _ = sessions;
        let count = self.run_controls.active_work_count();
        Some(
            div()
                .debug_selector(|| "run-leave-confirm".into())
                .absolute()
                .left(px(0.))
                .top(px(0.))
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                // The card blocks the shell until the user decides, but owns no modal window.
                .bg(gpui_kit::Hsla {
                    a: 0.35,
                    ..cx.theme().background
                })
                .child(
                    v_flex()
                        .id("run-leave-confirm-card")
                        .w(px(420.))
                        .gap_3()
                        .p_4()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().popover)
                        .text_color(cx.theme().popover_foreground)
                        .shadow_lg()
                        .child(
                            div()
                                .font_semibold()
                                .child(t!("run.leave_title").to_string()),
                        )
                        .child(div().child(t!("run.leave_body", count = count).to_string()))
                        .child(
                            h_flex()
                                .gap_2()
                                .justify_end()
                                .child(
                                    div()
                                        .id("run-leave-cancel")
                                        .debug_selector(|| "run-leave-cancel".into())
                                        .child(t!("run.form_cancel").to_string())
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.cancel_leave(cx)),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("run-leave-confirm-accept")
                                        .debug_selector(|| "run-leave-confirm-accept".into())
                                        .child(t!("run.leave_confirm").to_string())
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.confirm_leave(cx)),
                                        ),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }

    /// The workspace key used for host-local configuration storage.
    pub(crate) fn workspace_key(&self) -> String {
        self.workspace.root().display().to_string()
    }

    /// The title-bar run group: a uniform dropdown, build, run, debug and stop, then a short rule.
    pub(crate) fn render_run_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.run_controls.selected().cloned();
        // A session for the selected configuration decides whether Stop is meaningful.
        let active = selected
            .as_ref()
            .and_then(|config| self.run_controls.running_for(&config.id));
        let pending = selected
            .as_ref()
            .is_some_and(|config| self.run_controls.is_pending(&config.id));
        let permitted = self.run_permitted(cx);
        let running = active.is_some()
            || pending
            || selected
                .as_ref()
                .is_some_and(|config| self.run_controls.debug_target_active(&config.id));
        // Build is about this configuration's own build actions, so its control reports the reason it
        // cannot run rather than being permanently unavailable.
        let build_blocker = selected.as_ref().and_then(|config| {
            if !permitted {
                return Some(t!("run.restricted").to_string().to_owned());
            }
            if self.run_controls.is_preparing(&config.id) {
                return Some(t!("run.preparing_busy").to_string().to_owned());
            }
            self.run_controls
                .preparation_error(&config.id)
                .filter(|_| config.build.is_empty())
        });
        let preparing = selected
            .as_ref()
            .and_then(|config| self.run_controls.preparing_step(&config.id));
        let label = selected
            .as_ref()
            .map(|config| {
                if let Some(step) = &preparing {
                    format!("{} · {}", config.name, step)
                } else if pending {
                    format!("{} · {}", config.name, t!("run.state_starting"))
                } else if active.is_some() {
                    format!("{} · {}", config.name, t!("run.state_running"))
                } else {
                    config.name.clone()
                }
            })
            .unwrap_or_else(|| t!("run.configurations").to_string().to_string());
        let state = active
            .as_ref()
            .map(|session| session.state)
            .or(pending.then_some(plugin_runtime::ExecutionState::Starting));
        // During graceful stopping the same slot exposes the explicit immediate-termination path.
        let stopping = selected
            .as_ref()
            .is_some_and(|config| self.run_controls.is_stopping(&config.id));

        h_flex()
            .debug_selector(|| "run-controls".into())
            .items_center()
            .gap_1()
            .child(
                div().debug_selector(|| "run-config-selector".into()).child(
                    Button::new("run-config-select")
                        .label(short_label(&label))
                        .child(Icon::new(IconName::ChevronDown))
                        .accessibility_label(label.clone())
                        .small()
                        .compact()
                        .ghost()
                        .border_1()
                        .border_color(cx.theme().input)
                        .tooltip(label.clone())
                        .on_click(
                            cx.listener(|this, event: &gpui_kit::ClickEvent, window, cx| {
                                // The dropdown opens at the pointer, like every other menu in the shell.
                                this.open_run_menu(event.position(), window, cx);
                            }),
                        ),
                ),
            )
            .child(
                div().debug_selector(|| "run-build".into()).child(
                    Button::new("run-build-action")
                        .icon(Icon::default().path("icons/run-build.svg"))
                        .accessibility_label(t!("run.form_build"))
                        .small()
                        .compact()
                        .ghost()
                        // Build runs the configuration's own build actions and nothing else: no
                        // pre-launch step, no program. A configuration without build actions keeps
                        // the control disabled with the reason it is disabled.
                        .disabled(!permitted || selected.is_none() || build_blocker.is_some())
                        .tooltip(
                            build_blocker
                                .clone()
                                .unwrap_or_else(|| t!("run.build_only_hint").to_string().into()),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.build_selected(window, cx);
                        })),
                ),
            )
            .child(
                div().debug_selector(|| "run-start".into()).child(
                    Button::new("run-start-action")
                        .icon(
                            Icon::default()
                                .path("icons/run-start.svg")
                                .text_color(cx.theme().success),
                        )
                        .accessibility_label(t!("run.start"))
                        .small()
                        .compact()
                        .ghost()
                        .disabled(!permitted || selected.is_none())
                        .tooltip(if permitted {
                            t!("run.start_hint").to_string()
                        } else {
                            t!("run.restricted").to_string()
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.start_selected_run(window, cx);
                        })),
                ),
            )
            .child(div().debug_selector(|| "run-debug".into()).child({
                // The reason is shown whether or not the control is available, and a debug click
                // that cannot proceed never falls back to running the program plainly.
                let debug_unavailable = selected.as_ref().is_none_or(|configuration| {
                    self.run_controls.debug_blocker(&configuration.id).is_some()
                });
                let debug_blocker = selected
                    .as_ref()
                    .and_then(|configuration| self.run_controls.debug_blocker(&configuration.id))
                    .or_else(|| match self.run_controls.debug_availability() {
                        Ok(provider) => Some(t!("run.debug_via", provider = provider).to_string()),
                        Err(reason) => Some(reason.to_owned()),
                    });
                Button::new("run-debug-action")
                    .icon(Icon::default().path("icons/run-debug.svg"))
                    .accessibility_label(t!("run.form_debug"))
                    .small()
                    .compact()
                    .ghost()
                    .disabled(!permitted || debug_unavailable)
                    .tooltip(
                        debug_blocker.unwrap_or_else(|| t!("run.debug_hint").to_string().into()),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.debug_selected(cx)))
            }))
            .child(
                div().debug_selector(|| "run-stop".into()).child(
                    Button::new("run-stop-action")
                        .debug_selector(move || {
                            if stopping {
                                "run-terminate"
                            } else {
                                "run-stop-action"
                            }
                            .into()
                        })
                        .icon(Icon::default().path("icons/run-stop.svg"))
                        .accessibility_label(if stopping {
                            t!("run.force")
                        } else {
                            t!("run.stop")
                        })
                        .small()
                        .compact()
                        .ghost()
                        .disabled(!running)
                        .tooltip(if stopping {
                            t!("run.force_hint")
                        } else {
                            t!("run.stop_hint")
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if stopping {
                                this.terminate_selected_run(cx);
                            } else {
                                this.stop_selected_run(cx);
                            }
                        })),
                ),
            )
            .child(
                // The short vertical rule keeps the run group distinct from the plugin icon.
                div()
                    .debug_selector(|| "run-controls-divider".into())
                    .w(px(1.))
                    .h(px(16.))
                    .mx_1()
                    .bg(cx.theme().border),
            )
            .when_some(state, |group, state| {
                group.child(
                    div()
                        .debug_selector(|| "run-session-state".into())
                        .text_xs()
                        .child(run_state_label(state)),
                )
            })
    }

    /// Start the selected configuration, or locate the session it already has.
    pub(crate) fn start_selected_run(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(config) = self.run_controls.selected().cloned() else {
            // With nothing selected the Run control opens the configuration list instead of guessing.
            self.open_run_config_dialog(window, cx, None);
            return;
        };
        self.start_configuration_without_environment(&config.id, window, cx);
    }

    /// Start one stored configuration, with no environment entries.
    ///
    /// Kept beside the environment-aware entry point so a caller that has no environment cannot
    /// accidentally supply one, and so the launch path stays one function.
    pub(crate) fn start_configuration_without_environment(
        &mut self,
        config_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_configuration(config_id, Vec::new(), window, cx);
    }

    /// Start one stored configuration with explicit environment entries.
    pub(crate) fn start_configuration(
        &mut self,
        config_id: &str,
        env: Vec<plugin_runtime::RunEnvEntry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Run on an existing debug target locates that same target instead of spawning a second one.
        if self.run_controls.debug_target_active(config_id) {
            self.run_controls.select_debug_session(config_id);
            self.debug_panel.open = true;
            cx.notify();
            return;
        }
        let Some(config) = self
            .run_controls
            .configurations()
            .iter()
            .find(|config| config.id == config_id)
            .cloned()
        else {
            self.status = t!("run.missing_configuration").to_string().into();
            cx.notify();
            return;
        };
        if !self.run_permitted(cx) {
            self.status = t!("run.restricted").to_string().into();
            cx.notify();
            return;
        }
        let root = self.workspace_key();
        match self.run_controls.plan_launch(&config.id, &root) {
            LaunchPlan::Existing { session } => {
                // A repeat launch reveals the running session rather than starting a second program.
                self.reveal_run_session(session, &config.id, window, cx);
            }
            LaunchPlan::Invalid { message } => {
                self.status = message;
                cx.notify();
            }
            LaunchPlan::Start { .. } => {
                let plan = match self.run_controls.launch_plan(&config.id, &root) {
                    Ok(plan) => plan,
                    Err(message) => {
                        self.status = message;
                        cx.notify();
                        return;
                    }
                };
                // Only a preparation that was already accepted may save and start, so a refused plan
                // never prompts for a save it cannot use.
                if !self.save_dirty_documents(cx) {
                    // A failed or unconfirmed save must not be followed by a launch of stale code.
                    return;
                }
                let request_id = self.run_controls.begin(&config.id);

                self.run_controls
                    .begin_sequence(&config.id, plan, request_id);
                // Extra entries belong to this launch only. The plan already carries the
                // configuration's own entries, and one of these replaces its entry of the same name.
                for entry in env {
                    self.run_controls
                        .override_program_environment(&config.id, &entry);
                }
                self.status = t!("run.preparing_named", name = config.name).to_string();
                self.drive_preparation(cx);
            }
        }
    }

    /// Apply the dialog's provider choice, so every launch path agrees with what the page shows.
    ///
    /// The runtime keeps its own versioned record of this choice, so this is the only place that has
    /// to write it. Nothing here touches a session that is already running.
    pub(crate) fn apply_provider_choice(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.run_form.as_ref() else {
            return;
        };
        let (id, provider) = {
            let form = form.read(cx);
            (form.draft.id.clone(), form.draft.provider.clone())
        };
        let workspace = self.workspace_key();
        if let Err(message) =
            self.run_controls
                .choose_provider(&id, provider.as_deref(), &workspace)
        {
            self.status = message;
            cx.notify();
            return;
        }
        let _ = self
            .extensions
            .read(cx)
            .stage_host_run(crate::extensions::HostWork::SetRunProvider { provider });
    }

    /// Ask the provider to act on the selected debug session: resume, pause or end it.
    ///
    /// The call is only sent when the control was offered, so a click can never become a method the
    /// provider did not declare. The answer is the session's new state, and it is awaited before any
    /// further action is offered.
    pub(crate) fn debug_action(&mut self, method: &str, cx: &mut Context<Self>) {
        let controls = self.run_controls.debug_controls();
        let outcome = match method {
            "resume" => &controls.resume,
            "pause" => &controls.pause,
            "stop" => &controls.stop,
            other => {
                self.status = t!("run.debug_unknown_action", action = other).to_string();
                cx.notify();
                return;
            }
        };
        if let Err(reason) = outcome {
            self.status = reason.clone();
            cx.notify();
            return;
        }
        // Resuming is the user driving the target: the next stop is not one to move the caret to.
        // Pausing is not — the target stops where it happens to be and the user wants to see it.
        if method == "resume" {
            self.run_controls.note_debug_moved_by_user();
        }
        let Some(session) = self
            .run_controls
            .debug_session()
            .map(|(_, session)| session.state().clone())
        else {
            self.status = t!("run.no_debug_session").to_string().into();
            cx.notify();
            return;
        };
        // The provider's session identity is what it answered with, never one the host invents.
        let Some(provider_session) = self.run_controls.debug_provider_session() else {
            self.status = t!("run.debug_not_connected").to_string().into();
            cx.notify();
            return;
        };
        let _ = session;
        self.run_controls.note_debug_action();
        let control = match method {
            "resume" => crate::run::DebugControl::Resume,
            "pause" => crate::run::DebugControl::Pause,
            _ => crate::run::DebugControl::Stop,
        };
        let Some(request) = self
            .run_controls
            .begin_debug_request(crate::run::DebugMethod::Control(control), None)
        else {
            self.run_controls.note_debug_action_finished();
            return;
        };
        let mut arguments = serde_json::json!({ "session": provider_session });
        if method == "resume" {
            arguments["pause"] =
                serde_json::json!(self.run_controls.debug_pause_epoch().unwrap_or(0));
        }
        if !self
            .extensions
            .read(cx)
            .stage_debug_call(request, method, arguments)
        {
            self.run_controls.note_debug_action_finished();
            self.run_controls.abandon_debug_request(request);
            self.status = t!("run.debug_worker_missing").to_string().into();
        }
        cx.notify();
    }

    /// Ask the provider to set the configuration's breakpoints, so the debugger knows where to stop.
    ///
    /// The whole set goes every time, which is how a removed breakpoint stops being set: the answer
    /// describes the positions the provider was just asked about, not a history of them. Sent while a
    /// session exists and the provider declared breakpoint support; a provider that did not is never
    /// asked, and the panel keeps saying the positions are unverified rather than implying they are in.
    fn send_debug_breakpoints(&mut self, cx: &mut Context<Self>) {
        if !self.run_controls.selected_debug_capabilities().breakpoints {
            return;
        }
        let Some(provider_session) = self.run_controls.debug_provider_session() else {
            return;
        };
        let Some(config) = self
            .run_controls
            .debug_session()
            .map(|(config, _)| config.to_owned())
        else {
            return;
        };
        let Some(configuration) = self.run_controls.configuration(&config) else {
            return;
        };
        let breakpoints = configuration
            .breakpoints
            .entries()
            .iter()
            .map(|entry| {
                serde_json::json!({
                    "source": entry.source,
                    "line": entry.line,
                })
            })
            .collect::<Vec<_>>();
        let Some(request) = self
            .run_controls
            .begin_debug_request(crate::run::DebugMethod::Breakpoints, None)
        else {
            return;
        };
        let queued = self.extensions.read(cx).stage_debug_call(
            request,
            "set_breakpoints",
            serde_json::json!({ "session": provider_session, "breakpoints": breakpoints }),
        );
        if !queued {
            self.run_controls.abandon_debug_request(request);
        }
    }

    /// Ask the provider for the stack of the pause being inspected, and for the selected frame's
    /// variables.
    ///
    /// Two requests are staged at most, and only when the provider declared both abilities and the
    /// target is stopped in a described pause: a session that cannot report frames is not asked, and a
    /// pause that has not begun has nothing to describe. Each request carries the pause it belongs to,
    /// so an answer that arrives after the target moved on is refused instead of showing a stack from a
    /// moment that is over. Asking twice for the same pause is left to `begin_debug_request`, which
    /// refuses the duplicate rather than sending two calls whose answers would race.
    fn fetch_debug_inspection(&mut self, cx: &mut Context<Self>) {
        if self.run_controls.debug_action_pending() {
            return;
        }
        if !matches!(
            self.run_controls.debug_state(),
            editor_core::DebugSessionState::Paused { .. }
        ) {
            return;
        }
        let Some(provider_session) = self.run_controls.debug_provider_session() else {
            return;
        };
        let mut staged = Vec::new();
        if self.run_controls.selected_debug_capabilities().inspect
            && !self
                .run_controls
                .debug_session()
                .is_some_and(|(_, session)| session.pause().frames_described())
            && let Some(request) = self
                .run_controls
                .begin_debug_request(crate::run::DebugMethod::Frames, None)
        {
            staged.push((
                request,
                "frames",
                serde_json::json!({ "session": provider_session, "pause":self.run_controls.debug_pause_epoch().unwrap_or(0) }),
            ));
        }
        // The frame list is a new pause's description, so it carries the first frame's scope with it.
        if let Some(frame) = self.run_controls.selected_debug_frame() {
            self.stage_variables(cx, &provider_session, frame, &mut staged);
        }
        self.stage_debug_requests(cx, staged);
    }

    /// Ask for the selected frame's variables, which is a different question once the frame changes.
    ///
    /// Selecting a frame changes the pause's scope without changing which pause it is, so nothing else
    /// would ask again: the panel would keep showing the previous frame's variables until some other
    /// update happened to pass through. The request is scoped to the frame, so a second selection makes
    /// the first answer stale rather than the two racing.
    fn fetch_debug_frame_variables(&mut self, cx: &mut Context<Self>) {
        if !self.run_controls.selected_debug_capabilities().inspect {
            return;
        }
        let Some(provider_session) = self.run_controls.debug_provider_session() else {
            return;
        };
        let Some(frame) = self.run_controls.selected_debug_frame() else {
            return;
        };
        let mut staged = Vec::new();
        self.stage_variables(cx, &provider_session, frame, &mut staged);
        self.stage_debug_requests(cx, staged);
    }

    /// Build the variables request for one frame, when the provider declared inspection and the
    /// request for this pause and frame has not already been made.
    fn stage_variables(
        &mut self,
        _cx: &mut Context<Self>,
        provider_session: &str,
        frame: u32,
        staged: &mut Vec<(u64, &'static str, serde_json::Value)>,
    ) {
        if self.run_controls.debug_action_pending()
            || !self.run_controls.selected_debug_capabilities().inspect
            || self
                .run_controls
                .debug_session()
                .is_some_and(|(_, session)| session.pause().variables_described(frame))
        {
            return;
        }
        if let Some(request) = self
            .run_controls
            .begin_debug_request(crate::run::DebugMethod::Variables, Some(frame))
        {
            staged.push((
                request,
                "variables",
                serde_json::json!({ "session": provider_session, "pause":self.run_controls.debug_pause_epoch().unwrap_or(0), "frame": frame }),
            ));
        }
    }

    /// Send the staged debug calls, releasing any the worker refused rather than awaiting it forever.
    fn stage_debug_requests(
        &mut self,
        cx: &mut Context<Self>,
        staged: Vec<(u64, &'static str, serde_json::Value)>,
    ) {
        for (request, method, arguments) in staged {
            if !self
                .extensions
                .read(cx)
                .stage_debug_call(request, method, arguments)
            {
                self.run_controls.abandon_debug_request(request);
            }
        }
    }

    /// Ask the provider to step the paused target, and remember which pause the answer describes.
    ///
    /// The request is only sent when the provider declared the ability and the target is stopped, so a
    /// click can never become a call the provider did not offer. The step is answered by a state: the
    /// editor waits for it rather than describing a pause the target has not reached.
    pub(crate) fn step_debug(&mut self, kind: editor_core::DebugStep, cx: &mut Context<Self>) {
        if let Err(reason) = self
            .run_controls
            .debug_controls()
            .step
            .into_iter()
            .find(|(candidate, _)| *candidate == kind)
            .map(|(_, outcome)| outcome)
            .unwrap_or_else(|| Err(t!("run.step_unsupported").to_string().into()))
        {
            self.status = reason;
            cx.notify();
            return;
        }
        // The user is driving the target, so the pause this produces is not one to chase.
        self.run_controls.note_debug_moved_by_user();
        let Some(request) = self
            .run_controls
            .begin_debug_request(crate::run::DebugMethod::Step(kind), None)
        else {
            self.status = t!("run.step_pending").to_string().into();
            cx.notify();
            return;
        };
        let Some(session) = self.run_controls.debug_provider_session() else {
            self.run_controls.abandon_debug_request(request);
            return;
        };
        self.run_controls.note_debug_action();
        if !self.extensions.read(cx).stage_debug_call(
            request,
            "step",
            serde_json::json!({ "session": session, "pause":self.run_controls.debug_pause_epoch().unwrap_or(0), "kind": kind.as_str() }),
        ) {
            self.run_controls.abandon_debug_request(request);
            self.run_controls.note_debug_action_finished();
            self.status = t!("run.step_worker_missing").to_string().into();
        }
        cx.notify();
    }

    /// Refuse or begin a debug launch, never substituting an ordinary run.
    ///
    /// The refusal is the point of this method: a debug click that cannot be honoured has to say so,
    /// because running the program without a debugger would look like success while handing the user
    /// something they did not ask for.
    pub(crate) fn debug_selected(&mut self, cx: &mut Context<Self>) {
        let Some(configuration) = self.run_controls.selected().map(|config| config.id.clone())
        else {
            self.status = t!("run.no_configuration").into();
            cx.notify();
            return;
        };
        self.debug_configuration(&configuration, cx);
    }

    /// Both an explicit Debug and a replacement use the captured owner, even after selection changes.
    fn debug_configuration(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(configuration) = self.run_controls.configuration(id).cloned() else {
            self.status = t!("run.no_configuration").into();
            cx.notify();
            return;
        };
        if !self.run_permitted(cx) {
            self.status = t!("run.restricted").into();
            cx.notify();
            return;
        }
        if self
            .run_controls
            .debug_session_of(&configuration.id)
            .is_some_and(|session| {
                matches!(
                    session.state(),
                    editor_core::DebugSessionState::Starting
                        | editor_core::DebugSessionState::Running
                        | editor_core::DebugSessionState::Paused { .. }
                )
            })
        {
            self.run_controls.select_debug_session(&configuration.id);
            self.debug_panel.open = true;
            cx.notify();
            return;
        }
        if self
            .run_controls
            .preparation(&configuration.id)
            .is_some_and(|sequence| sequence.is_active())
        {
            return;
        }
        if self.run_controls.running_for(&configuration.id).is_some() {
            self.status = t!("run.stop_before_debug").into();
            cx.notify();
            return;
        }
        if let Some(reason) = self.run_controls.debug_blocker(&configuration.id) {
            self.status = reason;
            cx.notify();
            return;
        }
        let workspace = self.workspace_key();
        let plan = match self.run_controls.launch_plan(&configuration.id, &workspace) {
            Ok(plan) => plan,
            Err(reason) => {
                self.status = reason;
                cx.notify();
                return;
            }
        };
        // Saving and preparation are identical to Run; only the final target crosses debug.session.
        if !self.save_dirty_documents(cx) {
            return;
        }
        match self
            .run_controls
            .begin_debug_preparation(&configuration.id, plan, &workspace)
        {
            Ok(()) => {
                self.debug_panel.open = true;
                self.status = t!("run.preparing_debug", name = configuration.name.clone()).into();
                self.drive_preparation(cx);
            }
            Err(reason) => {
                self.status = reason;
                cx.notify();
            }
        }
    }

    /// Start the frozen final target once, only after every build/prelaunch exit was actually zero.
    fn start_prepared_debug(
        &mut self,
        configuration: &str,
        details: serde_json::Value,
        cx: &mut Context<Self>,
    ) {
        self.run_controls.note_debug_session_begun(configuration);
        self.run_controls.begin_debug_session(configuration);
        if let Ok(provider) = self.run_controls.debug_availability().map(str::to_owned) {
            self.run_controls
                .note_debug_provider_owner(configuration, &provider);
        }
        if self
            .run_controls
            .selected()
            .is_some_and(|selected| selected.id == configuration)
            // Cargo may finish after the user starts inspecting a different pause. Preserve that
            // view; the new session appears as its own panel tab and can be selected explicitly.
            && self.run_controls.debug_session().is_none_or(|(other,session)|
                other==configuration || !matches!(session.state(),editor_core::DebugSessionState::Paused {..}))
        {
            self.run_controls.select_debug_session(configuration);
        }
        let Some(request) = self.run_controls.begin_debug_start_request(configuration) else {
            return;
        };
        if !self
            .extensions
            .read(cx)
            .stage_debug_launch(request, configuration, details)
        {
            let message = t!("run.debug_unavailable").to_string();
            let _ = self.run_controls.fail_debug_reply(request, message.clone());
            self.status = message;
        } else {
            self.status = t!("run.starting_debug", name = configuration.to_owned()).into();
        }
        cx.notify();
    }

    /// Ask the installed plugins what this workspace offers, and report what changed.
    ///
    /// Discovery is a read: it starts nothing, and it never adds, renames or deletes a configuration.
    /// A target nobody claimed stays unclaimed until the user confirms it.
    pub(crate) fn discover_run_targets(&mut self, cx: &mut Context<Self>) {
        if !self.run_permitted(cx) {
            self.status = t!("run.restricted").to_string().into();
            cx.notify();
            return;
        }
        match self.run_controls.begin_discovery() {
            Ok(request) => {
                let workspace = self.workspace_key();
                self.status = if self
                    .extensions
                    .read(cx)
                    .stage_host_run(Work::DiscoverTargets { workspace, request })
                {
                    t!("run.discovering").into()
                } else {
                    t!("run.debug_unavailable").into()
                };
            }
            Err(error) => self.status = error,
        }
        cx.notify();
    }

    /// Build the selected configuration: its own build actions, then nothing else.
    ///
    /// A build never runs a pre-launch step and never starts the program, so it is safe to run while
    /// deciding whether to launch.
    pub(crate) fn build_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let _ = window;
        let Some(config) = self.run_controls.selected().cloned() else {
            return;
        };
        if !self.run_permitted(cx) {
            self.status = t!("run.restricted").to_string().into();
            cx.notify();
            return;
        }
        if let Some(reason) = self.run_controls.preparation_error(&config.id) {
            self.status = reason;
            cx.notify();
            return;
        }
        let root = self.workspace_key();
        // A build plan holds the build actions only: Build must never run a pre-launch step.
        let plan = match self
            .run_controls
            .prepare_build(&config.id, &root, MAX_PREPARED_STEPS)
        {
            Ok(plan) => plan,
            Err(message) => {
                self.status = message;
                cx.notify();
                return;
            }
        };
        if !self.save_dirty_documents(cx) {
            // A build of stale code is worse than no build: the same confirmation the run path uses.
            return;
        }
        let request_id = self.run_controls.begin(&config.id);
        self.run_controls.begin_build(&config.id, &plan, request_id);
        self.status = t!("run.building_named", name = config.name).to_string();
        self.drive_preparation(cx);
    }

    /// Advance every configuration whose preparation has something to do next.
    ///
    /// This is the only place a preparation step is requested or observed, so the order a user sees
    /// is the order the sequence decides rather than the order events happen to arrive.
    pub(crate) fn drive_preparation(&mut self, cx: &mut Context<Self>) {
        let configs = self
            .run_controls
            .configurations()
            .iter()
            .map(|config| config.id.clone())
            .collect::<Vec<_>>();
        for config in configs {
            loop {
                let action = self.run_controls.preparation_action(&config);
                match action {
                    Some(SequenceAction::Start { index }) => {
                        // A requested step is not progress yet: the sequence waits for its exit.
                        self.request_preparation_step(&config, index, cx);
                        break;
                    }
                    Some(SequenceAction::Stop { session }) => {
                        self.stop_preparation_step(&config, session, cx);
                        break;
                    }
                    Some(SequenceAction::Wait) => {
                        self.observe_preparation(&config, cx);
                        break;
                    }
                    Some(SequenceAction::Blocked { reason }) => {
                        self.run_controls.fail_prepared_debug(&config, &reason);
                        self.status = reason;
                        self.run_controls.forget_step_sessions(&config);
                        cx.notify();
                        break;
                    }
                    Some(SequenceAction::Done) => {
                        self.finish_preparation(&config, cx);
                        break;
                    }
                    None => break,
                }
            }
        }
    }

    /// Request the step at one index.
    ///
    /// Returns whether this attempt produced a request. A step that could not even be queued blocks
    /// the sequence here, so the caller stops looking for more work either way.
    fn request_preparation_step(
        &mut self,
        config: &str,
        index: usize,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(step) = self
            .run_controls
            .preparation(config)
            .and_then(|sequence| sequence.current_step())
            .map(|step| (step.name.clone(), step.kind))
        else {
            return false;
        };
        let Some(request) = self
            .run_controls
            .preparation(config)
            .and_then(|sequence| sequence.planned_request(index).cloned())
        else {
            self.run_controls
                .sequence_start_failed(config, index, &t!("run.step_no_command"));
            self.status = t!("run.preparation_no_command").to_string().into();
            cx.notify();
            return false;
        };
        let request_id = self.run_controls.begin(config);
        // The step is owned before the request is staged, so nothing can request it twice.
        self.run_controls
            .note_step_request(config, index, request_id);
        let work = if let Some((provider, binding)) = self
            .run_controls
            .preparation(config)
            .and_then(|sequence| sequence.planned_preparation(index))
            .cloned()
        {
            if step.1 == StepKind::Program {
                self.run_controls.sequence_start_failed(
                    config,
                    index,
                    "Build did not resolve this provider target",
                );
                return false;
            }
            self.run_controls
                .register_provider_preparation(config, index, request_id);
            Work::PrepareTarget {
                config: config.into(),
                index,
                request: request_id,
                provider,
                binding,
                env: request.env,
            }
        } else {
            Work::StartRun {
                request,
                config: config.into(),
                request_id,
            }
        };
        let queued = self.extensions.read(cx).stage_host_run(work);
        if !queued {
            self.run_controls
                .sequence_start_failed(config, index, &t!("run.worker_missing"));
            self.status = t!("run.preparation_worker_missing").to_string().into();
            cx.notify();
            return false;
        }

        self.status = format!("{} {}", step.1.label(), step.0);
        cx.notify();
        true
    }

    /// Stop the program one preparation step owns.
    fn stop_preparation_step(&mut self, config: &str, session: u64, cx: &mut Context<Self>) {
        // The sequence keeps asking for this stop until its provider answers, so without this guard
        // every frame asks again: one program would be told to stop repeatedly, and leaving would
        // count several stops for one session. A stop already in flight is the answer being waited for.
        if self.run_controls.is_stopping(config) {
            return;
        }
        let request_id = self.run_controls.begin_stop(config, session);
        let queued = self.extensions.read(cx).stage_host_run(Work::StopRun {
            session,
            config: config.to_owned(),
            mode: plugin_runtime::plugin_protocol::process::ExitMode::Graceful,
            request_id,
        });
        if !queued {
            // The provider is unreachable, so the sequence cannot be told the program ended; it stays
            // owned rather than reporting a stop that did not happen.
            self.status = t!("run.stop_worker_missing").to_string().into();
        }
        cx.notify();
    }

    /// Ask this preparation's provider what became of the program it started.
    fn observe_preparation(&mut self, config: &str, cx: &mut Context<Self>) {
        let Some((index, session)) = self.run_controls.preparation(config).and_then(|sequence| {
            sequence
                .current_session()
                .map(|session| (sequence.current_index(), session))
        }) else {
            return;
        };
        if self.run_controls.has_poll(config, index) {
            // One outstanding query per step: polling faster would not learn anything sooner.
            return;
        }
        let request_id = self.run_controls.begin_poll(config, index, session);
        let queued = self.extensions.read(cx).stage_host_run(Work::PollRun {
            session,
            config: config.to_owned(),
            request_id,
        });
        if !queued {
            self.run_controls.reconcile_run_status(&[(
                config.to_owned(),
                request_id,
                crate::extensions::RunStatus::Unknown,
            )]);
            self.status = t!("run.status_worker_missing").to_string().into();
            cx.notify();
        }
    }

    /// Report a preparation that finished, whether it ended in a launch or a completed build.
    fn finish_preparation(&mut self, config: &str, cx: &mut Context<Self>) {
        if let Some(details) = self.run_controls.take_prepared_debug(config) {
            self.start_prepared_debug(config, details, cx);
            return;
        }
        let name = self
            .run_controls
            .configuration(config)
            .map(|stored| stored.name.clone())
            .unwrap_or_else(|| config.to_owned());
        self.run_controls.forget_step_sessions(config);
        self.status = t!("run.built_named", name = name).to_string();
        cx.notify();
    }

    /// Start a stored configuration exactly as its own target describes it.
    ///
    /// Kept for checks that need the request the launch path builds rather than a hand-assembled
    /// equivalent. Nothing in the application calls it — the title bar goes through the preparation
    /// sequence, which also owns the step it starts — so it is compiled only for tests instead of
    /// sitting in the shipping binary as an unused second way to start a program.
    #[cfg(test)]
    pub(crate) fn start_stored_configuration(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = window;
        let Some(config) = self.run_controls.selected().cloned() else {
            return;
        };
        let root = self.workspace_key();
        let plan = self.run_controls.plan_launch(&config.id, &root);
        let Some(request) = RunControls::request_for(&plan) else {
            return;
        };
        // The request is staged here rather than through the sequence, because this entry point has no
        // step to own: it exists so a check can start a stored configuration exactly as the launch path
        // builds it, without the title bar's preparation in between.
        let request_id = self.run_controls.begin(&config.id);
        let queued = self.extensions.read(cx).stage_host_run(Work::StartRun {
            request,
            config: config.id.clone(),
            request_id,
        });
        self.status = if queued {
            t!("run.starting_named", name = config.name).to_string()
        } else {
            t!("run.start_worker_missing").to_string().into()
        };
        cx.notify();
    }

    /// Select a configuration and ask its pinned provider to reveal the session's own retained view.
    pub(crate) fn reveal_run_session(
        &mut self,
        session: u64,
        config: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(_found) = self
            .run_controls
            .sessions()
            .into_iter()
            .find(|candidate| candidate.id == session)
        else {
            return;
        };
        // Selection follows the session's own configuration so Stop affects exactly this session.
        let key = self.workspace_key();
        self.run_controls.select(config, &key);
        let request = self.run_controls.begin_location(session, config);
        self.status = if self
            .extensions
            .read(cx)
            .stage_host_run(Work::LocateRun { session, request })
        {
            t!("run.locating", session = session.to_string()).into()
        } else {
            self.run_controls.finish_location(session, request);
            t!("run.locate_unavailable").into()
        };
        cx.notify();
    }

    /// Request normal exit through the selected session's original owner.
    pub(crate) fn stop_selected_run(&mut self, cx: &mut Context<Self>) {
        self.request_selected_stop(
            plugin_runtime::plugin_protocol::process::ExitMode::Graceful,
            cx,
        );
    }

    /// Explicit force bypasses normal cleanup and its grace period, retaining ownership until exit.
    pub(crate) fn terminate_selected_run(&mut self, cx: &mut Context<Self>) {
        self.request_selected_stop(
            plugin_runtime::plugin_protocol::process::ExitMode::Force,
            cx,
        );
    }

    /// Both controls seal the preparation before sending effects to its owned program.
    fn request_selected_stop(
        &mut self,
        mode: plugin_runtime::plugin_protocol::process::ExitMode,
        cx: &mut Context<Self>,
    ) {
        let Some(config) = self.run_controls.selected().cloned() else {
            return;
        };
        if let Some(request) = self.run_controls.provider_preparation_request(&config.id) {
            self.run_controls.request_configuration_stop(&config.id);
            self.run_controls.cancel_rerun(&config.id);
            self.extensions
                .read(cx)
                .stage_host_run(Work::CancelTarget { request, mode });
            self.drive_preparation(cx);
            cx.notify();
            return;
        }
        // Debug may still be building through ordinary execution; stop that sequence before the adapter exists.
        if self.run_controls.is_preparing(&config.id) {
            self.run_controls.request_configuration_stop(&config.id);
        }
        if self.run_controls.debug_target_active(&config.id) {
            self.run_controls.select_debug_session(&config.id);
            if mode == plugin_runtime::plugin_protocol::process::ExitMode::Force {
                self.force_debug(cx);
            } else {
                self.debug_action("stop", cx);
            }
            return;
        }
        let Some(session) = self.run_controls.running_for(&config.id) else {
            return;
        };
        self.run_controls.cancel_rerun(&config.id);
        self.run_controls.request_configuration_stop(&config.id);
        if !session.is_active() {
            self.status = t!("run.session_ended").to_string().into();
            cx.notify();
            return;
        }
        let request_id = self.run_controls.begin_stop(&config.id, session.id);
        let queued = self.extensions.read(cx).stage_host_run(Work::StopRun {
            session: session.id,
            config: config.id.clone(),
            mode,
            request_id,
        });
        self.status = if queued {
            // The provider is asked, not commanded by the host; the answer arrives in its own time.
            t!("run.stopping_named", session = session.id).to_string()
        } else {
            t!("run.stop_request_worker_missing").to_string().into()
        };
        cx.notify();
    }

    /// Run the selected configuration again, replacing the instance it already has.
    ///
    /// Rerunning is an explicit action: the running instance is stopped first so two programs never
    /// overlap, and the new start follows the ordinary launch rules including save coordination.
    pub(crate) fn rerun_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(config) = self.run_controls.selected().cloned() else {
            self.open_run_config_dialog(window, cx, None);
            return;
        };
        if let Some(request) = self.run_controls.provider_preparation_request(&config.id) {
            let debug = self.run_controls.debug_target_active(&config.id);
            self.run_controls.request_configuration_stop(&config.id);
            self.run_controls
                .wait_to_prepare_again(&config.id, request, debug);
            self.extensions.read(cx).stage_host_run(Work::CancelTarget {
                request,
                mode: plugin_runtime::plugin_protocol::process::ExitMode::Graceful,
            });
            cx.notify();
            return;
        }
        if self.run_controls.debug_target_active(&config.id) {
            self.run_controls.select_debug_session(&config.id);
            self.run_controls.wait_to_debug_again(&config.id);
            self.debug_action("stop", cx);
            return;
        }
        if let Some(session) = self.run_controls.running_for(&config.id) {
            // Seal the old sequence before its clean stop can be mistaken for successful preparation.
            self.run_controls.request_configuration_stop(&config.id);
            // The stop is requested before the replacement starts, and its outcome is reported.
            let request_id = self.run_controls.begin_stop(&config.id, session.id);
            let queued = self.extensions.read(cx).stage_host_run(Work::StopRun {
                session: session.id,
                config: config.id.clone(),
                mode: plugin_runtime::plugin_protocol::process::ExitMode::Graceful,
                request_id,
            });
            if !queued {
                self.status = t!("run.rerun_worker_missing").to_string().into();
                cx.notify();
                return;
            }
            // The provider's acceptance is only a control acknowledgement. Actual exit releases
            // this barrier in sync_run_controls, which repeats validation, save and preparation.
            self.run_controls.wait_to_rerun(&config.id, session.id);
            self.status = t!("run.rerun_waiting").to_string().into();
            cx.notify();
            return;
        }
        self.start_selected_run(window, cx);
    }

    /// Open the B1 configuration dialog for a stored configuration or a new one.
    pub(crate) fn open_run_config_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        editing: Option<String>,
    ) {
        // Keep the modal inside its owning native window. UI Automation can retain a field
        // after a secondary HWND is destroyed; the same-window base Dialog revokes only nodes.
        let key = self.workspace_key();
        // Opening the dialog asks the host for its provider listings, so the debug page and the debug
        // control speak about a capability that has been confirmed rather than one assumed from the
        // absence of a refusal.
        self.extensions.read(cx).ask_run_providers();
        let editing_id = editing.clone();
        let form = cx.new(|cx| {
            RunConfigForm::open(&self.run_controls, &key, editing_id.as_deref(), window, cx)
        });
        self.run_form = Some(form);
        let owner = cx.entity().downgrade();
        let observed = cx.entity();
        self.run_dialog = Some(cx.new(|cx| {
            let focus = cx.focus_handle();
            focus.focus(window, cx);
            RunConfigModal {
                owner,
                focus,
                _updates: vec![cx.observe(&observed, |_, _, cx| cx.notify())],
                observed_form: None,
                _form_update: None,
            }
        }));
        cx.notify();
    }

    /// Mount the modal entity only after the editor's render lease is released.
    pub(crate) fn render_run_form_modal(
        &self,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> AnyElement {
        self.run_dialog
            .as_ref()
            .map(|dialog| div().child(dialog.clone()).into_any_element())
            .unwrap_or_else(|| div().into_any_element())
    }

    /// Save the dialog draft into host-local storage and close the dialog.
    pub(crate) fn commit_run_form(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.run_form.clone() else {
            return;
        };
        // The draft is read from the fields themselves, so the saved value is what is on screen.
        // A malformed environment line is reported while the form is open, never at launch time.
        let configuration = form.update(cx, |form, cx| {
            form.collect(cx);
            form.draft.to_config()
        });
        let configuration = match configuration {
            Ok(configuration) => configuration,
            Err(message) => {
                form.update(cx, |form, cx| {
                    form.error = Some(message.clone());
                    cx.notify();
                });
                self.status = message;
                cx.notify();
                return;
            }
        };
        let key = self.workspace_key();
        match self.run_controls.upsert(configuration.clone(), &key) {
            Ok(()) => {
                // Saving never starts the program; the target merely becomes the selected one.
                self.run_controls.select(&configuration.id, &key);
                self.status = t!("run.saved_named", name = configuration.name).to_string();
                self.close_run_form(cx);
            }
            Err(message) => {
                // The rejected draft stays open so the offending field can be corrected.
                form.update(cx, |form, cx| {
                    form.error = Some(message.clone());
                    cx.notify();
                });
                self.status = message;
            }
        }
        cx.notify();
    }

    /// Close the configuration dialog without saving anything.
    pub(crate) fn close_run_form(&mut self, cx: &mut Context<Self>) {
        // Removing the modal nodes keeps the native HWND alive, including for IME/UIA clients.
        self.run_dialog = None;
        self.run_form = None;
        cx.notify();
    }

    /// Save every modified document before a launch, reporting a refusal instead of launching.
    ///
    /// A document whose disk copy changed underneath the editor, or whose file disappeared, needs an
    /// explicit overwrite confirmation. Launching would run the older file, so the launch stops here.
    pub(crate) fn save_dirty_documents(&mut self, cx: &mut Context<Self>) -> bool {
        let dirty = (0..self.tabs.len())
            .filter(|index| {
                let tab = &self.tabs[*index];
                tab.session.is_dirty() || tab.disk_state != crate::DiskState::Synced
            })
            .collect::<Vec<_>>();
        for index in dirty {
            if self.tabs[index].disk_state != crate::DiskState::Synced
                && !self.tabs[index].overwrite_confirmed
            {
                // The ordinary save path asks for confirmation; the launch waits for that answer.
                self.status = t!("status.confirm_disk_overwrite").to_string();
                cx.notify();
                return false;
            }
            let path = self.tabs[index].session.path().to_path_buf();
            self.save_document_at(index, cx);
            if self.tabs[index].session.is_dirty() {
                // A save that did not take effect must not be treated as a successful preparation.
                self.status = t!("run.save_failed_named", path = path.display()).to_string();
                cx.notify();
                return false;
            }
        }
        true
    }

    /// Save one open document by index, using the same session and file store as a manual save.
    fn save_document_at(&mut self, index: usize, cx: &mut Context<Self>) {
        let value = self.tabs[index].editor.read(cx).value().to_string();
        if let Some(history) = &self.history {
            let _ = history.snapshot_file(self.tabs[index].session.path());
        }
        let tab = &mut self.tabs[index];
        match tab.session.save(&self.file_store, &value) {
            Ok(()) => {
                tab.disk_digest = Sha256::digest(value.as_bytes()).into();
                tab.last_saved_at = std::time::Instant::now();
                tab.disk_state = crate::DiskState::Synced;
                tab.overwrite_confirmed = false;
                let path = tab.session.path().to_path_buf();
                self.notify_language_document_saved(&path, value, cx);
            }
            Err(error) => {
                self.status = t!("status.save_failed", error = error.to_string()).to_string();
            }
        }
    }
}

/// A short button label keeps the title bar compact when a configuration name is long.
fn short_label(label: &str) -> String {
    let mut text = label.chars().take(18).collect::<String>();
    if label.chars().count() > 18 {
        text.push('…');
    }
    text
}

/// A visible state word for one session; the host never invents a stronger claim than the provider's.
fn run_state_label(state: plugin_runtime::ExecutionState) -> String {
    match state {
        plugin_runtime::ExecutionState::Starting => t!("run.state_starting").to_string().into(),
        plugin_runtime::ExecutionState::Running => t!("run.state_running").to_string().into(),
        plugin_runtime::ExecutionState::Stopping => t!("run.state_stopping").to_string().into(),
        plugin_runtime::ExecutionState::Terminating => {
            t!("run.state_terminating").to_string().into()
        }
        plugin_runtime::ExecutionState::Failed | plugin_runtime::ExecutionState::Exited => {
            t!("run.state_ended").to_string().into()
        }
    }
}

/// Render the B1 form: configuration entry points, four tabs, fields and the footer.
/// Render one prepared-action list: a row per action with its own move and remove controls, then an
/// add control for the next action.
///
/// Each row is an ordinary single-line field, so a row edit never rewrites another row's text.
fn render_step_rows(
    app: &Entity<EditorApp>,
    field: RunField,
    shell: bool,
    cx: &mut gpui_kit::App,
) -> AnyElement {
    let Some(form) = app.read(cx).run_form.clone() else {
        return div().into_any_element();
    };
    let rows = form.read(cx).rows_of(field);
    let count = rows.len();
    let mut list = v_flex().gap_1();
    for (index, input) in rows {
        let owner = app.clone();
        list = list.child(
            h_flex()
                .debug_selector(move || format!("{}-row-{index}", field.selector()))
                .gap_1()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .child(crate::ui::controls::Input::new(&input)),
                )
                .child(step_control(
                    &owner,
                    field,
                    StepEdit::Up,
                    index,
                    index > 0,
                    shell,
                ))
                .child(step_control(
                    &owner,
                    field,
                    StepEdit::Down,
                    index,
                    index + 1 < count,
                    shell,
                ))
                .child(step_control(
                    &owner,
                    field,
                    StepEdit::Remove,
                    index,
                    true,
                    shell,
                )),
        );
    }
    list.child(step_control(app, field, StepEdit::Add, count, true, shell))
        .into_any_element()
}

/// One structural control for a prepared-action list.
///
/// A control that cannot act — moving the first row up, for example — is disabled rather than
/// silently doing nothing, and each carries the row it addresses.
fn step_control(
    app: &Entity<EditorApp>,
    field: RunField,
    edit: StepEdit,
    index: usize,
    enabled: bool,
    shell: bool,
) -> AnyElement {
    let (label, hint) = match edit {
        StepEdit::Add => (
            t!("run.step_add").to_string(),
            t!("run.step_add_hint").to_string(),
        ),
        StepEdit::Remove => (
            t!("run.step_remove").to_string(),
            t!("run.step_remove_hint").to_string(),
        ),
        StepEdit::Up => (
            t!("run.step_up").to_string(),
            t!("run.step_up_hint").to_string(),
        ),
        // A pre-launch step may require another configuration's build by naming it after `@`.
        StepEdit::Down => (
            t!("run.step_down").to_string(),
            t!("run.step_down_hint").to_string(),
        ),
    };
    let _ = shell;
    let owner = app.clone();
    let edit_key = match edit {
        StepEdit::Add => "add",
        StepEdit::Remove => "remove",
        StepEdit::Up => "up",
        StepEdit::Down => "down",
    };
    let id = format!("{}-{edit_key}-{index}", field.selector());
    let selector = id.clone();
    div()
        .debug_selector(move || selector.clone())
        .child(
            Button::new(id)
                .label(label)
                .small()
                .compact()
                .ghost()
                .disabled(!enabled)
                .tooltip(hint)
                .on_click(move |_, window, cx| {
                    owner.update(cx, |state, cx| {
                        let Some(form) = state.run_form.clone() else {
                            return;
                        };
                        form.update(cx, |form, cx| {
                            form.edit_rows(field, edit, index, window, cx);
                        });
                        cx.notify();
                    });
                }),
        )
        .into_any_element()
}

/// One destination choice for the configuration dialog's footer.
///
/// The two choices are explicit rather than a checkbox, because they mean different things: one keeps
/// the configuration on this machine, the other puts its portable half in a file the whole project
/// reads. The chosen one is marked, so the state a save will use is visible before it happens.
fn save_destination(
    app: &Entity<EditorApp>,
    share: bool,
    selector: &'static str,
    label: String,
    to_project: bool,
) -> AnyElement {
    let selected = share == to_project;
    let owner = app.clone();
    Button::new(selector)
        .small()
        .compact()
        .ghost()
        .debug_selector(move || selector.into())
        .when(selected, |choice| choice.primary())
        .label(label)
        .on_click(move |_, _, cx| {
            owner.update(cx, |state, cx| {
                let Some(form) = state.run_form.clone() else {
                    return;
                };
                form.update(cx, |form, cx| {
                    form.draft.share = to_project;
                    form.error = None;
                    cx.notify();
                });
                cx.notify();
            });
        })
        .into_any_element()
}

/// Render bounded discovered candidates in B1 without implicitly storing or executing them.
fn render_discovered_targets(app: &Entity<EditorApp>, cx: &mut gpui_kit::App) -> AnyElement {
    let targets = app.read(cx).run_controls.discovered_targets().to_vec();
    let mut list = v_flex()
        .id("run-config-targets")
        .max_h(px(120.))
        .overflow_y_scroll()
        .gap_1();
    for target in targets {
        let owner = app.clone();
        let identity = target.id.clone();
        let selector = format!("run-config-target-{}", target.id);
        list = list.child(
            Button::new(selector.clone())
                .debug_selector(move || selector.clone())
                .label(format!(
                    "{} · {} · {}",
                    target.label, target.provider, target.found_in
                ))
                .tooltip(t!("run.confirm_candidate").to_string())
                .on_click(move |_, window, cx| {
                    owner.update(cx, |state, cx| {
                        let workspace = state.workspace_key();
                        match state.run_controls.confirm_target(&identity, &workspace) {
                            Ok(id) => {
                                // Replace editing state atomically as well as the draft. Reusing old
                                // InputState values would save the previous target's visible fields.
                                state.run_form = Some(cx.new(|cx| {
                                    RunConfigForm::open(
                                        &state.run_controls,
                                        &workspace,
                                        Some(&id),
                                        window,
                                        cx,
                                    )
                                }));
                                state.status = t!(
                                    "run.target_added",
                                    name = state.run_controls.configuration(&id).unwrap().name
                                )
                                .into();
                            }
                            Err(error) => state.status = error,
                        }
                        cx.notify();
                    });
                }),
        );
    }
    list.into_any_element()
}

pub(crate) fn render_run_config_form(
    app: &WeakEntity<EditorApp>,
    content: DialogContent,
    cx: &mut gpui_kit::App,
) -> DialogContent {
    let Some(app) = app.upgrade() else {
        return content;
    };
    let (tab, error, shell, share, texts, inputs, textareas, saved, providers, chosen_provider) = {
        let state = app.read(cx);
        let saved = state.run_controls.configurations().to_vec();
        let Some(form) = state.run_form.as_ref() else {
            return content;
        };
        let form = form.read(cx);
        (
            form.tab,
            form.error.clone(),
            form.draft.shell,
            form.draft.share,
            RunField::ALL.map(|field| {
                let text = form.text(field, cx);
                text
            }),
            RunField::ALL.map(|field| {
                form.inputs
                    .iter()
                    .find(|(candidate, _)| *candidate == field)
                    .map(|(_, input)| input.clone())
            }),
            RunField::ALL.map(|field| {
                form.textareas
                    .iter()
                    .find(|(candidate, _)| *candidate == field)
                    .map(|(_, input)| input.clone())
            }),
            saved,
            // The provider list is the runtime's answer, read through the app rather than copied into
            // the form: a listing that changed since the dialog opened is the one that is shown.
            state
                .extensions
                .read(cx)
                .run_providers()
                .unwrap_or_default(),
            form.draft.provider.clone(),
        )
    };
    let danger = cx.theme().danger;
    let border = cx.theme().border;

    let tabs = RunConfigTab::ALL
        .into_iter()
        .map(|candidate| {
            let selected = candidate == tab;
            let owner = app.clone();
            Button::new(candidate.selector())
                .debug_selector(move || candidate.selector().into())
                .small()
                .compact()
                .ghost()
                .when(selected, |tab| tab.text_color(cx.theme().primary))
                // A tab whose settings are not implemented is visibly disabled, not silently empty.
                .disabled(!candidate.available())
                .label(candidate.label())
                .on_click(move |_, _, cx| {
                    if !candidate.available() {
                        return;
                    }
                    owner.update(cx, |state, cx| {
                        if let Some(form) = state.run_form.as_ref() {
                            form.update(cx, |form, cx| {
                                form.tab = candidate;
                                cx.notify();
                            });
                        }
                        cx.notify();
                    });
                })
        })
        .collect::<Vec<_>>();

    // The environment page edits its own field; the basic page shows the launch settings.
    let page_fields: &[RunField] = if tab == RunConfigTab::Build {
        &[RunField::Build, RunField::Prelaunch]
    } else if tab == RunConfigTab::Environment {
        &[RunField::Environment, RunField::ToolPaths]
    } else if tab == RunConfigTab::Debug {
        // The debug page edits the provider choice and the breakpoint list, neither of which is one
        // of the basic page's launch fields.
        &[RunField::Breakpoints]
    } else {
        // Shell mode replaces the program field's meaning and adds the script body.
        if shell {
            &[
                RunField::Name,
                RunField::Program,
                RunField::Arguments,
                RunField::Script,
                RunField::Directory,
            ]
        } else {
            &[
                RunField::Name,
                RunField::Program,
                RunField::Arguments,
                RunField::Directory,
            ]
        }
    };
    let fields = RunField::ALL
        .into_iter()
        .zip(texts)
        .zip(inputs)
        .zip(textareas)
        .filter(|(((field, _), _), _)| page_fields.contains(field))
        .map(|(((field, text), input), textarea)| {
            // In shell mode the executable field names the interpreter, which is what it means.
            let label = match (field, shell) {
                (RunField::Program, true) => t!("run.field_interpreter").to_string(),
                (RunField::Arguments, true) => t!("run.field_interpreter_arguments").to_string(),
                _ => field.label(),
            };
            // A prepared-action list is edited row by row, so each action can be moved or removed
            // without its text being re-parsed first.
            let list = matches!(field, RunField::Build | RunField::Prelaunch);
            h_flex()
                .gap_2()
                .items_center()
                .child(div().w(px(150.)).child(label))
                .child(
                    div()
                        .debug_selector(move || field.selector().into())
                        .flex_1()
                        .child(if list {
                            render_step_rows(&app, field, shell, cx).into_any_element()
                        } else {
                            if let Some(input) = textarea {
                                div()
                                    .child(crate::ui::controls::Textarea::new(&input))
                                    .into_any_element()
                            } else {
                                match input {
                                    // The field renders its own editing state, so text never resets.
                                    Some(input) => div()
                                        .child(crate::ui::controls::Input::new(&input))
                                        .into_any_element(),
                                    None => div().child(text).into_any_element(),
                                }
                            }
                        }),
                )
                .into_any_element()
        })
        .collect::<Vec<_>>();

    content.child(
        v_flex()
            .id("run-config-form")
            .overflow_y_scroll()
            .debug_selector(|| "run-config-form".into())
            .size_full()
            .gap_3()
            .p_4()
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .debug_selector(|| "run-config-existing".into())
                            .child(t!("run.form_saved_count", count = saved.len()).to_string()),
                    )
                    .child({
                        let owner = app.clone();
                        Button::new("run-config-discover")
                            .debug_selector(|| "run-config-discover".into())
                            .label(t!("run.discover").to_string())
                            .on_click(move |_, _, cx| {
                                // Discovery only publishes candidates; choosing a row is the
                                // explicit confirmation that persists an editable configuration.
                                owner.update(cx, |state, cx| state.discover_run_targets(cx));
                            })
                    })
                    .child({
                        let owner = app.clone();
                        Button::new("run-config-new")
                            .small()
                            .compact()
                            .ghost()
                            .debug_selector(|| "run-config-new".into())
                            .label(t!("run.form_new").to_string())
                            .on_click(move |_, window, cx| {
                                owner.update(cx, |state, cx| {
                                    let key = state.workspace_key();
                                    state.run_form = Some(cx.new(|cx| {
                                        RunConfigForm::open(
                                            &state.run_controls,
                                            &key,
                                            None,
                                            window,
                                            cx,
                                        )
                                    }));
                                    cx.notify();
                                });
                            })
                    }),
            )
            .child(render_discovered_targets(&app, cx))
            .child(h_flex().gap_1().children(tabs))
            .child(
                // The mode is an explicit choice: a program's arguments are never a command line,
                // and a script is never split into one.
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().w(px(150.)).child(t!("run.form_mode").to_string()))
                    .child({
                        let owner = app.clone();
                        Button::new("run-config-mode-program")
                            .small()
                            .compact()
                            .ghost()
                            .debug_selector(|| "run-config-mode-program".into())
                            .when(!shell, |mode| mode.text_color(cx.theme().primary))
                            .label(t!("run.field_program").to_string())
                            .on_click(move |_, _, cx| {
                                owner.update(cx, |state, cx| {
                                    if let Some(form) = state.run_form.as_ref() {
                                        form.update(cx, |form, cx| {
                                            form.draft.shell = false;
                                            form.error = None;
                                            cx.notify();
                                        });
                                    }
                                    cx.notify();
                                });
                            })
                    })
                    .child({
                        let owner = app.clone();
                        Button::new("run-config-mode-shell")
                            .small()
                            .compact()
                            .ghost()
                            .debug_selector(|| "run-config-mode-shell".into())
                            .when(shell, |mode| mode.text_color(cx.theme().primary))
                            .label(t!("run.form_shell").to_string())
                            .on_click(move |_, _, cx| {
                                owner.update(cx, |state, cx| {
                                    if let Some(form) = state.run_form.as_ref() {
                                        form.update(cx, |form, cx| {
                                            form.draft.shell = true;
                                            form.error = None;
                                            cx.notify();
                                        });
                                    }
                                    cx.notify();
                                });
                            })
                    }),
            )
            .child(
                v_flex()
                    .debug_selector(|| "run-config-basic".into())
                    .gap_2()
                    .children(fields),
            )
            .when(!tab.available(), |form| {
                form.child(
                    div()
                        .debug_selector(|| "run-config-tab-hint".into())
                        .child(tab.hint()),
                )
            })
            .when(tab == RunConfigTab::Build, |form| {
                let provider_build = app
                    .read(cx)
                    .run_form
                    .as_ref()
                    .map(|form| form.read(cx).draft.provider_build.clone())
                    .unwrap_or_default();
                form.children(
                    provider_build
                        .into_iter()
                        .map(|(_, step)| div().child(step.name)),
                )
            })
            .when(tab == RunConfigTab::Debug, |form| {
                // The provider choice belongs to this page. Following the default and asking for a
                // named provider are two different things, and a provider that cannot run is shown
                // with its reason rather than hidden or silently replaced.
                let mut page = form.child(
                    div().debug_selector(|| "run-provider-rows".into()).child(
                        t!(
                            "run.provider_current",
                            provider = providers
                                .iter()
                                .find(|candidate| candidate.selected)
                                .map(|candidate| candidate.plugin.clone())
                                .unwrap_or_else(|| t!("run.provider_none").to_string())
                        )
                        .to_string(),
                    ),
                );
                let mut rows = vec![(
                    None,
                    t!("run.provider_default").to_string().to_owned(),
                    None,
                )];
                for candidate in &providers {
                    rows.push((
                        Some(candidate.plugin.clone()),
                        candidate.plugin.clone(),
                        candidate.unavailable.clone(),
                    ));
                }
                for (plugin, label, unavailable) in rows {
                    let owner = app.clone();
                    let requested = plugin.clone();
                    let mut row = Button::new(format!(
                        "run-provider-{}",
                        plugin.clone().unwrap_or_else(|| "default".into())
                    ))
                    .debug_selector({
                        let plugin = plugin.clone();
                        move || {
                            format!(
                                "run-provider-{}",
                                plugin.clone().unwrap_or_else(|| "default".into())
                            )
                        }
                    })
                    .small()
                    .compact()
                    .ghost()
                    .disabled(unavailable.is_some())
                    .when(chosen_provider == plugin, |row| row.primary());
                    let text = match &unavailable {
                        Some(reason) => format!("{label}（{reason}）"),
                        None => label.clone(),
                    };
                    // A provider that cannot run is not selectable: offering it would only produce a
                    // failure the user already has the explanation for.
                    if unavailable.is_none() {
                        row = row.on_click(move |_, _, cx| {
                            owner.update(cx, |state, cx| {
                                if let Some(form) = state.run_form.as_ref() {
                                    form.update(cx, |form, cx| {
                                        form.draft.provider = requested.clone();
                                        cx.notify();
                                    });
                                }
                                // The choice is applied to the scope immediately, so every launch
                                // path agrees with what this page shows, and only launches that
                                // start later are affected.
                                state.apply_provider_choice(cx);
                                cx.notify();
                            });
                        });
                    }
                    page = page.child(row.label(text));
                }
                // B1 edits launch settings. Live target controls and inspection have one stable
                // home in the unified native panel, so opening this form never moves that view.
                page.child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!("run.form_debug_panel_hint")),
                )
            })
            .when_some(error, |form, message| {
                form.child(
                    div()
                        .debug_selector(|| "run-config-error".into())
                        .text_color(danger)
                        .child(message),
                )
            })
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .border_t_1()
                    .border_color(border)
                    .pt_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .debug_selector(|| "run-config-destination".into())
                            .child(t!("run.form_destination").to_string())
                            .child(save_destination(
                                &app,
                                share,
                                "run-config-local",
                                t!("run.form_local").to_string(),
                                false,
                            ))
                            .child(save_destination(
                                &app,
                                share,
                                "run-config-shared",
                                t!("run.form_shared").to_string(),
                                true,
                            )),
                    )
                    .child(div().flex_1())
                    .child({
                        let owner = app.clone();
                        Button::new("run-config-cancel")
                            .small()
                            .compact()
                            .ghost()
                            .debug_selector(|| "run-config-cancel".into())
                            .label(t!("run.form_cancel").to_string())
                            .on_click(move |_, _, cx| {
                                owner.update(cx, |state, cx| state.close_run_form(cx));
                            })
                    })
                    .child({
                        let owner = app.clone();
                        Button::new("run-config-save")
                            .small()
                            .compact()
                            .ghost()
                            .debug_selector(|| "run-config-save".into())
                            .label(t!("run.form_save").to_string())
                            .on_click(move |_, _, cx| {
                                owner.update(cx, |state, cx| state.commit_run_form(cx));
                            })
                    }),
            ),
    )
}
