//! Debug inspection belongs to its task tab; snapshots are read-only projections of RunControls.
use super::*;
use gpui_kit::{FocusHandle, KeyDownEvent, ScrollHandle};

/// Retained focus is shared with the selected native inspection, never with target stdin.
pub(crate) struct DebugPanelState {
    pub open: bool,
    focus: FocusHandle,
}
impl DebugPanelState {
    /// Preparing presentation does not allocate a debug target or another dock panel.
    pub fn new(cx: &mut App) -> Self {
        Self {
            open: false,
            focus: cx.focus_handle(),
        }
    }
}
impl gpui_kit::Focusable for DebugPanelState {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EditorApp {
    /// Force is distinct from DAP disconnect: synchronously revoke the selected target's own root.
    pub(crate) fn force_debug(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.run_controls.debug_provider_session() else {
            if let Some(config) = self
                .run_controls
                .debug_session()
                .map(|(config, _)| config.to_owned())
            {
                self.run_controls.defer_debug_close(&config);
            }
            self.status = t!("run.debug_connecting").into();
            cx.notify();
            return;
        };
        let Some(request) = self.run_controls.begin_debug_request(
            crate::run::DebugMethod::Control(crate::run::DebugControl::Force),
            None,
        ) else {
            return;
        };
        self.run_controls.note_debug_action();
        if !self
            .extensions
            .read(cx)
            .stage_host_run(Work::ForceDebug { session, request })
        {
            self.run_controls.abandon_debug_request(request);
            self.run_controls.note_debug_action_finished();
            self.status = t!("run.debug_unavailable").into();
        }
        cx.notify();
    }

    /// Keyboard navigation selects a real reported frame, then reads that frame's same-pause scope.
    fn select_inspection_frame(&mut self, frame: u32, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = self.run_controls.select_debug_frame(frame) {
            self.status = super::super::debug_presentation::inspection_error(&error);
        } else {
            if let Some((source, line)) = self
                .run_controls
                .debug_location()
                .map(|(source, line)| (source.to_owned(), line))
            {
                self.open_debug_location(&source, line, window, cx);
            }
            self.fetch_debug_frame_variables(cx);
        }
        cx.notify();
    }

