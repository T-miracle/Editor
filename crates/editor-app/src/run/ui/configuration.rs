//! Simplified native configuration layout: a sidebar, three main fields and controlled disclosures.
//! Plugin candidates fill an unsaved draft; all execution semantics remain in the existing contracts.
use super::*;
mod actions;
mod editor;
mod picker;
mod sidebar;
mod step_editor;
mod steps;
use picker::{PickerKind, picker_button};
pub(super) use step_editor::StepEditor;

/// Render the selected design from retained native controls; only the body scrolls, never Save.
pub(crate) fn render_run_config_form(
    app: &WeakEntity<EditorApp>,
    content: DialogContent,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) -> DialogContent {
    let Some(app) = app.upgrade() else {
        return content;
    };
    let Some(form) = app.read(cx).run_form.clone() else {
        return content;
    };
    if form.read(cx).plugin.is_some() {
        return content.child(super::plugin_form::render(&app, &form, window, cx));
    }
    let body = if form.read(cx).editor.is_some() {
        editor::render_editor(&app, &form, cx)
    } else if form.read(cx).pending_selection.is_some() {
        editor::render_navigation_confirmation(&app, &form, cx)
    } else {
        let narrow = window.viewport_size().width < px(640.);
        let columns = div()
            .flex()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .when(narrow, |layout| layout.flex_col())
            .child(sidebar::render_sidebar(&app, &form, narrow, cx))
            .child(render_page(&app, &form, cx));
        v_flex()
            .size_full()
            .min_h_0()
            .child(columns)
            .child(render_footer(&app, cx))
            .into_any_element()
    };
    let popup = form.read(cx).picker.as_ref().map(|menu| menu.popup.clone());
    content.child(
        v_flex()
            .debug_selector(|| "run-config-form".into())
            .size_full()
            .min_h_0()
            .text_sm()
            .child(body)
            .when_some(popup, |layout, popup| layout.child(popup)),
    )
}

/// Long optional content is confined to the editing pane, keeping the sidebar and footer reachable.
fn render_page(
    app: &Entity<EditorApp>,
    form: &Entity<RunConfigForm>,
    cx: &mut gpui_kit::App,
) -> AnyElement {
    let state = form.read(cx);
    let mut fields = vec![
        render_field(form, RunField::Name, t!("run.field_name").into(), cx),
        render_target(app, form, cx),
        render_field_choice(
            app,
            form,
            RunField::Arguments,
            t!("run.form_arguments").into(),
            cx,
        ),
    ];
    if state.draft.shell {
        fields.push(render_field(
            form,
            RunField::Script,
            t!("run.field_script").into(),
            cx,
        ));
    }
    v_flex()
        .id("run-config-page")
        .debug_selector(|| "run-config-page".into())
        .flex_1()
        .min_w_0()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&state.page_scroll)
        .gap(px(17.))
        .px(px(28.))
        .py(px(23.))
        .children(fields)
        .child(render_startup(app, form, cx))
        .child(render_more(app, form, cx))
        .when_some(state.error.clone(), |page, error| {
            page.child(
                div()
                    .debug_selector(|| "run-config-error".into())
                    .text_color(cx.theme().danger)
                    .child(error),
            )
        })
        .into_any_element()
}

/// Local controls keep input state, focus, IME and literal values independent from disclosure state.
pub(super) fn render_field(
    form: &Entity<RunConfigForm>,
    field: RunField,
    label: String,
    cx: &gpui_kit::App,
) -> AnyElement {
    let state = form.read(cx);
    let input = if let Some((_, input)) = state.textareas.iter().find(|(key, _)| *key == field) {
        crate::ui::controls::Textarea::new(input).into_any_element()
    } else if let Some((_, input)) = state.inputs.iter().find(|(key, _)| *key == field) {
        crate::ui::controls::Input::new(input)
            .h(px(35.))
            .into_any_element()
    } else {
        div().into_any_element()
    };
    v_flex()
        .gap(px(7.))
        .child(div().text_xs().child(label))
        .child(
            div()
                .debug_selector(move || field.selector().into())
                .min_w_0()
                .child(input),
        )
        .into_any_element()
}

