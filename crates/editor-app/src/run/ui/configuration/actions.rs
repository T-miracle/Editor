//! Navigation, copy and explicit delete decisions preserve unsaved input and storage rollback.
use super::*;
use crate::app::messages::MessageLevel;
impl EditorApp {
    /// Selecting defaults is reversible; changed native fields require an explicit navigation decision.
    pub(in crate::run::ui) fn select_run_form(
        &mut self,
        selection: FormSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = self.run_form.clone() else {
            return;
        };
        if matches!(&selection,FormSelection::Configuration(id) if id==&form.read(cx).draft.id)
            || matches!(&selection,FormSelection::Candidate(id) if form.read(cx).draft.from_target.as_ref()==Some(id))
        {
            return;
        }
        if matches!(selection, FormSelection::Delete)
            || (!matches!(selection, FormSelection::Copy) && form.read(cx).modified(cx))
        {
            form.update(cx, |form, cx| {
                form.pending_selection = Some(selection);
                form.picker = None;
                cx.notify();
            });
        } else {
            self.apply_run_form_selection(selection, window, cx);
        }
        cx.notify();
    }

    /// Apply an explicit switch after its guard; no ordinary selection invokes persistence.
    pub(in crate::run::ui) fn apply_run_form_selection(
        &mut self,
        selection: FormSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = self.workspace_key();
        // Only an explicit confirmed deletion persists here; selecting or copying remains a draft.
        let deleted_name = matches!(&selection, FormSelection::Delete)
            .then(|| self.run_form.as_ref().unwrap().read(cx).draft.name.clone());
        let result = match selection {
            FormSelection::Configuration(id) => self
                .run_controls
                .configuration(&id)
                .map(|config| RunConfigDraft::from_config(Some(config), id))
                .ok_or_else(|| t!("run.target_config_missing").to_string()),
            FormSelection::Candidate(id) => self
                .run_controls
                .target_template(&id, &key)
                .map(|config| RunConfigDraft::from_config(Some(&config), config.id.clone())),
            FormSelection::New(shell) => {
                let mut draft =
                    RunConfigDraft::from_config(None, self.run_controls.generate_id(&key));
                draft.shell = shell;
                Ok(draft)
            }
            FormSelection::Copy => {
                let mut draft = self.run_form.as_ref().unwrap().read(cx).snapshot(cx);
                draft.id = self.run_controls.generate_id(&key);
                draft.name = t!("run.form_copy_name", name = draft.name).into();
                draft.from_target = None;
                draft.source = editor_core::RunConfigSource::Local;
                draft.share = false;
                Ok(draft)
            }
            FormSelection::Delete => {
                let id = self.run_form.as_ref().unwrap().read(cx).draft.id.clone();
                match self.run_controls.remove(&id, &key) {
                    Ok(()) => Ok(RunConfigDraft::from_config(
                        self.run_controls.selected(),
                        self.run_controls.generate_id(&key),
                    )),
                    Err(message) => Err(message),
                }
            }
        };
        match result {
            Ok(draft) => {
                let form = cx.new(|cx| {
                    let mut form = RunConfigForm::from_draft(draft, window, cx);
                    form.manual_target = form.draft.provided.is_none();
                    form
                });
                let focus = form
                    .read(cx)
                    .inputs
                    .iter()
                    .find(|(field, _)| *field == RunField::Name)
                    .unwrap()
                    .1
                    .read(cx)
                    .focus_handle(cx);
                self.run_form = Some(form);
                focus.focus(window, cx);
            }
            Err(message) => {
                // Preserve the inline refusal while retaining the result after the form is dismissed.
                self.record_host_message(
                    if deleted_name.is_some() {
                        MessageLevel::Error
                    } else {
                        MessageLevel::Warning
                    },
                    message.clone(),
                    cx,
                );
                if let Some(form) = &self.run_form {
                    form.update(cx, |form, cx| {
                        form.pending_selection = None;
                        form.error = Some(message);
                        cx.notify();
                    });
                }
            }
        }
        cx.notify();
    }

    /// Cancel a detail edit or switch first; only the outermost cancellation drops the whole draft.
    pub(crate) fn cancel_run_form(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(form) = self.run_form.clone() else {
            return true;
        };
        let handled = form.update(cx, |form, cx| {
            if form.editor.is_some() {
                if let Some(FormEditor::Field { field, original }) = form.editor.take() {
                    form.set_text(field, original, window, cx);
                }
                if let Some(focus) = form.return_focus.take() {
                    focus.focus(window, cx);
                }
                form.error = None;
                cx.notify();
                return true;
            }
            if form.pending_selection.take().is_some() {
                cx.notify();
                return true;
            }
            false
        });
        if handled {
            cx.notify();
            false
        } else {
            self.close_run_form(cx);
            true
        }
    }
}
