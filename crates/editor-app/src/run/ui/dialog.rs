//! Provider-owned forms use the same owned native dialog mechanism as plugin management.
//! Window ownership and draft lifetime are kept together; detail editors never create extra HWNDs.
use super::*;

/// Native dialog content observes the editor and follows each replacement draft's identity.
pub(crate) struct RunConfigModal {
    owner: WeakEntity<EditorApp>,
    focus: FocusHandle,
    _updates: Vec<Subscription>,
    observed_form: Option<Entity<RunConfigForm>>,
    _form_update: Option<Subscription>,
}

impl Render for RunConfigModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(app) = self.owner.upgrade() else {
            // The owned window cannot keep an abandoned editor workspace alive.
            window.defer(cx, |window, _| window.remove_window());
            return div().into_any_element();
        };
        let Some(form) = app.read(cx).run_form.clone() else {
            self.observed_form = None;
            self._form_update = None;
            return div().into_any_element();
        };
        if self
            .observed_form
            .as_ref()
            .is_none_or(|old| old.entity_id() != form.entity_id())
        {
            self._form_update = Some(cx.observe(&form, |_, _, cx| cx.notify()));
            self.observed_form = Some(form.clone());
        }
        let cancel = self.owner.clone();
        let save = self.owner.clone();
        crate::ui::controls::dialog::native_modal(
            self.focus.clone(),
            render_run_config_form(&self.owner, DialogContent::new(), window, cx)
                .into_any_element(),
            move |_, cx| {
                cancel
                    .update(cx, |state, cx| state.request_plugin_close(cx))
                    .unwrap_or(true)
            },
            move |_, cx| {
                save.update(cx, |state, cx| {
                    let Some(form) = &state.run_form else {
                        return true;
                    };
                    if form.read(cx).plugin.is_some() {
                        // Keyboard confirmation follows the same asynchronous validation as Save.
                        state.begin_plugin_commit(super::plugin_form::CommitMode::Save, cx);
                        return false;
                    }
                    // A first initialization paint has no provider state and cannot save old fields.
                    false
                })
                .unwrap_or(true)
            },
            window,
            cx,
        )
    }
}

impl EditorApp {
    /// Removing a prompt or inline editor retires its focus node; return keyboard routing to the body.
    pub(in crate::run::ui) fn focus_run_form_body(
        &self,
        window: &mut Window,
        cx: &mut gpui_kit::App,
    ) {
        if let Some(dialog) = &self.run_dialog {
            let focus = dialog.read(cx).focus.clone();
            focus.focus(window, cx);
        }
    }

    /// Open or activate one native modal window, following the external configuration selection.
    /// The shared AppDialog uses WindowKind::Dialog, keeping it above and modal to its parent.
    pub(crate) fn open_run_config_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        editing: Option<String>,
    ) {
        if self.run_dialog_opening {
            return;
        }
        self.extensions.read(cx).ask_run_providers();
        // Discovery stages defaults only, and cannot bypass the existing workspace trust gate.
        self.run_dialog_opening = true;
        let owner = cx.entity().downgrade();
        let parent = window.window_handle();
        // Native creation paints its Root immediately. Do not let that paint read a leased EditorApp.
        cx.defer(move |cx| open_native_run_dialog(owner, parent, editing, cx));
    }

    /// Native close requests follow the same draft decision as X and Escape.
    /// Draft cleanup runs on the close receipt, avoiding destruction inside the native close callback.
    fn allow_native_run_form_close(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.request_plugin_close(cx)
    }

    /// Discard the draft and close only its native window, after the editor's update lease ends.
    pub(crate) fn close_run_form(&mut self, cx: &mut Context<Self>) {
        self.cancel_plugin_window_calls(cx);
        let handle = self.run_dialog_window.take();
        self.run_dialog_opening = false;
        self.run_dialog = None;
        self.run_form = None;
        self._run_dialog_closed_subscription = None;
        if let Some(handle) = handle {
            cx.defer(move |cx| {
                // Chrome or the platform may already have removed it; stale handles are harmless.
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            });
        }
        cx.notify();
    }
}

/// Create/activate outside the editor lease, then attach a form initialized in the actual native HWND.
fn open_native_run_dialog(
    owner: WeakEntity<EditorApp>,
    parent: gpui_kit::AnyWindowHandle,
    _editing: Option<String>,
    cx: &mut gpui_kit::App,
) {
    let Some(app) = owner.upgrade() else {
        return;
    };
    if !app.read(cx).run_dialog_opening {
        return;
    }
    if let Some(handle) = app.read(cx).run_dialog_window
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        app.update(cx, |state, _| state.run_dialog_opening = false);
        return;
    }
    // The shared Windows backend owns its Dialog relative to the currently active parent.
    if parent
        .update(cx, |_, window, _| window.activate_window())
        .is_err()
    {
        app.update(cx, |state, _| state.run_dialog_opening = false);
        return;
    }
    let body = cx.new(|cx| RunConfigModal {
        owner: owner.clone(),
        focus: cx.focus_handle(),
        _updates: vec![cx.observe(&app, |_, _, cx| cx.notify())],
        observed_form: None,
        _form_update: None,
    });
    let view = body.clone();
    let (dialog, handle) = app_dialog::open_dialog_sized(
        t!("run.form_title").to_string(),
        940.,
        502.,
        move |content, _, _| content.h_full().child(view.clone()),
        cx,
    );
    let chrome_owner = owner.clone();
    dialog.update(cx, |dialog, _| {
        dialog.set_close_request(move |window, cx| {
            chrome_owner
                .update(cx, |state, cx| {
                    state.allow_native_run_form_close(window, cx)
                })
                .unwrap_or(true)
        })
    });
    let focus = body.read(cx).focus.clone();
    let form = handle
        .update(cx, |_, window, cx| {
            // Inputs must use this dialog's window, not the parent's input/IME geometry.
            let form = cx.new(RunConfigForm::for_plugins);
            focus.focus(window, cx);
            window.on_window_should_close(cx, move |window, cx| {
                owner
                    .update(cx, |state, cx| {
                        state.allow_native_run_form_close(window, cx)
                    })
                    .unwrap_or(true)
            });
            form
        })
        .expect("new configuration dialog is available");
    let window_id = handle.window_id();
    let closing = app.downgrade();
    let closed = cx.on_window_closed(move |cx, closed_id| {
        if closed_id == window_id {
            let _ = closing.update(cx, |state, cx| {
                // A delayed old close receipt cannot discard a new window's draft.
                if state
                    .run_dialog_window
                    .is_some_and(|current| current.window_id() == closed_id)
                {
                    state.cancel_plugin_window_calls(cx);
                    state.run_dialog_window = None;
                    state.run_dialog = None;
                    state.run_form = None;
                    state._run_dialog_closed_subscription = None;
                    cx.notify();
                }
            });
        }
    });
    app.update(cx, |state, cx| {
        state.run_dialog_opening = false;
        state.run_form = Some(form);
        let form = state.run_form.clone().unwrap();
        state.initialize_plugin_form(&form, cx);
        state.run_dialog = Some(body);
        state.run_dialog_window = Some(handle);
        state._run_dialog_closed_subscription = Some(closed);
        cx.notify();
    });
}