/// Provider targets are choices, never editable binding strings or guessed shell commands.
fn render_target(
    app: &Entity<EditorApp>,
    form: &Entity<RunConfigForm>,
    cx: &gpui_kit::App,
) -> AnyElement {
    let state = form.read(cx);
    let provided = state.draft.provided.is_some();
    let manual =
        !provided && (state.manual_target || !state.text(RunField::Program, cx).is_empty());
    let label = if provided || !manual {
        t!("run.form_target")
    } else if state.draft.shell {
        t!("run.field_interpreter")
    } else {
        t!("run.field_program")
    };
    let target_label = if provided {
        state.draft.program.clone()
    } else {
        t!("run.form_choose_target").into()
    };
    let source = state
        .draft
        .from_target
        .as_ref()
        .and_then(|id| {
            app.read(cx)
                .run_controls
                .discovered_targets()
                .iter()
                .find(|target| &target.id == id)
        })
        .map(|target| format!("{} · {}", target.provider, target.found_in))
        .unwrap_or_else(|| {
            state
                .draft
                .provided
                .as_ref()
                .map(|target| match target {
                    editor_core::RunTarget::Provided { provider, .. } => {
                        t!("run.form_source_provider", provider = provider).into()
                    }
                    _ => String::new(),
                })
                .unwrap_or_default()
        });
    let control = if manual {
        let input = state
            .inputs
            .iter()
            .find(|(field, _)| *field == RunField::Program)
            .unwrap()
            .1
            .clone();
        h_flex()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .debug_selector(|| "run-config-program".into())
                    .child(crate::ui::controls::Input::new(&input).h(px(35.))),
            )
            .child(
                picker_button(app, PickerKind::Target, String::new(), cx)
                    .w(px(35.))
                    .compact(),
            )
            .into_any_element()
    } else {
        picker_button(app, PickerKind::Target, target_label, cx)
            .tooltip(source)
            .h(px(35.))
            .into_any_element()
    };
    v_flex()
        .gap(px(7.))
        .child(div().text_xs().child(label))
        .child(control)
        .into_any_element()
}

/// A compact optional-field entry opens an ordinary native multi-line editor on demand.
fn render_field_choice(
    app: &Entity<EditorApp>,
    form: &Entity<RunConfigForm>,
    field: RunField,
    label: String,
    cx: &gpui_kit::App,
) -> AnyElement {
    let text = form.read(cx).text(field, cx);
    let summary = if text.is_empty() {
        t!("run.form_unset").into()
    } else {
        text.lines().collect::<Vec<_>>().join("  ")
    };
    let owner = app.clone();
    let id = format!("{}-edit", field.selector());
    v_flex()
        .gap(px(7.))
        .child(h_flex().gap_2().child(div().text_xs().child(label)).when(
            field == RunField::Arguments,
            |label| {
                label.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!("run.form_optional")),
                )
            },
        ))
        .child(
            Button::new(id.clone())
                .debug_selector(move || id.clone())
                .ghost()
                .content_full_width()
                .w_full()
                .min_w_0()
                .h(px(35.))
                .border_1()
                .border_color(cx.theme().input)
                .child(div().flex_1().min_w_0().truncate().child(summary))
                .child(Icon::new(IconName::Ellipsis))
                .on_click(move |_, window, cx| {
                    owner.update(cx, |state, cx| {
                        state.open_run_field_editor(field, window, cx)
                    })
                }),
        )
        .into_any_element()
}

/// Build and prelaunch remain separate lists; opaque provider preparations are visible but locked.
fn render_startup(
    app: &Entity<EditorApp>,
    form: &Entity<RunConfigForm>,
    cx: &gpui_kit::App,
) -> AnyElement {
    let state = form.read(cx);
    let count = state.rows_of(RunField::Build).len() + state.rows_of(RunField::Prelaunch).len();
    let automatic = !state.draft.provider_build.is_empty();
    let summary: String = if automatic {
        t!("run.form_startup_automatic", count = count).into()
    } else if count == 0 {
        t!("run.form_unset").into()
    } else {
        t!("run.form_startup_count", count = count).into()
    };
    let owner = app.clone();
    crate::ui::controls::disclosure(
        "run-config-startup",
        t!("run.form_startup"),
        summary,
        state.startup_open,
        &state.disclosure_focus[0],
        v_flex()
            .gap_4()
            .pl_5()
            .pb_3()
            .child(steps::render_step_rows(app, RunField::Build, cx))
            .child(steps::render_step_rows(app, RunField::Prelaunch, cx))
            .into_any_element(),
        move |open, _, cx| {
            owner.update(cx, |state, cx| {
                if let Some(form) = &state.run_form {
                    form.update(cx, |form, cx| {
                        form.startup_open = open;
                        cx.notify();
                    });
                }
            })
        },
        cx,
    )
}

