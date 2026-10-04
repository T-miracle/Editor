//! Native run controls: the title-bar group and the B1 configuration dialog.
//!
//! Every control here is a native widget composed from the project's own UI layer; no WebView is
//! used. The group's position, separation and disabled states follow the approved B1 layout, and a
//! control whose capability is not implemented yet is disabled with a visible reason rather than
//! behaving like the control next to it.
use super::{
    LaunchPlan, MAX_PREPARED_STEPS, RunConfigDraft, RunControls, RunMenuEntry, SequenceAction,
};
use crate::app::dialog as app_dialog;
use crate::extensions::HostWork as Work;
use crate::ui::controls::menu::MenuStyle;
use crate::ui::controls::{Button, DialogContent};
// The crate root already selects the same widget and styling traits the rest of the shell uses.
use crate::*;
use gpui_base::input::{Input as BaseInput, InputEvent, InputState};
use gpui_kit::component::menu::{PopupMenu as KitPopupMenu, PopupMenuItem};
use gpui_kit::{AnyElement, DismissEvent, WeakEntity, Window, div, px};
use rust_i18n::t;
use sha2::{Digest, Sha256};

/// Width of the unified run dropdown; it holds session labels with state words beside them.
const RUN_MENU_WIDTH: f32 = 260.;

/// The unified dropdown retaining the component lifetime and its dismissal subscription.
pub(crate) struct RunMenu {
    pub position: gpui_kit::Point<gpui_kit::Pixels>,
    pub popup: Entity<KitPopupMenu>,
    _dismiss: Subscription,
}

/// The B1 configuration dialog's tabs; only the basic page is implemented by this slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunConfigTab {
    Basic,
    Build,
    Debug,
    Environment,
}

impl RunConfigTab {
    const ALL: [Self; 4] = [Self::Basic, Self::Build, Self::Debug, Self::Environment];

    fn label(self) -> &'static str {
        match self {
            Self::Basic => "基本",
            Self::Build => "构建",
            Self::Debug => "调试",
            Self::Environment => "环境",
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

    fn hint(self) -> &'static str {
        match self {
            Self::Basic => "名称、程序、参数与工作目录",
            Self::Build => "构建操作与启动前步骤：前者可只构建，后者在启动前按顺序执行",
            Self::Debug => "调试提供者与断点设置随后续工单提供",
            Self::Environment => "本配置的环境变量，每行一个 名称=值",
        }
    }

    /// Tabs whose settings are not implemented yet are shown disabled instead of pretending.
    fn available(self) -> bool {
        // Basic, environment and build all edit one stored configuration; only debug is still to come.
        matches!(self, Self::Basic | Self::Environment | Self::Build)
    }
}

/// One labelled single-line field of the basic page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RunField {
    Name,
    Program,
    Arguments,
    Script,
    Directory,
    Environment,
    ToolPaths,
    Build,
    Prelaunch,
}

impl RunField {
    const ALL: [Self; 9] = [
        Self::Name,
        Self::Program,
        Self::Arguments,
        Self::Script,
        Self::Directory,
        Self::Environment,
        Self::ToolPaths,
        Self::Build,
        Self::Prelaunch,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Name => "名称",
            Self::Program => "程序",
            // One argument per line keeps a value containing spaces literal.
            Self::Arguments => "参数（每行一个）",
            Self::Script => "脚本文本",
            Self::Directory => "工作目录",
            Self::Environment => "环境变量（每行 名称=值）",
            Self::ToolPaths => "本机工具路径（每行一个目录，优先于继承的 PATH）",
            Self::Build => "构建操作（每行 名称 = 程序 | 参数）",
            Self::Prelaunch => "启动前步骤（每行 名称 = 程序 | 参数，顺序执行）",
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
        }
    }
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
            _subscriptions: Vec::new(),
        };
        for field in RunField::ALL {
            let initial = field.value(&form.draft);
            let label = field.label();
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
        form
    }

    /// Current text of one field, read from its editing state.
    fn text(&self, field: RunField, cx: &gpui_kit::App) -> String {
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
    }
}