    /// Publish the selected tab's canonical inspection without borrowing the parent during child render.
    pub(crate) fn sync_debug_inspection(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.terminal.read(cx).active_task() else {
            return;
        };
        if !self.terminal.read(cx).task_is_debug(&key) {
            return;
        }
        if self.run_controls.debug_session_of(&key).is_none() {
            return;
        }
        self.run_controls.select_debug_session(&key);
        self.fetch_debug_inspection(cx);
        let status = match self.run_controls.debug_state() {
            editor_core::DebugSessionState::Disconnected => {
                t!("run.debug_disconnected").to_string()
            }
            editor_core::DebugSessionState::Starting => t!("run.debug_connecting").to_string(),
            editor_core::DebugSessionState::Running => t!("run.debug_running").to_string(),
            editor_core::DebugSessionState::Paused { source, line, .. } => {
                t!("run.debug_paused", source = source, line = line.to_string()).to_string()
            }
            editor_core::DebugSessionState::Exited => t!("run.debug_exited").to_string(),
            editor_core::DebugSessionState::Failed { reason } => {
                t!("run.debug_failed", reason = reason).to_string()
            }
        };
        let model = Inspection {
            configuration: key.clone(),
            session: self.run_controls.debug_provider_session(),
            pause: self.run_controls.debug_pause_epoch(),
            controls: self.run_controls.debug_controls(),
            rows: self.run_controls.debug_panel_rows(),
            status,
            force: self.run_controls.debug_provider_session().is_some()
                && self.run_controls.debug_target_active(&key),
            breakpoints: self
                .run_controls
                .debug_breakpoint_positions()
                .into_iter()
                .map(|(source, line)| {
                    let verified = self.run_controls.debug_breakpoint_verified(&source, line);
                    (source, line, verified)
                })
                .collect(),
        };
        let parent = cx.entity().downgrade();
        let focus = self.debug_panel.focus.clone();
        self.terminal.update(cx, |panel, cx| {
            panel.publish_inspection(key, model, parent, focus, cx)
        });
    }
}

/// This model is immutable publication data. Canonical sessions, pauses and variables stay in RunControls.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Inspection {
    configuration: String,
    session: Option<String>,
    pause: Option<u64>,
    controls: editor_core::DebugControls,
    rows: crate::run::DebugPanelRows,
    status: String,
    breakpoints: Vec<(String, u32, Option<bool>)>,
    force: bool,
}
#[derive(Clone)]
enum InspectAction {
    Control(&'static str),
    Step(editor_core::DebugStep),
    Frame(u32),
    Navigate(String, u32),
    Hide,
}

/// One retained view per task keeps scrolling/focus while its tab is hidden.
pub(crate) struct InspectionView {
    parent: WeakEntity<EditorApp>,
    model: Inspection,
    focus: FocusHandle,
    frames: ScrollHandle,
    variables: ScrollHandle,
}
impl InspectionView {
    pub(crate) fn new(
        parent: WeakEntity<EditorApp>,
        model: Inspection,
        focus: FocusHandle,
    ) -> Self {
        Self {
            parent,
            model,
            focus,
            frames: ScrollHandle::new(),
            variables: ScrollHandle::new(),
        }
    }
    /// Only changed publications invalidate this child; output alone does not reset its controls.
    pub(crate) fn publish(&mut self, model: Inspection, cx: &mut Context<Self>) {
        if self.model != model {
            self.model = model;
            cx.notify();
        }
    }
    fn dispatch(&self, action: InspectAction, window: &mut Window, cx: &mut App) {
        let parent = self.parent.clone();
        let snapshot = self.model.clone();
        let handle = window.window_handle();
        // End the child's mutable event borrow before the coordinator may update the same terminal.
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                let _ = parent.update(cx, |app, cx| {
                    app.run_controls
                        .select_debug_session(&snapshot.configuration);
                    // A delayed control cannot resume a newer pause or close a replacement target.
                    if app.run_controls.debug_provider_session() != snapshot.session
                        || (matches!(
                            action,
                            InspectAction::Step(_)
                                | InspectAction::Frame(_)
                                | InspectAction::Control("resume")
                        ) && app.run_controls.debug_pause_epoch() != snapshot.pause)
                    {
                        app.status = t!("run.debug_stale_pause").to_string();
                        cx.notify();
                        return;
                    }
                    match action {
                        InspectAction::Control("force") => app.force_debug(cx),
                        InspectAction::Control(method) => app.debug_action(method, cx),
                        InspectAction::Step(kind) => app.step_debug(kind, cx),
                        InspectAction::Frame(frame) => {
                            app.select_inspection_frame(frame, window, cx)
                        }
                        InspectAction::Navigate(source, line) => {
                            app.open_debug_location(&source, line, window, cx);
                        }
                        InspectAction::Hide => app
                            .terminal
                            .update(cx, |panel, cx| panel.set_visible(false, cx)),
                    }
                });
            });
        });
    }
    fn button(
        &self,
        id: String,
        label: String,
        reason: Option<String>,
        action: InspectAction,
        cx: &Context<Self>,
    ) -> Button {
        let selector = id.clone();
        Button::new(id)
            .small()
            .compact()
            .ghost()
            .label(label.clone())
            .disabled(reason.is_some())
            .tooltip(reason.unwrap_or(label))
            .debug_selector(move || selector.clone())
            .on_click(
                cx.listener(move |view, _, window, cx| view.dispatch(action.clone(), window, cx)),
            )
    }
}
impl Render for InspectionView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let model = &self.model;
        let mut toolbar = h_flex().gap_1().items_center().flex_wrap();
        for (method, key, result) in [
            ("resume", "run.debug_resume", &model.controls.resume),
            ("pause", "run.debug_pause", &model.controls.pause),
            ("stop", "run.debug_stop", &model.controls.stop),
        ] {
            toolbar = toolbar.child(self.button(
                format!("debug-panel-{method}"),
                t!(key).to_string(),
                result.as_ref().err().cloned(),
                InspectAction::Control(method),
                cx,
            ));
        }
        for (kind, result) in &model.controls.step {
            let key = match kind {
                editor_core::DebugStep::Into => "run.debug_into",
                editor_core::DebugStep::Over => "run.debug_over",
                editor_core::DebugStep::Out => "run.debug_out",
            };
            toolbar = toolbar.child(self.button(
                format!("debug-panel-step-{}", kind.as_str()),
                t!(key).to_string(),
                result.as_ref().err().cloned(),
                InspectAction::Step(*kind),
                cx,
            ));
        }
        toolbar = toolbar
            .child(self.button(
                "debug-panel-force".into(),
                t!("run.force").to_string(),
                (!model.force).then(|| t!("run.debug_exited").to_string()),
                InspectAction::Control("force"),
                cx,
            ))
            .child(self.button(
                "debug-panel-hide".into(),
                t!("run.debug_hide").to_string(),
                None,
                InspectAction::Hide,
                cx,
            ));
        let mut breakpoints = v_flex()
            .id("debug-panel-breakpoints")
            .debug_selector(|| "debug-panel-breakpoints".into())
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .child(div().font_semibold().child(t!("run.debug_breakpoints")));
        for (source, line, verified) in &model.breakpoints {
            let label = match verified {
                Some(true) => t!("run.breakpoint_verified"),
                Some(false) => t!("run.breakpoint_unverified"),
                None => t!("run.breakpoint_waiting"),
            };
            breakpoints = breakpoints.child(
                self.button(
                    format!("debug-panel-breakpoint-{source}-{line}"),
                    format!("{source}:{line} ({label})"),
                    None,
                    InspectAction::Navigate(source.clone(), *line),
                    cx,
                )
                .content_full_width(),
            );
        }
        let mut frames = v_flex()
            .id("debug-panel-frames")
            .debug_selector(|| "debug-panel-frames".into())
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.frames)
            .child(div().font_semibold().child(t!("run.debug_stack")));
        for row in &model.rows.frames {
            frames = frames.child(
                self.button(
                    format!("debug-panel-frame-{}", row.frame),
                    row.label.clone(),
                    None,
                    InspectAction::Frame(row.frame),
                    cx,
                )
                .content_full_width()
                .when(row.selected, |button| button.text_color(cx.theme().primary)),
            );
        }
        let mut variables = v_flex()
            .id("debug-panel-variables")
            .debug_selector(|| "debug-panel-variables".into())
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.variables)
            .child(div().font_semibold().child(t!("run.debug_locals")));
        if model.rows.variables.is_empty() {
            variables = variables.child(t!("run.debug_no_locals"));
        }
        for row in &model.rows.variables {
            let selector = row.selector.clone();
            variables = variables.child(
                div()
                    .debug_selector(move || selector.clone())
                    .child(row.label.clone()),
            );
        }
        v_flex()
            .id("debug-panel")
            .debug_selector(|| "debug-panel".into())
            .w_full()
            .h_full()
            .min_h_0()
            .p_2()
            .gap_1()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_sm()
            .track_focus(&self.focus)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, window, cx| {
                    view.focus.focus(window, cx);
                    cx.stop_propagation();
                }),
            )
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                let shift = event.keystroke.modifiers.shift;
                let action = match (key, shift) {
                    ("f5", false) => InspectAction::Control("resume"),
                    ("f5", true) => InspectAction::Control("stop"),
                    ("f6", _) => InspectAction::Control("pause"),
                    ("f10", _) => InspectAction::Step(editor_core::DebugStep::Over),
                    ("f11", false) => InspectAction::Step(editor_core::DebugStep::Into),
                    ("f11", true) => InspectAction::Step(editor_core::DebugStep::Out),
                    ("up" | "down", _) => {
                        let rows = &view.model.rows.frames;
                        if rows.is_empty() {
                            return;
                        }
                        let previous = rows.iter().position(|row| row.selected).unwrap_or(0);
                        let next = if key == "up" {
                            previous.saturating_sub(1)
                        } else {
                            (previous + 1).min(rows.len() - 1)
                        };
                        view.frames.scroll_to_item(next);
                        InspectAction::Frame(rows[next].frame)
                    }
                    _ => return,
                };
                view.dispatch(action, window, cx);
                cx.stop_propagation();
            }))
            .child(toolbar)
            .child(
                div()
                    .debug_selector(|| "debug-panel-state".into())
                    .child(model.status.clone()),
            )
            .child(
                h_flex()
                    .items_stretch()
                    .flex_1()
                    .min_h_0()
                    .gap_3()
                    .child(breakpoints)
                    .child(frames)
                    .child(variables),
            )
            .when(model.rows.another_paused, |panel| {
                panel.child(t!("run.debug_other_paused"))
            })
    }
}
