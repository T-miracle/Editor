//! Retained native run-configuration state, literal fields and action rows.
use super::*;

/// Navigation is explicit and never persists a draft merely because a row was selected.
#[derive(Clone)]
pub(super) enum FormSelection {
    Configuration(String),
    Candidate(String),
    New(bool),
    Copy,
    Delete,
}

/// Detail editors share the owning HWND; field cancellation restores its original text.
pub(super) enum FormEditor {
    Field {
        field: RunField,
        original: String,
    },
    Step {
        field: RunField,
        index: Option<usize>,
        state: Entity<configuration::StepEditor>,
    },
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
    pub(super) const ALL: [Self; 10] = [
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
    pub(super) fn multiline(self) -> bool {
        matches!(
            self,
            Self::Arguments
                | Self::Script
                | Self::Environment
                | Self::ToolPaths
                | Self::Breakpoints
        )
    }
    pub(super) fn label(self) -> String {
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

    pub(super) fn selector(self) -> &'static str {
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

    pub(super) fn value(self, draft: &RunConfigDraft) -> String {
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

    pub(super) fn apply(self, draft: &mut RunConfigDraft, value: String) {
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
    /// Public template forms use this draft; old helpers remain isolated during the integration cutover.
    pub(super) plugin: Option<Box<super::plugin_form::State>>,
    /// Rare options start collapsed; toggling visibility keeps the same input entities.
    pub(super) startup_open: bool,
    pub(super) more_open: bool,
    /// Summary buttons keep keyboard focus across repaints that add or remove their content.
    pub(super) disclosure_focus: [FocusHandle; 2],
    /// A newly selected configuration starts at the top; folding keeps this draft's own scroll offset.
    pub(super) page_scroll: gpui_kit::ScrollHandle,
    /// The baseline detects pending edits before switching to another configuration.
    original: RunConfigDraft,
    pub(super) pending_selection: Option<FormSelection>,
    pub(super) editor: Option<FormEditor>,
    pub(super) return_focus: Option<FocusHandle>,
    /// A blank draft offers plugin targets first; manual types are chosen explicitly in the menu.
    pub(super) manual_target: bool,
    /// The configuration being edited, including its stable identity.
    pub(super) draft: RunConfigDraft,
    /// Validation or storage message shown above the dialog buttons.
    pub(super) error: Option<String>,
    /// Editing state for each field, created with the dialog so text never resets per frame.
    pub(super) inputs: Vec<(RunField, Entity<InputState>)>,
    /// Lists/scripts need actual Enter and multi-line selection, rather than a single-line placeholder.
    pub(super) textareas: Vec<(RunField, Entity<TextareaState>)>,
    /// One editing state per prepared action, for the lists that are edited row by row.
    ///
    /// A row is its own single-line field so an action can be moved or removed without its text
    /// being re-parsed, and so the row controls always address the action the user sees.
    pub(super) rows: Vec<(RunField, Entity<InputState>)>,
    /// One transient picker for configuration or save location; closing the form revokes it.
    pub(super) picker: Option<RunMenu>,
    /// Subscriptions are retained here; dropping the form releases them with its inputs.
    pub(super) _subscriptions: Vec<Subscription>,
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
        Self::from_draft(draft, window, cx)
    }

    /// Install defaults and native editing state together, without any storage or process side effect.
    pub(super) fn from_draft(
        draft: RunConfigDraft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut form = Self {
            plugin: None,
            startup_open: false,
            more_open: false,
            disclosure_focus: [cx.focus_handle(), cx.focus_handle()],
            page_scroll: gpui_kit::ScrollHandle::new(),
            manual_target: draft.provided.is_none() && (!draft.program.is_empty() || draft.shell),
            original: draft.clone(),
            pending_selection: None,
            editor: None,
            return_focus: None,
            draft,
            error: None,
            inputs: Vec::new(),
            textareas: Vec::new(),
            rows: Vec::new(),
            picker: None,
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
                        if matches!(event, InputEvent::Change) {
                            form.error = None;
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
                if matches!(event, InputEvent::Change) {
                    form.error = None;
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
    pub(super) fn rebuild_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    pub(super) fn sync_rows(&mut self, field: RunField, cx: &gpui_kit::App) {
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
    pub(super) fn text(&self, field: RunField, cx: &gpui_kit::App) -> String {
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
    pub(super) fn collect(&mut self, cx: &gpui_kit::App) {
        self.draft = self.snapshot(cx);
    }

    /// Compare live inputs, including collapsed fields, without changing their baseline or undo state.
    pub(super) fn snapshot(&self, cx: &gpui_kit::App) -> RunConfigDraft {
        let mut draft = self.draft.clone();
        for field in RunField::ALL {
            field.apply(&mut draft, self.text(field, cx));
        }
        for field in [RunField::Build, RunField::Prelaunch] {
            field.apply(
                &mut draft,
                join_step_lines(
                    &self
                        .rows_of(field)
                        .iter()
                        .map(|(_, input)| input.read(cx).value().to_string())
                        .collect::<Vec<_>>(),
                ),
            );
        }
        draft
    }

    /// A navigation request cannot silently replace typed values or a changed save location.
    pub(super) fn modified(&self, cx: &gpui_kit::App) -> bool {
        self.snapshot(cx) != self.original
    }

    /// Identify a rejected retained field using the public parsers and core validator, even if hidden.
    /// This routes the existing rules to their editing surface instead of duplicating validation rules.
    pub(super) fn validated_configuration(
        &mut self,
        cx: &gpui_kit::App,
    ) -> Result<editor_core::RunConfig, (RunField, String)> {
        self.collect(cx);
        if self.draft.shell && self.draft.script.trim().is_empty() {
            return Err((RunField::Script, t!("run.form_script_required").into()));
        }
        crate::run::parse_environment(&self.draft.environment)
            .map_err(|error| (RunField::Environment, error))?;
        crate::run::parse_steps(&self.draft.build).map_err(|error| (RunField::Build, error))?;
        crate::run::parse_steps(&self.draft.prelaunch)
            .map_err(|error| (RunField::Prelaunch, error))?;
        crate::run::parse_breakpoints(&self.draft.breakpoints)
            .map_err(|error| (RunField::Breakpoints, error))?;
        let config = self
            .draft
            .to_config()
            .map_err(|error| (RunField::Name, error))?;
        config.validate().map_err(|error| {
            use editor_core::RunConfigError as Error;
            let field = match error {
                Error::EmptyName | Error::NameTooLong | Error::InvalidIdentity { .. } => {
                    RunField::Name
                }
                Error::EmptyProgram | Error::ProgramTooLong => RunField::Program,
                Error::ArgumentTooLong | Error::TooManyArguments => RunField::Arguments,
                Error::InvalidBreakpoint(_) => RunField::Breakpoints,
                Error::DirectoryNotAbsolute | Error::DirectoryTooLong => RunField::Directory,
                Error::TooManyToolPaths | Error::InvalidToolPath { .. } => RunField::ToolPaths,
                Error::TooManyEnvEntries | Error::InvalidEnvEntry { .. } => RunField::Environment,
                _ => RunField::Build,
            };
            (field, error.to_string())
        })?;
        Ok(config)
    }

    /// Restore a detail edit through its original native control, preserving literal text and IME.
    pub(super) fn set_text(
        &mut self,
        field: RunField,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((_, input)) = self.textareas.iter().find(|(key, _)| *key == field) {
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        } else if let Some((_, input)) = self.inputs.iter().find(|(key, _)| *key == field) {
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        }
    }
}

/// Structural accessors exist only for native checks, so production builds carry no unused surface.
#[cfg(test)]
impl RunConfigForm {
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
}
