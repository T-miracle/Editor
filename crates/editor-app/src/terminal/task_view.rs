//! Task controls use the selected tab's immutable owner, independently from the top run dropdown.

use super::*;
use gpui_kit::AnyElement;

/// The run coordinator, rather than the view, owns stop barriers and new-round admission.
#[derive(Clone, Copy)]
pub(crate) enum TaskAction {
    Stop,
    Force,
    Rerun,
    Reload,
}

/// Retained control state is a read-only publication; the run coordinator owns all actual sessions.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct TaskControls {
    pub rerun: bool,
    pub reload: bool,
    pub state: Option<plugin_runtime::ExecutionState>,
}

impl TerminalPanel {
    /// Update only meaningful state changes, keeping output frames from invalidating native buttons.
    pub(crate) fn task_controls(
        &mut self,
        key: &str,
        controls: TaskControls,
        cx: &mut Context<Self>,
    ) {
        if let Some(task) = self
            .sessions
            .iter_mut()
            .find(|session| session.task.as_ref().is_some_and(|task| task.key == key))
            .and_then(|session| session.task.as_mut())
            && task.controls != controls
        {
            task.controls = controls;
            cx.notify();
        }
    }
    /// Built-in task controls share the terminal title and never allocate another output panel.
    pub(super) fn task_toolbar(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let session = &self.sessions[self.active_index()?];
        let task = session.task.as_ref()?;
        let parent = self.parent.upgrade()?;
        let reload = task.controls.reload;
        let rerun = task.controls.rerun;
        let active = !session.exited;
        let key = task.key.clone();
        let execution = task.execution;
        let status = task.controls.state;
        let mut row = h_flex().gap_1().items_center();
        if let Some(status) = status {
            row = row.child(crate::run::ui::run_state_label(status));
        }
        for (id, label, action, enabled) in [
            (
                "terminal-task-stop",
                t!("run.stop"),
                TaskAction::Stop,
                active,
            ),
            (
                "terminal-task-force",
                t!("run.force"),
                TaskAction::Force,
                active,
            ),
            (
                "terminal-task-rerun",
                t!("run.rerun"),
                TaskAction::Rerun,
                rerun,
            ),
            (
                "terminal-task-reload",
                t!("plugin_dev.reload"),
                TaskAction::Reload,
                reload && active,
            ),
        ] {
            if matches!(action, TaskAction::Reload) && !reload {
                continue;
            }
            let parent = parent.downgrade();
            let key = key.clone();
            row = row.child(
                Button::new(id)
                    .debug_selector(move || id.into())
                    .label(label)
                    .small()
                    .compact()
                    .ghost()
                    .disabled(!enabled)
                    .on_click(move |_, window, cx| {
                        let handle = window.window_handle();
                        let parent = parent.clone();
                        let key = key.clone();
                        // End the child's mutable borrow before the coordinator updates this same panel.
                        cx.defer(move |cx| {
                            let _ = handle.update(cx, |_, window, cx| {
                                let _ = parent.update(cx, |app, cx| {
                                    app.terminal_task_action(&key, execution, action, window, cx)
                                });
                            });
                        });
                    }),
            );
        }
        Some(row.into_any_element())
    }
}
