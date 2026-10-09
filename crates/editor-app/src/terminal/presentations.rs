//! An explicitly presented guest PTY joins its authenticated host task or a standalone native tab.
use super::*;
use crate::extensions::{HostWork, PresentedTerminalMessage};

impl TerminalPanel {
    /// Canonical debug snapshots are published by the parent, avoiding parent borrows in child render.
    pub(crate) fn publish_inspection(
        &mut self,
        key: String,
        model: crate::run::ui::panel::Inspection,
        parent: WeakEntity<EditorApp>,
        focus: FocusHandle,
        cx: &mut Context<Self>,
    ) {
        if let Some(view) = self.inspections.get(&key) {
            view.update(cx, |view, cx| view.publish(model, cx));
        } else {
            let view = cx.new(|_| crate::run::ui::panel::InspectionView::new(parent, model, focus));
            self.inspections.insert(key, view);
            cx.notify();
        }
    }
    /// Read-only resource lookup supports explicit host closure without revealing OS process handles.
    pub(crate) fn task_presentation(&self, key: &str) -> Option<protocol::api::ResourceHandle> {
        self.sessions
            .iter()
            .filter_map(|session| session.task.as_ref())
            .find(|task| task.key == key)?
            .presentation
            .clone()
    }
    /// The active tab determines which inspection is visible; dropdown selection cannot redirect it.
    pub(crate) fn active_task(&self) -> Option<String> {
        self.sessions
            .get(self.active_index()?)?
            .task
            .as_ref()
            .map(|task| task.key.clone())
    }
    /// Old rounds and closed views are refused before bytes can reach the retained Alacritty grid.
    pub(crate) fn observe_presentation(
        &mut self,
        message: PresentedTerminalMessage,
        cx: &mut Context<Self>,
    ) -> bool {
        let projection = message.presentation;
        let key = projection
            .owner
            .as_ref()
            .and_then(|owner| owner.configuration.clone())
            .unwrap_or_else(|| {
                format!(
                    "presented:{}:{}",
                    projection.handle.instance, projection.handle.resource
                )
            });
        let owned = projection
            .owner
            .as_ref()
            .is_some_and(|owner| owner.configuration.is_some());
        let index = self
            .sessions
            .iter()
            .position(|session| session.task.as_ref().is_some_and(|task| task.key == key));
        let newly_created = index.is_none();
        if index.is_none() {
            // An admitted host invocation already owns its tab. Late publications cannot recreate it.
            if owned || self.closed_tasks.contains(&key) {
                return false;
            }
            if !self.begin_task(&key, &projection.title, message.request_id, cx) {
                self.io_host
                    .read(cx)
                    .stage_host_run(HostWork::PresentedExit {
                        handle: projection.handle,
                        mode: protocol::process::ExitMode::Force,
                    });
                return false;
            }
        }
        let index = self
            .sessions
            .iter()
            .position(|session| session.task.as_ref().is_some_and(|task| task.key == key))
            .unwrap();
        let session = &mut self.sessions[index];
        let task = session.task.as_mut().unwrap();
        if owned && task.request != message.request_id {
            return false;
        }
        // A protocol adapter's explicitly decoded diagnostics share its invocation's task, while
        // its separate PTY remains the sole input and target-lifecycle binding.
        if owned && !projection.interactive {
            let id = session.id;
            if let Some(error) = projection.failure {
                self.report_error(FailureKind::Process, error);
            }
            for update in projection.updates {
                if matches!(update, Update::Output { .. }) {
                    self.apply_update(id, update, cx);
                }
            }
            cx.notify();
            return false;
        }
        if task
            .presentation
            .as_ref()
            .is_some_and(|handle| handle != &projection.handle)
        {
            return false;
        }
        let newly_bound = task.presentation.is_none();
        task.presentation = Some(projection.handle);
        task.presentation_input = projection.interactive;
        if !projection
            .updates
            .iter()
            .any(|update| matches!(update, Update::Exited { .. } | Update::Terminated))
        {
            task.process_alive = true;
            // Preparation may have finished before the PTY announcement arrived. A live, current
            // resource reopens input for this same round, without reviving closed or replaced tabs.
            session.exited = false;
        }
        let id = session.id;
        if let Some(error) = projection.failure {
            self.report_error(FailureKind::Process, error);
        }
        for update in projection.updates {
            self.apply_update(id, update, cx);
        }
        // Geometry is already driven by layout; output must not perpetually defer the native resize.
        if newly_bound && projection.interactive {
            self.resize_process(id, self.grid_size(), cx);
        }
        cx.notify();
        newly_created
    }
}
