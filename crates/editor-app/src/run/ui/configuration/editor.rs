//! Same-window detail editing and unsaved navigation decisions over the retained form.
use super::*;
use crate::app::messages::MessageLevel;

/// Detail content replaces the main card inside its owning modal, retaining all collapsed inputs.
pub(super) fn render_editor(
    app: &Entity<EditorApp>,
    form: &Entity<RunConfigForm>,
    cx: &gpui_kit::App,
) -> AnyElement {
    let state = form.read(cx);
    let fields = match state.editor.as_ref().unwrap() {
        FormEditor::Field { field, .. } => render_field(form, *field, field.label(), cx),
        FormEditor::Step { state, .. } => StepEditor::render_form(state, app, cx),
    };
    let cancel = app.clone();
    let apply = app.clone();
    v_flex()
        .size_full()
        .min_h_0()
        .child(
            v_flex()
                .id("run-config-detail-body")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_4()
                .gap_3()
                .child(fields)
                .when_some(state.error.clone(), |body, error| {
                    body.child(div().text_color(cx.theme().danger).child(error))
                }),
        )
        .child(
            h_flex()
                .flex_shrink_0()
                .justify_end()
                .gap_2()
                .p_3()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(
                    Button::new("run-config-detail-cancel")
                        .debug_selector(|| "run-config-detail-cancel".into())
                        .label(t!("run.form_cancel"))
                        .on_click(move |_, window, cx| {
                            cancel.update(cx, |state, cx| {
                                state.cancel_run_form(window, cx);
                            });
                        }),
                )
                .child(
                    Button::new("run-config-detail-done")
                        .debug_selector(|| "run-config-detail-done".into())
                        .primary()
                        .label(t!("run.form_done"))
                        .on_click(move |_, window, cx| {
                            apply.update(cx, |state, cx| state.finish_run_editor(window, cx))
                        }),
                ),
        )
        .into_any_element()
}

/// A decision is shown only after user input would be lost, or before an explicit deletion.
pub(super) fn render_navigation_confirmation(
    app: &Entity<EditorApp>,
    form: &Entity<RunConfigForm>,
    cx: &gpui_kit::App,
) -> AnyElement {
    let deleting = matches!(form.read(cx).pending_selection, Some(FormSelection::Delete));
    let cancel = app.clone();
    let discard = app.clone();
    let save = app.clone();
    v_flex()
        .size_full()
        .min_h_0()
        .p_4()
        .gap_3()
        .child(div().flex_1().child(if deleting {
            t!("run.form_delete_question")
        } else {
            t!("run.form_switch_question")
        }))
        .child(
            h_flex()
                .flex_wrap()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("run-config-switch-cancel")
                        .debug_selector(|| "run-config-switch-cancel".into())
                        .label(t!("run.form_cancel"))
                        .on_click(move |_, window, cx| {
                            cancel.update(cx, |state, cx| {
                                state.cancel_run_form(window, cx);
                            });
                        }),
                )
                .child(
                    Button::new("run-config-switch-discard")
                        .debug_selector(|| "run-config-switch-discard".into())
                        .label(if deleting {
                            t!("run.form_delete")
                        } else {
                            t!("run.form_discard")
                        })
                        .on_click(move |_, window, cx| {
                            discard.update(cx, |state, cx| {
                                let selection = state
                                    .run_form
                                    .as_ref()
                                    .unwrap()
                                    .update(cx, |form, _| form.pending_selection.take())
                                    .unwrap();
                                state.apply_run_form_selection(selection, window, cx);
                            })
                        }),
                )
                .when(!deleting, |buttons| {
                    buttons.child(
                        Button::new("run-config-switch-save")
                            .debug_selector(|| "run-config-switch-save".into())
                            .primary()
                            .label(t!("run.form_save_short"))
                            .on_click(move |_, window, cx| {
                                save.update(cx, |state, cx| state.commit_run_form(window, cx))
                            }),
                    )
                }),
        )
        .into_any_element()
}

