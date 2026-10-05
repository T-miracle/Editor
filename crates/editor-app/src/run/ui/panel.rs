//! Unified native debug presentation. Base buttons own activation; GPUI owns bounded scrolling/focus.
//! The panel holds no target or text snapshot: every row comes from the selected RunControls session.
use super::*;
use gpui_kit::{FocusHandle, KeyDownEvent, ScrollHandle};

/// Presentation-only state survives re-paints and session switches without duplicating inspection.
pub(crate) struct DebugPanelState {
    pub open: bool,
    focus: FocusHandle,
    frames: ScrollHandle,
    variables: ScrollHandle,
}
impl DebugPanelState {
    /// The owner retains native focus and scroll handles; hiding never stops a debug target.
    pub fn new(cx: &mut App) -> Self {
        Self {
            open: false,
            focus: cx.focus_handle(),
            frames: ScrollHandle::new(),
            variables: ScrollHandle::new(),
        }
    }
}

/// The standard GPUI focus interface lets inspection retain keyboard navigation across source jumps.
impl gpui_kit::Focusable for DebugPanelState {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EditorApp {
    /// Force is distinct from DAP disconnect: synchronously revoke the selected target's own root.
    pub(crate) fn force_debug(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.run_controls.debug_provider_session() else {
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

    /// One native panel serves every provider and configuration, with ordinary theme/scale contracts.
    pub(crate) fn render_debug_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.debug_panel.open {
            return None;
        }
        let controls = self.run_controls.debug_controls();
        let rows = self.run_controls.debug_panel_rows();
        let mut breakpoints = v_flex()
            .id("debug-panel-breakpoints")
            .debug_selector(|| "debug-panel-breakpoints".into())
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .child(div().font_semibold().child(t!("run.debug_breakpoints")));
        for (source, line) in self.run_controls.debug_breakpoint_positions() {
            let verification = match self.run_controls.debug_breakpoint_verified(&source, line) {
                Some(true) => t!("run.breakpoint_verified"),
                Some(false) => t!("run.breakpoint_unverified"),
                None => t!("run.breakpoint_waiting"),
            };
            let location = source.clone();
            let selector = format!("debug-panel-breakpoint-{}-{line}", source);
            breakpoints = breakpoints.child(
                Button::new(selector.clone())
                    .small()
                    .ghost()
                    .content_full_width()
                    .label(format!("{source}:{line} ({verification})"))
                    .debug_selector(move || selector.clone())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_debug_location(&location, line, window, cx);
                    })),
            );
        }
        let mut toolbar = h_flex()
            .gap_1()
            .items_center()
            .flex_wrap()
            .child(div().font_semibold().child(t!("run.debug_panel")));
        // Stable configuration identities select the inspection; defaults never re-route live targets.
        for (configuration, _) in self.run_controls.debug_sessions() {
            let id = configuration.to_owned();
            let selected = self
                .run_controls
                .debug_session()
                .is_some_and(|(current, _)| current == configuration);
            let name = self
                .run_controls
                .configuration(configuration)
                .map(|config| config.name.clone())
                .unwrap_or_else(|| configuration.into());
            let selector = format!("debug-session-{configuration}");
            toolbar = toolbar.child(
                Button::new(selector.clone())
                    .label(name)
                    .small()
                    .compact()
                    .ghost()
                    .when(selected, |button| button.text_color(cx.theme().primary))
                    .debug_selector(move || selector.clone())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.run_controls.select_debug_session(&id);
                        cx.notify();
                    })),
            );
        }
        for (method, key, outcome) in [
            ("resume", "run.debug_resume", controls.resume),
            ("pause", "run.debug_pause", controls.pause),
            ("stop", "run.debug_stop", controls.stop),
        ] {
            let selector = format!("debug-panel-{method}");
            let reason = outcome.as_ref().err().cloned();
            toolbar = toolbar.child(
                Button::new(selector.clone())
                    .label(t!(key))
                    .small()
                    .compact()
                    .ghost()
                    .disabled(reason.is_some())
                    .tooltip(reason.unwrap_or_else(|| t!(key).into()))
                    .debug_selector(move || selector.clone())
                    .on_click(cx.listener(move |this, _, _, cx| this.debug_action(method, cx))),
            );
        }
        for (kind, key, outcome) in controls.step.into_iter().map(|(kind, outcome)| {
            (
                kind,
                match kind {
                    editor_core::DebugStep::Into => "run.debug_into",
                    editor_core::DebugStep::Over => "run.debug_over",
                    editor_core::DebugStep::Out => "run.debug_out",
                },
                outcome,
            )
        }) {
            let selector = format!("debug-panel-step-{}", kind.as_str());
            let reason = outcome.as_ref().err().cloned();
            toolbar = toolbar.child(
                Button::new(selector.clone())
                    .label(t!(key))
                    .small()
                    .compact()
                    .ghost()
                    .disabled(reason.is_some())
                    .tooltip(reason.unwrap_or_else(|| t!(key).into()))
                    .debug_selector(move || selector.clone())
                    .on_click(cx.listener(move |this, _, _, cx| this.step_debug(kind, cx))),
            );
        }
        toolbar = toolbar
            .child(
                Button::new("debug-panel-force")
                    .label(t!("run.force"))
                    .small()
                    .compact()
                    .ghost()
                    .disabled(
                        self.run_controls.debug_provider_session().is_none()
                            || !self
                                .run_controls
                                .debug_session()
                                .is_some_and(|(id, _)| self.run_controls.debug_target_active(id)),
                    )
                    .tooltip(t!("run.force_hint"))
                    .debug_selector(|| "debug-panel-force".into())
                    .on_click(cx.listener(|this, _, _, cx| this.force_debug(cx))),
            )
            .child(
                Button::new("debug-panel-hide")
                    .label(t!("run.debug_hide"))
                    .small()
                    .compact()
                    .ghost()
                    .debug_selector(|| "debug-panel-hide".into())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.debug_panel.open = false;
                        cx.notify();
                    })),
            );
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
        let mut frames = v_flex()
            .id("debug-panel-frames")
            .debug_selector(|| "debug-panel-frames".into())
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.debug_panel.frames)
            .child(div().font_semibold().child(t!("run.debug_stack")));
        for row in rows.frames {
            let frame = row.frame;
            let selector = format!("debug-panel-frame-{frame}");
            frames = frames.child(
                Button::new(selector.clone())
                    .label(row.label)
                    .small()
                    .ghost()
                    .content_full_width()
                    .when(row.selected, |button| button.text_color(cx.theme().primary))
                    .debug_selector(move || selector.clone())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_inspection_frame(frame, window, cx)
                    })),
            );
        }
        let mut variables = v_flex()
            .id("debug-panel-variables")
            .debug_selector(|| "debug-panel-variables".into())
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.debug_panel.variables)
            .child(div().font_semibold().child(t!("run.debug_locals")));
        if rows.variables.is_empty() {
            variables = variables.child(div().child(t!("run.debug_no_locals")));
        }
        for row in rows.variables {
            variables = variables.child(
                div()
                    .debug_selector(move || row.selector.clone())
                    .child(row.label),
            );
        }
        let mut panel = v_flex()
            .id("debug-panel")
            .debug_selector(|| "debug-panel".into())
            .w_full()
            .h(px(220.))
            .max_h(px(300.))
            .flex_shrink_0()
            .gap_1()
            .p_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_sm()
            .track_focus(&self.debug_panel.focus)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.debug_panel.focus.focus(window, cx);
                    // The surrounding editor also handles pointer focus; inspection owns this click.
                    cx.stop_propagation();
                }),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                let shift = event.keystroke.modifiers.shift;
                match (key, shift) {
                    ("f5", false) => this.debug_action("resume", cx),
                    ("f5", true) => this.debug_action("stop", cx),
                    ("f6", _) => this.debug_action("pause", cx),
                    ("f10", _) => this.step_debug(editor_core::DebugStep::Over, cx),
                    ("f11", false) => this.step_debug(editor_core::DebugStep::Into, cx),
                    ("f11", true) => this.step_debug(editor_core::DebugStep::Out, cx),
                    ("up" | "down", _) => {
                        let frames = this.run_controls.debug_frames();
                        if frames.is_empty() {
                            return;
                        }
                        let previous = frames
                            .iter()
                            .position(|frame| {
                                Some(frame.id) == this.run_controls.selected_debug_frame()
                            })
                            .unwrap_or(0);
                        let next = if key == "up" {
                            previous.saturating_sub(1)
                        } else {
                            (previous + 1).min(frames.len() - 1)
                        };
                        let frame = frames[next].id;
                        this.debug_panel.frames.scroll_to_item(next);
                        this.select_inspection_frame(frame, window, cx);
                    }
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .child(toolbar)
            .child(
                div()
                    .debug_selector(|| "debug-panel-state".into())
                    .child(status),
            )
            // The shared row must stretch panes to its bounded height. Center alignment leaves
            // each long list at its full content height, so a wheel has no viewport to scroll.
            .child(
                h_flex()
                    .items_stretch()
                    .flex_1()
                    .min_h_0()
                    .gap_3()
                    .child(breakpoints)
                    .child(frames)
                    .child(variables),
            );
        if rows.another_paused {
            panel = panel.child(div().child(t!("run.debug_other_paused")));
        }
        Some(panel.into_any_element())
    }
}
