//! Local inline controls and fixed decision prompts for shortcut editing.

use super::*;
use crate::app::shortcuts::catalog::Target;
use crate::ui::controls::shortcut_keycaps;

impl ShortcutPanel {
    /// Show separate edit/delete controls per sequence plus operation-level add and restore.
    pub(in crate::app::shortcuts) fn binding_controls(
        &self,
        operation: &Operation,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // Bound controls wrap inside the right column instead of pushing descriptions offscreen.
        let mut row = div()
            .flex()
            .items_center()
            .flex_wrap()
            .max_w(px(360.))
            .gap_1();
        // A failed profile load preserves read-only discovery and the original keymap.
        let disabled = self.confirm.is_some() || !cx.has_global::<BindingEngine>();
        let stem = match &operation.target {
            Target::Native { action, .. } => action.name().to_owned(),
            Target::Plugin { plugin, command } => format!("{plugin}/{command}"),
        };
        for (index, binding) in operation.defaults.iter().enumerate() {
            let id = operation.id.clone();
            let debug = format!("shortcut-binding-{stem}-{index}");
            row = row.child(
                Button::new(format!("edit-{}-{index}", operation.id))
                    .disabled(disabled)
                    .debug_selector(move || debug.clone())
                    .small()
                    .ghost()
                    .accessibility_label(t!("shortcuts.edit.modify").to_string())
                    .child(shortcut_keycaps(&capture::display(binding), cx))
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        panel.request_edit_intent(
                            Intent::Edit {
                                id: id.clone(),
                                index: Some(index),
                            },
                            window,
                            cx,
                        )
                    })),
            );
            let id = operation.id.clone();
            let debug = format!("shortcut-delete-{stem}-{index}");
            row = row.child(
                Button::new(format!("delete-{}-{index}", operation.id))
                    .disabled(disabled)
                    .debug_selector(move || debug.clone())
                    .small()
                    .ghost()
                    .label("−")
                    .accessibility_label(t!("shortcuts.edit.delete").to_string())
                    .tooltip(t!("shortcuts.edit.delete").to_string())
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        panel.request_edit_intent(
                            Intent::Remove {
                                id: id.clone(),
                                index,
                            },
                            window,
                            cx,
                        )
                    })),
            );
        }
        if operation.defaults.is_empty() {
            let id = operation.id.clone();
            row = row.child(
                Button::new(format!("unbound-{}", operation.id))
                    .disabled(disabled)
                    .small()
                    .ghost()
                    .label(t!("shortcuts.unbound").to_string())
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        panel.request_edit_intent(
                            Intent::Edit {
                                id: id.clone(),
                                index: None,
                            },
                            window,
                            cx,
                        );
                    })),
            );
        }
        let id = operation.id.clone();
        let add_debug = format!("shortcut-add-{stem}");
        row = row.child(
            Button::new(format!("add-{}", operation.id))
                .disabled(disabled)
                .debug_selector(move || add_debug.clone())
                .small()
                .ghost()
                .label("+")
                .accessibility_label(t!("shortcuts.edit.add").to_string())
                .tooltip(t!("shortcuts.edit.add").to_string())
                .on_click(cx.listener(move |panel, _, window, cx| {
                    panel.request_edit_intent(
                        Intent::Edit {
                            id: id.clone(),
                            index: None,
                        },
                        window,
                        cx,
                    )
                })),
        );
        let id = operation.id.clone();
        let restore_debug = format!("shortcut-restore-{stem}");
        row.child(
            Button::new(format!("restore-{}", operation.id))
                .disabled(disabled)
                .debug_selector(move || restore_debug.clone())
                .small()
                .ghost()
                .label("↺")
                .accessibility_label(t!("shortcuts.edit.restore").to_string())
                .tooltip(t!("shortcuts.edit.restore").to_string())
                .on_click(cx.listener(move |panel, _, window, cx| {
                    panel.request_edit_intent(Intent::Restore(id.clone()), window, cx)
                })),
        )
        .into_any_element()
    }

    /// Render capture and explicit save/cancel beneath the matching stable operation row.
    pub(in crate::app::shortcuts) fn render_draft(
        &self,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let draft = self.draft.as_ref().filter(|draft| draft.id == id)?;
        let label = if draft.capture.strokes.is_empty() {
            div()
                .child(t!("shortcuts.press_keys").to_string())
                .into_any_element()
        } else {
            shortcut_keycaps(&capture::display(&draft.capture.strokes), cx)
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    Button::new("shortcuts-edit-capture")
                        .disabled(self.confirm.is_some())
                        .debug_selector(|| "shortcuts-edit-capture".into())
                        .outline()
                        .content_full_width()
                        .w_full()
                        .child(label)
                        .accessibility_label(t!("shortcuts.edit.record").to_string())
                        .on_click(cx.listener(|panel, _, window, cx| {
                            if let Some(draft) = panel.draft.as_mut() {
                                draft.capture.clear();
                            }
                            panel.edit_error = None;
                            panel.focus.focus(window, cx);
                            cx.notify();
                        })),
                )
                .when_some(self.edit_error.clone(), |row, error| {
                    row.child(
                        div()
                            .debug_selector(|| "shortcuts-edit-error".into())
                            .text_color(cx.theme().danger)
                            .child(error),
                    )
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("shortcuts-edit-cancel")
                                .disabled(self.confirm.is_some())
                                .debug_selector(|| "shortcuts-edit-cancel".into())
                                .small()
                                .label(t!("shortcuts.edit.cancel").to_string())
                                .on_click(cx.listener(|panel, _, window, cx| {
                                    panel.cancel_draft(window, cx)
                                })),
                        )
                        .child(
                            Button::new("shortcuts-edit-save")
                                .debug_selector(|| "shortcuts-edit-save".into())
                                .track_focus(&draft.save_focus)
                                .small()
                                .primary()
                                .label(t!("shortcuts.edit.save").to_string())
                                .disabled(
                                    self.confirm.is_some()
                                        || !cx.has_global::<BindingEngine>()
                                        || draft.capture.strokes.is_empty()
                                        || draft.capture.waiting,
                                )
                                .on_click(
                                    cx.listener(|panel, _, window, cx| {
                                        panel.save_draft(window, cx)
                                    }),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }

    /// Render a fixed decision region; the caller places it outside the scrolling rows.
    pub(in crate::app::shortcuts) fn render_edit_confirmation(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let Some(confirmation) = self.confirm.as_ref() else {
            // Restore/delete can fail without an inline draft, so their errors need a visible home.
            return self
                .edit_error
                .as_ref()
                .filter(|_| self.draft.is_none())
                .map(|error| {
                    div()
                        .debug_selector(|| "shortcuts-edit-error".into())
                        .flex_shrink_0()
                        .p_3()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(error.clone())
                        .into_any_element()
                });
        };
        let replacing = matches!(confirmation, Confirmation::Replace { .. });
        let mut body = div()
            .id("shortcuts-edit-confirmation")
            .debug_selector(|| "shortcuts-edit-confirmation".into())
            .flex()
            .flex_col()
            .flex_shrink_0()
            .p_3()
            .gap_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().popover)
            .text_sm()
            .child(
                t!(if replacing {
                    "shortcuts.edit.conflict"
                } else {
                    "shortcuts.edit.unsaved"
                })
                .to_string(),
            );
        if let Confirmation::Replace { conflicts, .. } = confirmation {
            body = body.child(
                div()
                    .id("shortcuts-conflicts")
                    .max_h(px(110.))
                    .overflow_y_scroll()
                    .children(conflicts.iter().map(|conflict| {
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(conflict.title.clone())
                            .child(shortcut_keycaps(&capture::display(&conflict.binding), cx))
                    })),
            );
        }
        Some(
            body.child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("shortcuts-edit-continue")
                            .debug_selector(|| "shortcuts-edit-continue".into())
                            .small()
                            .label(
                                t!(if replacing {
                                    "shortcuts.edit.cancel"
                                } else {
                                    "shortcuts.edit.continue"
                                })
                                .to_string(),
                            )
                            .on_click(cx.listener(|panel, _, window, cx| {
                                panel.confirm = None;
                                panel.resume_draft(window, cx);
                            })),
                    )
                    .child(
                        Button::new(if replacing {
                            "shortcuts-edit-replace"
                        } else {
                            "shortcuts-edit-discard"
                        })
                        .debug_selector(move || {
                            if replacing {
                                "shortcuts-edit-replace"
                            } else {
                                "shortcuts-edit-discard"
                            }
                            .into()
                        })
                        .small()
                        .primary()
                        .label(
                            t!(if replacing {
                                "shortcuts.edit.replace"
                            } else {
                                "shortcuts.edit.discard"
                            })
                            .to_string(),
                        )
                        .on_click(cx.listener(|panel, _, window, cx| {
                            match panel.confirm.take() {
                                Some(Confirmation::Leave(intent)) => {
                                    panel.draft = None;
                                    panel.perform_edit_intent(intent, window, cx);
                                }
                                Some(Confirmation::Replace {
                                    mutation, revision, ..
                                }) => panel.commit_mutation(mutation, true, revision, window, cx),
                                None => {}
                            }
                            cx.notify();
                        })),
                    ),
            )
            .into_any_element(),
        )
    }
}
