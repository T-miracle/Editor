//! Conflict choices own a focused local modal; recovery force requires a second dirty-buffer confirmation.
use super::*;

impl EditorApp {
    fn choose_transfer_conflict(
        &mut self,
        choice: Choice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(prompt) = self.file_transfers.prompt.take() else {
            return;
        };
        let mut prompt = prompt;
        if choice == Choice::Force
            && !prompt.discard
            && prompt
                .affected
                .iter()
                .any(|path| self.dirty_transfer_target(path))
        {
            // The first force decision covers disk changes. Losing live edits needs a separate decision.
            prompt.discard = true;
            self.file_transfers.prompt = Some(prompt);
            cx.notify();
            return;
        }
        if choice == Choice::Replace && !prompt.merge && self.dirty_transfer_target(&prompt.target)
        {
            self.file_transfers.prompt = Some(prompt);
            cx.notify();
            return;
        }
        let _ = prompt.reply.send(Decision {
            choice,
            subsequent: prompt.subsequent,
            discard: choice == Choice::Force && prompt.discard,
            approvals: self.file_recovery_approvals(&prompt.affected),
        });
        if let Some(previous) = prompt.previous_focus {
            previous.focus(window, cx);
        }
        cx.notify();
    }

    /// UI painting stays in local controls; the explorer owns only choices and operation state.
    pub(crate) fn render_file_transfer(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut surface = div();
        if let Some(active) = &self.file_transfers.active
            && active.visible
        {
            let cancel = active.cancel.clone();
            surface = surface.child(ui::controls::file_operation::progress(
                t!(
                    "transfer.progress",
                    completed = active.completed,
                    bytes = active.bytes,
                    path = active.path.display()
                )
                .to_string(),
                move |_, _| {
                    cancel.store(true, Ordering::Relaxed);
                },
                cx,
            ));
        }
        if let Some(prompt) = &self.file_transfers.prompt {
            // Merge preserves unique files; dirty protection applies to colliding leaf files.
            let protected = !prompt.merge && self.dirty_transfer_target(&prompt.target);
            let owner = cx.entity().downgrade();
            let choices = if prompt.recovery {
                vec![
                    (
                        "transfer-skip",
                        t!("transfer.skip").to_string(),
                        Choice::Skip,
                        true,
                    ),
                    (
                        "transfer-force",
                        if prompt.discard {
                            t!("transfer.confirm_discard")
                        } else {
                            t!("transfer.force")
                        }
                        .to_string(),
                        Choice::Force,
                        true,
                    ),
                    (
                        "transfer-conflict-cancel",
                        t!("common.cancel").to_string(),
                        Choice::Cancel,
                        true,
                    ),
                ]
            } else {
                vec![
                    (
                        "transfer-skip",
                        t!("transfer.skip").to_string(),
                        Choice::Skip,
                        true,
                    ),
                    (
                        "transfer-keep-both",
                        t!("transfer.keep_both").to_string(),
                        Choice::KeepBoth,
                        true,
                    ),
                    (
                        "transfer-replace",
                        if prompt.merge {
                            t!("transfer.merge")
                        } else {
                            t!("transfer.replace")
                        }
                        .to_string(),
                        Choice::Replace,
                        !protected,
                    ),
                    (
                        "transfer-conflict-cancel",
                        t!("common.cancel").to_string(),
                        Choice::Cancel,
                        true,
                    ),
                ]
            }
            .into_iter()
            .map(|(id, label, choice, enabled)| {
                let owner = owner.clone();
                ui::controls::file_operation::FileChoice {
                    id,
                    label: label.into(),
                    enabled,
                    activate: Box::new(move |window, cx| {
                        let _ = owner.update(cx, |app, cx| {
                            app.choose_transfer_conflict(choice, window, cx)
                        });
                    }),
                }
            })
            .collect();
            let subsequent_owner = owner.clone();
            let mut details = vec![
                prompt.source.display().to_string(),
                prompt.target.display().to_string(),
            ];
            if protected {
                details.push(t!("transfer.dirty_target").to_string());
            }
            if prompt.recovery {
                details = prompt
                    .affected
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect();
                details.push(
                    if prompt.discard {
                        t!("transfer.discard_hint")
                    } else {
                        t!("transfer.recovery_hint")
                    }
                    .to_string(),
                );
            }
            surface = surface.child(ui::controls::file_operation::conflict_prompt(
                prompt.focus.clone(),
                if prompt.discard {
                    t!("transfer.discard_title")
                } else if prompt.recovery {
                    t!("transfer.recovery_conflict")
                } else {
                    t!("transfer.conflict")
                }
                .to_string(),
                details,
                choices,
                (!prompt.recovery).then_some((
                    prompt.subsequent,
                    Box::new(move |checked, cx| {
                        let _ = subsequent_owner.update(cx, |app, cx| {
                            if let Some(prompt) = &mut app.file_transfers.prompt {
                                prompt.subsequent = checked;
                            }
                            cx.notify();
                        });
                    }),
                )),
                move |window, cx| {
                    let _ = owner.update(cx, |app, cx| {
                        app.choose_transfer_conflict(Choice::Cancel, window, cx)
                    });
                },
                cx,
            ));
        }
        surface.into_any_element()
    }
}