/// Structural accessors exist only for native checks, so production builds carry no unused surface.
#[cfg(test)]
impl RunConfigForm {
    /// The tabs this dialog offers, with whether this build implements each one.
    ///
    /// Lets a native check confirm the approved structure without depending on how the strip is
    /// painted in the dialog's own window.
    pub(crate) fn tab_labels(&self) -> Vec<(&'static str, bool)> {
        RunConfigTab::ALL
            .into_iter()
            .map(|tab| (tab.label(), tab.available()))
            .collect()
    }

    /// The labelled fields of the basic page, in the order they are presented.
    pub(crate) fn field_labels(&self) -> Vec<&'static str> {
        RunField::ALL.into_iter().map(RunField::label).collect()
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
    pub(crate) fn sync_run_controls(&mut self, cx: &mut Context<Self>) {
        let (executions, errors, stops, statuses) = self.extensions.read(cx).take_host_runs();
        self.run_controls.reconcile(&executions);
        // A step's session belongs to its preparation, so the sequence learns about it before the
        // next step can be requested.
        for execution in &executions {
            if let Some(step) = self.run_controls.step_of(execution.id) {
                let (config, index) = (step.0.to_owned(), step.1);
                if execution.state == plugin_runtime::ExecutionState::Running {
                    self.run_controls.sequence_started(
                        &config,
                        index,
                        execution.id,
                        execution.provider_session.clone(),
                    );
                }
            }
        }
        if let Some((_, _, message)) = errors.first() {
            // A refused start is reported where the launch was requested instead of failing silently.
            self.status = message.clone();
        }
        for (session, result) in self.run_controls.reconcile_stops(&stops) {
            // An acknowledgement means the provider was asked, not that the program has exited, so
            // the visible state never claims more than the provider actually reported.
            self.status = match result {
                Ok(()) => format!("已请求停止会话 {session}"),
                Err(message) => format!("停止会话 {session} 失败：{message}"),
            };
        }
        // An observed end advances its sequence; a program still running is not progress.
        for (config, _, _) in self.run_controls.reconcile_run_status(&statuses) {
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
        let previous_focus = window
            .focused(cx)
            .unwrap_or_else(|| self.editor.focus_handle(cx));
        // One description drives both the menu and its native check, so grouping cannot drift
        // between what a test asserts and what a user opens.
        let entries = self.run_controls.menu_entries();
        let popup = KitPopupMenu::build(window, cx, move |mut menu, _window, _cx| {
            menu = menu
                .min_w(px(RUN_MENU_WIDTH))
                .action_context(previous_focus);
            for entry in entries {
                match entry {
                    RunMenuEntry::Separator => menu = menu.separator(),
                    RunMenuEntry::Session { id, label } => {
                        let locate = owner.clone();
                        menu =
                            menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                                let _ = locate.update(cx, |app, cx| {
                                    // The session's own configuration is the selection Stop acts on.
                                    let config = app
                                        .run_controls
                                        .sessions()
                                        .into_iter()
                                        .find(|session| session.id == id)
                                        .map(|session| session.config)
                                        .unwrap_or_default();
                                    app.reveal_run_session(id, &config, window, cx);
                                });
                            }));
                    }
                    RunMenuEntry::Configuration { id, label } => {
                        let choose = owner.clone();
                        menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                            let _ = choose.update(cx, |app, cx| {
                                let key = app.workspace_key();
                                app.run_controls.select(&id, &key);
                                cx.notify();
                            });
                        }));
                    }
                    RunMenuEntry::Action { id, label, enabled } => {
                        if !enabled {
                            menu = menu.item(PopupMenuItem::new(label).disabled(true));
                            continue;
                        }
                        let act = owner.clone();
                        menu =
                            menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                                let _ = act.update(cx, |app, cx| match id.as_str() {
                                    "run-edit" => {
                                        let editing =
                                            app.run_controls.selected().map(|c| c.id.clone());
                                        app.open_run_config_dialog(window, cx, editing);
                                    }
                                    "run-new" => app.open_run_config_dialog(window, cx, None),
                                    _ => {}
                                });
                            }));
                    }
                }
            }
            menu
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
            position,
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
            .child(
                gpui_kit::anchored()
                    .position(menu.position)
                    .child(menu.popup.clone()),
            )
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
        // Stopping is requested per session; the window does not wait for each answer, because a
        // provider that has already exited cannot answer and the host is leaving anyway.
        for session in self.run_controls.active_sessions() {
            let request_id = self.run_controls.begin_stop(&session.config, session.id);
            let _ = self.extensions.read(cx).stage_host_run(Work::StopRun {
                session: session.id,
                config: session.config.clone(),
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
        let count = sessions.len();
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
                        .child(div().font_semibold().child("仍有正在运行的程序"))
                        .child(div().child(format!(
                            "关闭窗口会先停止 {count} 个运行会话。取消可保留它们继续运行。"
                        )))
                        .child(
                            h_flex()
                                .gap_2()
                                .justify_end()
                                .child(
                                    div()
                                        .id("run-leave-cancel")
                                        .debug_selector(|| "run-leave-cancel".into())
                                        .child("取消")
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.cancel_leave(cx)),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("run-leave-confirm-accept")
                                        .debug_selector(|| "run-leave-confirm-accept".into())
                                        .child("停止并关闭")
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
        let running = active.is_some() || pending;
        // Build is about this configuration's own build actions, so its control reports the reason it
        // cannot run rather than being permanently unavailable.
        let build_blocker = selected.as_ref().and_then(|config| {
            if !permitted {
                return Some("受限工作区不能启动程序".to_owned());
            }
            if self.run_controls.is_preparing(&config.id) {
                return Some("该配置正在准备".to_owned());
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
                    format!("{} · 启动中", config.name)
                } else if active.is_some() {
                    format!("{} · 运行中", config.name)
                } else {
                    config.name.clone()
                }
            })
            .unwrap_or_else(|| "运行配置".to_string());
        let state = active
            .as_ref()
            .map(|session| session.state)
            .or(pending.then_some(plugin_runtime::ExecutionState::Starting));

        h_flex()
            .debug_selector(|| "run-controls".into())
            .items_center()
            .gap_1()
            .child(
                div().debug_selector(|| "run-config-selector".into()).child(
                    Button::new("run-config-select")
                        .label(short_label(&label))
                        .small()
                        .compact()
                        .ghost()
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
                        .label("构建")
                        .small()
                        .compact()
                        .ghost()
                        // Build runs the configuration's own build actions and nothing else: no
                        // pre-launch step, no program. A configuration without build actions keeps
                        // the control disabled with the reason it is disabled.
                        .disabled(!permitted || build_blocker.is_some())
                        .tooltip(
                            build_blocker
                                .clone()
                                .unwrap_or_else(|| "只执行构建操作".into()),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.build_selected(window, cx);
                        })),
                ),
            )
            .child(
                div().debug_selector(|| "run-start".into()).child(
                    Button::new("run-start-action")
                        .label("运行")
                        .small()
                        .compact()
                        .ghost()
                        .disabled(!permitted || selected.is_none())
                        .tooltip(if permitted {
                            "运行所选配置"
                        } else {
                            "受限工作区不能启动程序"
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.start_selected_run(window, cx);
                        })),
                ),
            )
            .child(
                div().debug_selector(|| "run-debug".into()).child(
                    Button::new("run-debug-action")
                        .label("调试")
                        .small()
                        .compact()
                        .ghost()
                        // Debugging arrives with its own ticket; it must never behave like Run.
                        .disabled(true)
                        .tooltip("尚无兼容的调试提供者")
                        .on_click(|_, _, _| {}),
                ),
            )
            .child(
                div().debug_selector(|| "run-stop".into()).child(
                    Button::new("run-stop-action")
                        .label("停止")
                        .small()
                        .compact()
                        .ghost()
                        .disabled(!running)
                        .tooltip("停止所选会话")
                        .on_click(cx.listener(|this, _, _, cx| this.stop_selected_run(cx))),
                ),
            )
            .child(
                div().debug_selector(|| "run-rerun".into()).child(
                    Button::new("run-rerun-action")
                        .label("重新运行")
                        .small()
                        .compact()
                        .ghost()
                        // Rerunning is its own action: a repeat Run click only reveals a session.
                        .disabled(!running)
                        .tooltip("先停止当前实例，再重新启动")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.rerun_selected(window, cx)),
                        ),
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
        let Some(config) = self
            .run_controls
            .configurations()
            .iter()
            .find(|config| config.id == config_id)
            .cloned()
        else {
            self.status = "运行配置不存在，请重新选择".into();
            cx.notify();
            return;
        };
        if !self.run_permitted(cx) {
            self.status = "受限工作区不能启动程序".into();
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
                self.status = format!("正在准备 {}", config.name);
                self.drive_preparation(cx);
            }
        }
    }

    /// Stage one already-composed launch request for a configuration and report it.
    fn stage_run_request(
        &mut self,
        config: &editor_core::RunConfig,
        request: plugin_runtime::RunRequest,
        cx: &mut Context<Self>,
    ) {
        let request_id = self.run_controls.begin(&config.id);
        let queued = self.extensions.read(cx).stage_host_run(Work::StartRun {
            request,
            config: config.id.clone(),
            request_id,
        });
        self.status = if queued {
            format!("正在启动 {}", config.name)
        } else {
            "插件后台服务不可用，无法启动".into()
        };
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
            self.status = "受限工作区不能启动程序".into();
            cx.notify();
            return;
        }
        if let Some(reason) = self.run_controls.preparation_error(&config.id) {
            self.status = reason;
            cx.notify();
            return;
        }
        let root = self.workspace_key();
        let plan = match self
            .run_controls
            .prepare(&config.id, &root, MAX_PREPARED_STEPS)
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
        self.status = format!("正在构建 {}", config.name);
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
                let action = self.run_controls.preparation(&config).map(|sequence| {
                    let controls = &self.run_controls;
                    sequence.next_action(
                        |session| controls.session_known(session),
                        |session| controls.session_is_active(session),
                    )
                });
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
                .sequence_start_failed(config, index, "步骤缺少可执行的命令");
            self.status = "准备步骤缺少可执行的命令".into();
            cx.notify();
            return false;
        };
        let request_id = self.run_controls.begin(config);
        // The step is owned before the request is staged, so nothing can request it twice.
        self.run_controls
            .note_step_request(config, index, request_id);
        let queued = self.extensions.read(cx).stage_host_run(Work::StartRun {
            request,
            config: config.to_owned(),
            request_id,
        });
        if !queued {
            self.run_controls
                .sequence_start_failed(config, index, "插件后台服务不可用");
            self.status = "插件后台服务不可用，无法继续准备".into();
            cx.notify();
            return false;
        }

        self.status = format!("{} {}", step.1.label(), step.0);
        cx.notify();
        true
    }

    /// Stop the program one preparation step owns.
    fn stop_preparation_step(&mut self, config: &str, session: u64, cx: &mut Context<Self>) {
        let request_id = self.run_controls.begin_stop(config, session);
        let queued = self.extensions.read(cx).stage_host_run(Work::StopRun {
            session,
            config: config.to_owned(),
            request_id,
        });
        if !queued {
            // The provider is unreachable, so the sequence cannot be told the program ended; it stays
            // owned rather than reporting a stop that did not happen.
            self.status = "插件后台服务不可用，无法停止准备步骤".into();
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
            self.status = "插件后台服务不可用，无法确认步骤状态".into();
            cx.notify();
        }
    }

    /// Report a preparation that finished, whether it ended in a launch or a completed build.
    fn finish_preparation(&mut self, config: &str, cx: &mut Context<Self>) {
        let name = self
            .run_controls
            .configuration(config)
            .map(|stored| stored.name.clone())
            .unwrap_or_else(|| config.to_owned());
        self.run_controls.forget_step_sessions(config);
        self.status = format!("{name} 构建完成");
        cx.notify();
    }

    /// Start a stored configuration exactly as its own target describes it.
    ///
    /// Used by the native acceptance for a mode the form produces, so the request under test is the
    /// one the launch path builds rather than a hand-assembled equivalent.
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
        self.stage_run_request(&config, request, cx);
    }

    /// Reveal one session's output: select its configuration and show the surface that owns it.
    ///
    /// The provider already asked the editor to show its own panel when it started the program; the
    /// host repeats only that recorded request rather than guessing which panel a provider uses.
    pub(crate) fn reveal_run_session(
        &mut self,
        session: u64,
        config: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(found) = self
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
        let outcome = self.show_provider_panel(&found.plugin, window, cx);
        self.status = match outcome {
            Ok(()) => format!(
                "定位会话 {}（{}）",
                found.provider_session.clone().unwrap_or_default(),
                run_state_label(found.state)
            ),
            Err(message) => format!("无法定位会话输出：{message}"),
        };
        cx.notify();
    }

    /// Stop is only meaningful once the provider accepts a stop request; until then it explains why.
    pub(crate) fn stop_selected_run(&mut self, cx: &mut Context<Self>) {
        let Some(config) = self.run_controls.selected().cloned() else {
            return;
        };
        let Some(session) = self.run_controls.running_for(&config.id) else {
            return;
        };
        if !session.is_active() {
            self.status = "所选会话已经结束".into();
            cx.notify();
            return;
        }
        let request_id = self.run_controls.begin_stop(&config.id, session.id);
        let queued = self.extensions.read(cx).stage_host_run(Work::StopRun {
            session: session.id,
            config: config.id.clone(),
            request_id,
        });
        self.status = if queued {
            // The provider is asked, not commanded by the host; the answer arrives in its own time.
            format!("正在请求停止会话 {}", session.id)
        } else {
            "插件后台服务不可用，无法请求停止".into()
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
        if let Some(session) = self.run_controls.running_for(&config.id) {
            // The stop is requested before the replacement starts, and its outcome is reported.
            let request_id = self.run_controls.begin_stop(&config.id, session.id);
            let queued = self.extensions.read(cx).stage_host_run(Work::StopRun {
                session: session.id,
                config: config.id.clone(),
                request_id,
            });
            if !queued {
                self.status = "插件后台服务不可用，无法重新运行".into();
                cx.notify();
                return;
            }
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
        // One configuration dialog owns the editor window at a time, like settings and the manager.
        if let Some((_, handle)) = self.run_dialog.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
        let key = self.workspace_key();
        let editing_id = editing.clone();
        let form = cx.new(|cx| {
            RunConfigForm::open(&self.run_controls, &key, editing_id.as_deref(), window, cx)
        });
        self.run_form = Some(form);
        let title = if editing.is_some() {
            "编辑运行配置"
        } else {
            "新建运行配置"
        };
        let entity = cx.entity().downgrade();
        let (dialog, handle) = app_dialog::open_dialog_sized(
            title,
            660.,
            440.,
            move |content, _, cx| render_run_config_form(&entity, content, cx),
            cx,
        );
        self.run_dialog = Some((dialog, handle));
        let weak = cx.entity().downgrade();
        let window_id = handle.window_id();
        self._run_dialog_closed = Some(cx.on_window_closed(move |cx, closed_id| {
            if closed_id == window_id {
                let _ = weak.update(cx, |this, cx| {
                    this.run_dialog = None;
                    this.run_form = None;
                    cx.notify();
                });
            }
        }));
        cx.notify();
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
                self.status = format!("已保存运行配置：{}", configuration.name);
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
        if let Some((_, handle)) = self.run_dialog.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
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
                self.status = format!("保存失败，未启动：{}", path.display());
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
        plugin_runtime::ExecutionState::Starting => "启动中".into(),
        plugin_runtime::ExecutionState::Running => "运行中".into(),
        plugin_runtime::ExecutionState::Failed => "已结束".into(),
    }
}

/// Render the B1 form: configuration entry points, four tabs, fields and the footer.
fn render_run_config_form(
    app: &WeakEntity<EditorApp>,
    content: DialogContent,
    cx: &mut gpui_kit::App,
) -> DialogContent {
    let Some(app) = app.upgrade() else {
        return content;
    };
    let (tab, error, shell, texts, inputs, saved) = {
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
            saved,
        )
    };
    let danger = cx.theme().danger;
    let border = cx.theme().border;

    let tabs = RunConfigTab::ALL
        .into_iter()
        .map(|candidate| {
            let selected = candidate == tab;
            let owner = app.clone();
            div()
                .id(candidate.selector())
                .debug_selector(move || candidate.selector().into())
                .px_3()
                .py_1()
                .rounded(px(4.))
                .when(selected, |tab| tab.bg(cx.theme().list_active))
                // A tab whose settings are not implemented is visibly disabled, not silently empty.
                .when(!candidate.available(), |tab| tab.opacity(0.5))
                .child(candidate.label())
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
        .filter(|((field, _), _)| page_fields.contains(field))
        .map(|((field, text), input)| {
            // In shell mode the executable field names the interpreter, which is what it means.
            let label = match (field, shell) {
                (RunField::Program, true) => "解释器",
                (RunField::Arguments, true) => "解释器参数（每行一个）",
                _ => field.label(),
            };
            h_flex()
                .gap_2()
                .items_center()
                .child(div().w(px(150.)).child(label))
                .child(
                    div()
                        .debug_selector(move || field.selector().into())
                        .flex_1()
                        .child(match input {
                            // The field renders its own editing state, so text never resets per frame.
                            Some(input) => div().child(BaseInput::new(&input)).into_any_element(),
                            None => div().child(text).into_any_element(),
                        }),
                )
                .into_any_element()
        })
        .collect::<Vec<_>>();

    content.child(
        v_flex()
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
                            .child(format!("已保存 {} 项", saved.len())),
                    )
                    .child(
                        div()
                            .debug_selector(|| "run-config-discover".into())
                            // Discovery arrives with its own ticket; the entry point explains itself
                            // instead of pretending to have found candidates.
                            .opacity(0.5)
                            .child("发现配置（随后续工单接通）"),
                    )
                    .child({
                        let owner = app.clone();
                        div()
                            .id("run-config-new")
                            .debug_selector(|| "run-config-new".into())
                            .child("新建")
                            .on_click(move |_, _, cx| {
                                owner.update(cx, |state, cx| {
                                    let key = state.workspace_key();
                                    let id = state.run_controls.generate_id(&key);
                                    if let Some(form) = state.run_form.as_ref() {
                                        form.update(cx, |form, cx| {
                                            form.draft = RunConfigDraft::from_config(None, id);
                                            form.error = None;
                                            cx.notify();
                                        });
                                    }
                                    cx.notify();
                                });
                            })
                    }),
            )
            .child(h_flex().gap_1().children(tabs))
            .child(
                // The mode is an explicit choice: a program's arguments are never a command line,
                // and a script is never split into one.
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().w(px(150.)).child("运行方式"))
                    .child({
                        let owner = app.clone();
                        div()
                            .id("run-config-mode-program")
                            .debug_selector(|| "run-config-mode-program".into())
                            .when(!shell, |mode| mode.font_semibold())
                            .child("程序")
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
                        div()
                            .id("run-config-mode-shell")
                            .debug_selector(|| "run-config-mode-shell".into())
                            .when(shell, |mode| mode.font_semibold())
                            .child("Shell 脚本")
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
                        div()
                            .debug_selector(|| "run-config-local".into())
                            // Saving is host-local by default; project sharing arrives with its ticket.
                            .child("保存到：仅本机"),
                    )
                    .child(div().flex_1())
                    .child({
                        let owner = app.clone();
                        div()
                            .id("run-config-cancel")
                            .debug_selector(|| "run-config-cancel".into())
                            .child("取消")
                            .on_click(move |_, _, cx| {
                                owner.update(cx, |state, cx| state.close_run_form(cx));
                            })
                    })
                    .child({
                        let owner = app.clone();
                        div()
                            .id("run-config-save")
                            .debug_selector(|| "run-config-save".into())
                            .child("保存配置")
                            .on_click(move |_, _, cx| {
                                owner.update(cx, |state, cx| state.commit_run_form(cx));
                            })
                    }),
            ),
    )
}
