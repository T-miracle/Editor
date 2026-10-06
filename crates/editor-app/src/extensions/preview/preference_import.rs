//! Successful private writes retire only their matching legacy session bundles; failed imports retry later.
use super::*;

impl EditorApp {
    /// Drain this worker's bounded per-owner receipts outside its mutex before saving the session.
    pub(in crate::extensions) fn finish_preference_imports(&mut self, cx: &App) {
        let worker = &self.extensions.read(cx).worker;
        let receipts = std::mem::take(&mut worker.state.lock().unwrap().preference_imports);
        let mut changed = false;
        for receipt in receipts {
            if receipt.succeeded && receipt.workspace == self.session_state.workspace {
                changed |= self
                    .session_state
                    .acknowledge_display_import(&receipt.owner, &receipt.data);
            }
        }
        if changed {
            self.persist_session();
        }
    }
}
