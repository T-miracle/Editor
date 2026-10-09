//! Conventional rename input and cancellable proposals share the native document transaction gate.
use super::*;

impl EditorApp {
    /// Prepare the ordinary rename field; an unsupported or unknown semantic target produces no edits.
    pub(crate) fn rename_symbol_action(
        &mut self,
        _: &RenameSymbol,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.reject_readonly_document_action(window, cx) {
            return;
        }
        if self.wait_for_native_input(DocumentAction::Rename, window, cx) {
            return;
        }
        let Some(index) = self.active_text_tab_index() else {
            return;
        };
        let language = language_for_path(self.tabs[index].path());
        let Some(server) = self
            .language_servers
            .get(&language)
            .filter(|server| server.provides_editing())
            .cloned()
        else {
            self.status = t!("editor.rename_unavailable").to_string();
            cx.notify();
            return;
        };
        let Ok(document) = self.plugin_document_version(index) else {
            self.status = t!("editor.editing_stale").to_string();
            cx.notify();
            return;
        };
        let source = self.editor.read(cx).text().to_string();
        let cursor = self.editor.read(cx).cursor();
        let position = navigation::position_at_byte(&source, cursor);
        let Some(uri) = navigation::file_uri(self.tabs[index].path()) else {
            return;
        };
        let lease = server.open_document(uri);
        let captured_lease = lease.clone();
        let active = server.clone();
        let snapshot = source.clone();
        self.language_edits.cancel_rename();
        self.language_edits.rename_pending = true;
        let request = self.language_edits.rename_request;
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .scheduler_executor()
                .spawn_dedicated(move |_| async move {
                    server
                        .prepare_rename_for(lease, snapshot, position)
                        .map(|response| response)
                })
                .await;
            let _ = this.update_in(cx, |app, window, cx| {
                if app.language_edits.rename_request != request {
                    return;
                }
                app.language_edits.rename_pending = false;
                if app
                    .active_text_tab_index()
                    .and_then(|index| app.plugin_document_version(index).ok())
                    .as_ref()
                    != Some(&document)
                    || app.editor.read(cx).cursor() != cursor
                    || !active.is_active()
                    || !app
                        .language_servers
                        .get(&language)
                        .is_some_and(|current| Arc::ptr_eq(current, &active))
                {
                    return;
                }
                let name = match result {
                    Ok(Some(PrepareRenameResponse::Range(range))) => {
                        byte_at_position(&source, range.start)
                            .and_then(|start| {
                                byte_at_position(&source, range.end).map(|end| (start, end))
                            })
                            .ok()
                            .and_then(|(start, end)| source.get(start..end))
                            .unwrap_or("")
                            .to_string()
                    }
                    Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
                        placeholder, ..
                    })) => placeholder,
                    Ok(Some(_)) => app.editor.read(cx).selected_text().to_string(),
                    _ => {
                        app.status = t!("editor.rename_unavailable").to_string();
                        cx.notify();
                        return;
                    }
                };
                let input = cx.new(|cx| InputState::new(window, cx).default_value(name));
                input.update(cx, |input, cx| {
                    input.focus(window, cx);
                    input.set_selected_range(0..input.text().len(), cx);
                });
                app.language_edits.rename = Some(RenameForm {
                    document,
                    lease: captured_lease,
                    position,
                    source,
                    server: active,
                    input,
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// The name input is separate native UI; the full paired rename is a single document transaction.
    fn confirm_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.language_edits.rename.as_ref() else {
            return;
        };
        let new_name = form.input.read(cx).value().to_string();
        if new_name.is_empty() || new_name.len() > 1024 {
            return;
        }
        let form = self.language_edits.rename.take().unwrap();
        let Some(index) = self.active_text_tab_index() else {
            return;
        };
        let language = language_for_path(self.tabs[index].path());
        if self.plugin_document_version(index).ok().as_ref() != Some(&form.document)
            || !form.lease.is_active()
            || !self
                .language_servers
                .get(&language)
                .is_some_and(|current| Arc::ptr_eq(current, &form.server))
        {
            return;
        }
        let lease = form.lease.clone();
        let active = form.server.clone();
        let snapshot = form.source.clone();
        self.language_edits.rename_request = self.language_edits.rename_request.wrapping_add(1);
        self.language_edits.rename_pending = true;
        let request = self.language_edits.rename_request;
        self.editor
            .update(cx, |editor, cx| editor.focus(window, cx));
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .scheduler_executor()
                .spawn_dedicated(move |_| async move {
                    form.server
                        .rename_for(lease, snapshot, form.position, new_name)
                })
                .await;
            let _ = this.update_in(cx, |app, window, cx| {
                if app.language_edits.rename_request != request {
                    return;
                }
                app.language_edits.rename_pending = false;
                if !app
                    .language_servers
                    .get(&language)
                    .is_some_and(|current| Arc::ptr_eq(current, &active))
                {
                    return;
                }
                let cursor = app.editor.read(cx).cursor();
                match result.and_then(|edits| {
                    app.apply_language_edits(
                        &form.document,
                        &form.source,
                        &active,
                        &edits,
                        cursor,
                        window,
                        cx,
                    )
                }) {
                    Ok(()) => app.status = t!("editor.renamed").to_string(),
                    Err(error) => {
                        app.status =
                            t!("editor.editing_failed", error = format!("{error:#}")).to_string()
                    }
                }
                app.editor.update(cx, |editor, cx| editor.focus(window, cx));
                cx.notify();
            });
        })
        .detach();
    }

    /// Local input and buttons use Base behavior while preserving the project's theme and translations.
    pub(crate) fn render_rename_prompt(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        let form = self.language_edits.rename.as_ref()?;
        Some(
            h_flex()
                .debug_selector(|| "editor-rename-prompt".into())
                // Base emits Enter and propagates for its parent to submit. Consume it before
                // platform character fallback can replace a selected name with a filtered newline.
                .on_action(cx.listener(|app, _: &gpui_base::input::Enter, window, cx| {
                    let composing = app.language_edits.rename.as_ref().is_some_and(|form| {
                        form.input.update(cx, |input, cx| {
                            input.marked_text_range(window, cx).is_some()
                        })
                    });
                    if composing {
                        // The platform/Base input keeps ownership of confirming an active IME preedit.
                        cx.propagate();
                        return;
                    }
                    cx.stop_propagation();
                    app.confirm_rename(window, cx);
                }))
                .absolute()
                .top_2()
                .right_2()
                .gap_2()
                .p_2()
                .bg(cx.theme().background)
                .border_1()
                .border_color(cx.theme().border)
                .rounded(cx.theme().radius)
                .child(
                    div()
                        .w(px(220.))
                        .child(crate::ui::controls::Input::new(&form.input)),
                )
                .child(
                    Button::new("editor-rename-confirm")
                        .label(t!("editor.rename_confirm").to_string())
                        .on_click(cx.listener(|app, _, window, cx| app.confirm_rename(window, cx))),
                )
                .child(
                    Button::new("editor-rename-cancel")
                        .label(t!("editor.rename_cancel").to_string())
                        .on_click(cx.listener(|app, _, window, cx| {
                            app.language_edits.cancel_rename();
                            app.editor.update(cx, |editor, cx| editor.focus(window, cx));
                            cx.notify();
                        })),
                )
                .into_any_element(),
        )
    }

    /// Escape dismisses the ordinary rename field and returns focus to the native source.
    pub(crate) fn cancel_rename_action(
        &mut self,
        _: &gpui_base::input::Escape,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.language_edits.cancel_rename() {
            cx.stop_propagation();
            self.editor
                .update(cx, |editor, cx| editor.focus(window, cx));
            cx.notify();
        } else {
            cx.propagate();
        }
    }
}