/// Advanced settings are real controls backed by the existing configuration fields and providers.
fn render_more(
    app: &Entity<EditorApp>,
    form: &Entity<RunConfigForm>,
    cx: &gpui_kit::App,
) -> AnyElement {
    let state = form.read(cx);
    let customized = !state.text(RunField::Directory, cx).is_empty()
        || !state.text(RunField::Environment, cx).is_empty()
        || !state.text(RunField::ToolPaths, cx).is_empty()
        || state.draft.provider.is_some()
        || !state.text(RunField::Breakpoints, cx).is_empty();
    let owner = app.clone();
    let fields = v_flex()
        .gap_3()
        .pl_5()
        .pb_3()
        .child(render_field(
            form,
            RunField::Directory,
            t!("run.field_directory").into(),
            cx,
        ))
        .child(render_field_choice(
            app,
            form,
            RunField::Environment,
            t!("run.form_environment_short").into(),
            cx,
        ))
        .child(
            v_flex()
                .gap_2()
                .child(div().text_xs().child(t!("run.form_provider")))
                .child(picker_button(
                    app,
                    PickerKind::Provider,
                    state
                        .draft
                        .provider
                        .clone()
                        .unwrap_or_else(|| t!("run.provider_default").into()),
                    cx,
                )),
        )
        .child(render_field_choice(
            app,
            form,
            RunField::ToolPaths,
            t!("run.form_tools_short").into(),
            cx,
        ))
        .child(render_field_choice(
            app,
            form,
            RunField::Breakpoints,
            t!("run.form_breakpoints_short").into(),
            cx,
        ))
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(t!("run.form_local_settings")),
        )
        .into_any_element();
    crate::ui::controls::disclosure(
        "run-config-more",
        t!("run.form_more"),
        if customized {
            t!("run.form_custom_settings")
        } else {
            t!("run.form_default_settings")
        },
        state.more_open,
        &state.disclosure_focus[1],
        fields,
        move |open, _, cx| {
            owner.update(cx, |state, cx| {
                if let Some(form) = &state.run_form {
                    form.update(cx, |form, cx| {
                        form.more_open = open;
                        cx.notify();
                    });
                }
            })
        },
        cx,
    )
}

/// Saving writes only the chosen configuration; scope stays in the fixed footer beside two actions.
fn render_footer(app: &Entity<EditorApp>, cx: &gpui_kit::App) -> AnyElement {
    let share = app.read(cx).run_form.as_ref().unwrap().read(cx).draft.share;
    let cancel = app.clone();
    let save = app.clone();
    h_flex()
        .debug_selector(|| "run-config-footer".into())
        .flex_shrink_0()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap_2()
        .px_4()
        .py_3()
        .border_t_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .debug_selector(|| "run-config-destination".into())
                .w(px(138.))
                .child(picker_button(
                    app,
                    PickerKind::Destination,
                    if share {
                        t!("run.form_shared")
                    } else {
                        t!("run.form_local")
                    }
                    .into(),
                    cx,
                )),
        )
        .child(
            h_flex()
                .gap_2()
                .child(
                    Button::new("run-config-cancel")
                        .debug_selector(|| "run-config-cancel".into())
                        .label(t!("run.form_cancel"))
                        .border_1()
                        .border_color(cx.theme().input)
                        .on_click(move |_, window, cx| {
                            cancel.update(cx, |state, cx| {
                                state.cancel_run_form(window, cx);
                            });
                        }),
                )
                .child(
                    Button::new("run-config-save")
                        .debug_selector(|| "run-config-save".into())
                        .primary()
                        .label(t!("run.form_save_short"))
                        .on_click(move |_, window, cx| {
                            save.update(cx, |state, cx| state.commit_run_form(window, cx))
                        }),
                ),
        )
        .into_any_element()
}
