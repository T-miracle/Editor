//! The run coordinator feeds one native terminal view per configuration, independent of each child.

use super::*;

impl EditorApp {
    /// All controls bind to their selected task, including stdio jobs and public consumer executions.
    pub(crate) fn terminal_task_action(
        &mut self,
        key: &str,
        execution: Option<u64>,
        action: crate::terminal::task_view::TaskAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::terminal::task_view::TaskAction;
        if matches!(action, TaskAction::Reload) {
            self.plugin_configuration_bridge.jobs.reload(key);
        } else if matches!(action, TaskAction::Rerun) {
            let workspace = self.workspace_key();
            self.run_controls.select(key, &workspace);
            self.rerun_selected(window, cx);
        } else if matches!(action, TaskAction::Stop) {
            self.plugin_configuration_bridge.rerun_jobs.remove(key);
            self.stop_terminal_task(key, execution, cx);
        } else {
            self.plugin_configuration_bridge.rerun_jobs.remove(key);
            if self.run_controls.configuration(key).is_some() {
                let workspace = self.workspace_key();
                self.run_controls.select(key, &workspace);
                self.terminate_selected_run(cx);
            } else if let Some(session) = execution {
                self.extensions.read(cx).stage_host_run(Work::StopRun {
                    session,
                    config: key.into(),
                    mode: plugin_runtime::plugin_protocol::process::ExitMode::Force,
                    request_id: 0,
                });
            }
        }
        cx.notify();
    }
    /// Admission owns the clear point; repeated active clicks only reveal the retained task tab.
    pub(in crate::run::ui) fn begin_terminal_task(
        &mut self,
        key: &str,
        name: &str,
        request: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let accepted = self
            .terminal
            .update(cx, |panel, cx| panel.begin_task(key, name, request, cx));
        if accepted {
            self.reveal_terminal(window, cx);
        }
        accepted
    }