impl EditorApp {
    /// Open the rejected optional field, or reveal and focus its main-form native input.
    pub(in crate::run::ui) fn reject_run_form(
        &mut self,
        field: RunField,
        message: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = self.run_form.clone() else {
            return;
        };
        form.update(cx, |form, cx| {
            form.pending_selection = None;
            if matches!(field, RunField::Build | RunField::Prelaunch) {
                form.startup_open = true;
            }
            if matches!(
                field,
                RunField::Directory
                    | RunField::Environment
                    | RunField::ToolPaths
                    | RunField::Breakpoints
            ) {
                form.more_open = true;
            }
            cx.notify();
        });
        if matches!(
            field,
            RunField::Arguments
                | RunField::Environment
                | RunField::ToolPaths
                | RunField::Breakpoints
        ) {
            self.open_run_field_editor(field, window, cx);
        } else if let Some((_, input)) = form.read(cx).inputs.iter().find(|(key, _)| *key == field)
        {
            let focus = input.read(cx).focus_handle(cx);
            focus.focus(window, cx);
        }
        // Native form validation is a host warning; provider validation messages retain their own logs.
        self.report_host_message(MessageLevel::Warning, message.clone(), cx);
        form.update(cx, |form, cx| {
            form.error = Some(message);
            cx.notify();
        });
        cx.notify();
    }

    /// Open an optional field through its retained native state and remember where focus returns.
    pub(in crate::run::ui) fn open_run_field_editor(
        &mut self,
        field: RunField,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = self.run_form.clone() else {
            return;
        };
        form.update(cx, |form, cx| {
            form.return_focus = window.focused(cx);
            form.editor = Some(FormEditor::Field {
                field,
                original: form.text(field, cx),
            });
            form.picker = None;
            if let Some((_, input)) = form.textareas.iter().find(|(key, _)| *key == field) {
                input.read(cx).focus_handle(cx).focus(window, cx);
            }
            cx.notify();
        });
        cx.notify();
    }

    /// A row edit stages typed fields separately; an added row is inserted only after validation.
    pub(in crate::run::ui) fn open_run_step_editor(
        &mut self,
        field: RunField,
        index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = self.run_form.clone() else {
            return;
        };
        let parsed = index
            .map(|index| {
                crate::run::parse_steps(&form.read(cx).rows_of(field)[index].1.read(cx).value())
            })
            .transpose();
        let step = match parsed {
            Ok(Some(mut rows)) => rows.pop(),
            Ok(None) => None,
            Err(message) => {
                // Parsing an explicitly opened native row is a user-facing host validation result.
                self.record_host_message(MessageLevel::Warning, message.clone(), cx);
                form.update(cx, |form, cx| {
                    form.error = Some(message);
                    cx.notify();
                });
                return;
            }
        };
        let editor = cx.new(|cx| StepEditor::new(step, window, cx));
        form.update(cx, |form, cx| {
            form.return_focus = window.focused(cx);
            form._subscriptions
                .push(cx.observe(&editor, |_, _, cx| cx.notify()));
            let focus = editor.read(cx).focus_handle(cx);
            focus.focus(window, cx);
            form.editor = Some(FormEditor::Step {
                field,
                index,
                state: editor,
            });
            form.picker = None;
            cx.notify();
        });
        cx.notify();
    }

    /// Accept only the active detail edit; this operation does not save or run the configuration.
    pub(in crate::run::ui) fn finish_run_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = self.run_form.clone() else {
            return;
        };
        let row = match form.read(cx).editor.as_ref() {
            Some(FormEditor::Step { state, .. }) => Some(state.read(cx).row_text(cx)),
            _ => None,
        };
        if let Some(Err(message)) = row {
            // Only the explicit Done action publishes validation; ordinary field edits stay transient.
            self.record_host_message(MessageLevel::Warning, message.clone(), cx);
            if let Some(FormEditor::Step { state, .. }) = &form.read(cx).editor {
                state
                    .clone()
                    .update(cx, |state, cx| state.reject(message, cx));
            }
            return;
        }
        form.update(cx, |form, cx| {
            if let Some(FormEditor::Step { field, index, .. }) = form.editor.take() {
                let text = row.unwrap().unwrap();
                if let Some(index) = index {
                    form.rows_of(field)[index]
                        .1
                        .update(cx, |input, cx| input.set_value(text, window, cx));
                } else {
                    let input = cx.new(|cx| InputState::new(window, cx).default_value(text));
                    form.rows.push((field, input));
                }
                form.sync_rows(field, cx);
            }
            if let Some(focus) = form.return_focus.take() {
                focus.focus(window, cx);
            }
            form.error = None;
            cx.notify();
        });
        cx.notify();
    }
}