    /// Select the built-in leaf without the empty-panel action that creates a Shell.
    pub(crate) fn reveal_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_terminal_dock(window, cx);
        let id = gpui_base::dock::PanelId::from(self.terminal.entity_id());
        self.dock_area.update(cx, |area, cx| {
            if !area.is_dock_open(gpui_base::dock::DockPlacement::Bottom) {
                area.toggle_dock(gpui_base::dock::DockPlacement::Bottom, window, cx);
            }
            area.select_panel(id, window, cx);
        });
        cx.notify();
    }

    /// Drain bytes before advancing preparation, so the last build output precedes the target header.
    pub(super) fn sync_terminal_messages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let messages = self.extensions.read(cx).take_native_execution_updates();
        let reveal = messages.iter().any(|message| message.update.locate);
        for message in messages {
            self.terminal
                .update(cx, |panel, cx| panel.observe_execution(message, cx));
        }
        if reveal && self.terminal.read(cx).visible() {
            self.reveal_terminal(window, cx);
        }
    }

    /// Shared snapshots are presentation data; the coordinator retains cancellation and actual exit authority.
    pub(super) fn sync_terminal_results(&mut self, cx: &mut Context<Self>) {
        let keys = self.terminal.read(cx).task_keys();
        let executions = self.extensions.read(cx).host_run_snapshots();
        for (key, execution) in keys {
            let report = self.plugin_configuration_bridge.jobs.report(&key);
            let controls = crate::terminal::task_view::TaskControls {
                rerun: self.run_controls.configuration(&key).is_some(),
                reload: self
                    .run_controls
                    .configuration_set()
                    .plugin_configurations
                    .get(&key)
                    .is_some_and(|config| {
                        config.template == "development"
                            && config.provider == crate::plugin_development::configuration::PROVIDER
                    }),
                state: report
                    .as_ref()
                    .map(|report| report.snapshot.state)
                    .or_else(|| {
                        self.run_controls
                            .running_for(&key)
                            .map(|session| session.state)
                    })
                    .or_else(|| {
                        execution.and_then(|id| {
                            executions
                                .iter()
                                .find(|session| session.id == id)
                                .map(|session| session.state)
                        })
                    }),
            };
            self.terminal
                .update(cx, |panel, cx| panel.task_controls(&key, controls, cx));
            if let Some(report) = &report {
                self.terminal.update(cx, |panel, cx| {
                    panel.task_snapshot(&key, "host-job", &report.output, cx)
                });
            }
            let active = self.run_controls.is_pending(&key)
                || self.run_controls.is_preparing(&key)
                || self
                    .run_controls
                    .running_for(&key)
                    .is_some_and(|session| session.is_active())
                || self.run_controls.debug_target_active(&key)
                || report.is_some_and(|report| report.snapshot.state.is_active())
                || execution.is_some_and(|id| {
                    executions
                        .iter()
                        .any(|session| session.id == id && session.state.is_active())
                });
            if !active {
                self.terminal
                    .update(cx, |panel, cx| panel.finish_task(&key, cx));
            }
        }
    }

    /// Retained provider output belongs to the current preparation before its completion advances it.
    pub(super) fn sync_terminal_preparation_output(&mut self, cx: &mut Context<Self>) {
        for (key, _) in self.terminal.read(cx).task_keys() {
            for (source, text) in self.run_controls.preparation_snapshots(&key) {
                self.terminal.update(cx, |panel, cx| {
                    panel.task_snapshot(&key, &source, &text, cx)
                });
            }
        }
    }

    /// Persist each completed step's result in its own task round, including silent failures.
    /// Snapshot cursors prevent repeated frames from appending the same result; output is drained
    /// first and the next step's header is added only after this method records its predecessor.
    pub(super) fn sync_terminal_step_results(&mut self, cx: &mut Context<Self>) {
        use crate::run::sequence::StepState;
        for (key, _) in self.terminal.read(cx).task_keys() {
            let Some(sequence) = self.run_controls.preparation(&key) else {
                continue;
            };
            for (index, step) in sequence.steps().iter().enumerate() {
                let status = match &step.state {
                    StepState::Succeeded => run_state_label(plugin_runtime::ExecutionState::Exited),
                    StepState::Stopped => t!("run.preparation_stopped").to_string(),
                    StepState::Failed { reason } => t!(
                        "run.step_failed",
                        name = step.name.clone(),
                        message = reason.clone()
                    )
                    .to_string(),
                    _ => continue,
                };
                let text = format!("\r\n[{}] {}\r\n", step.name, status);
                self.terminal.update(cx, |panel, cx| {
                    panel.task_snapshot(&key, &format!("step-result:{index}"), &text, cx)
                });
            }
        }
    }

    /// Locating preparation and stdio jobs needs no new launch receipt and never clears the grid.
    pub(crate) fn locate_terminal_task(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let found = self
            .terminal
            .update(cx, |panel, cx| panel.locate_task(key, cx));
        if found {
            self.reveal_terminal(window, cx);
        }
    }

    /// Task close/Stop retains its own configuration even when the top-level selection changed meanwhile.
    pub(crate) fn stop_terminal_task(
        &mut self,
        key: &str,
        execution: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        // Confirming Close revokes a queued stdio rerun as well as the currently owned process.
        self.plugin_configuration_bridge.rerun_jobs.remove(key);
        let workspace = self.workspace_key();
        if self.run_controls.configuration(key).is_some() {
            self.run_controls.select(key, &workspace);
            self.request_selected_stop(
                plugin_runtime::plugin_protocol::process::ExitMode::Graceful,
                cx,
            );
        } else if let Some(session) = execution {
            self.extensions.read(cx).stage_host_run(Work::StopRun {
                session,
                config: key.into(),
                mode: plugin_runtime::plugin_protocol::process::ExitMode::Graceful,
                request_id: 0,
            });
        }
    }
}
