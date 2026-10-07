//! Compact action summaries and icon controls; structure is edited in a typed detail surface.
use super::*;

/// Merge immutable provider actions at their original positions beside independently editable rows.
pub(super) fn render_step_rows(
    app: &Entity<EditorApp>,
    field: RunField,
    cx: &gpui_kit::App,
) -> AnyElement {
    let form = app.read(cx).run_form.as_ref().unwrap().clone();
    let state = form.read(cx);
    let rows = state.rows_of(field);
    let count = rows.len();
    let opaque = if field == RunField::Build {
        &state.draft.provider_build
    } else {
        &state.draft.provider_prelaunch
    };
    let owner = app.clone();
    let add_id = format!("{}-add-{}", field.selector(), count);
    let mut list = v_flex().gap_1().child(
        h_flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(if field == RunField::Build {
                        t!("run.form_build_short")
                    } else {
                        t!("run.form_prelaunch_short")
                    }),
            )
            .child(
                Button::new(add_id.clone())
                    .debug_selector(move || add_id.clone())
                    .small()
                    .compact()
                    .ghost()
                    .icon(IconName::Plus)
                    .accessibility_label(t!("run.step_add_hint"))
                    .tooltip(t!("run.step_add_hint"))
                    .disabled(count + opaque.len() >= editor_core::MAX_RUN_STEPS)
                    .on_click(move |_, window, cx| {
                        owner.update(cx, |state, cx| {
                            state.open_run_step_editor(field, None, window, cx)
                        })
                    }),
            ),
    );
    let mut ordered = rows.into_iter().map(|row| Ok(row)).collect::<Vec<_>>();
    for (index, step) in opaque {
        ordered.insert((*index).min(ordered.len()), Err(step.clone()));
    }
    for row in ordered {
        let element = match row {
            Err(step) => h_flex()
                .gap_2()
                .items_center()
                .py_2()
                .child(Icon::default().path("icons/run-build.svg"))
                .child(
                    v_flex().flex_1().min_w_0().child(step.name).child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("run.form_plugin_preparation")),
                    ),
                )
                .child(Icon::default().path(gpui_kit::assets::IconName::Lock.path()))
                .into_any_element(),
            Ok((index, input)) => {
                let text = input.read(cx).value().to_string();
                let parsed = crate::run::parse_steps(&text)
                    .ok()
                    .and_then(|mut steps| steps.pop());
                let (name, summary) = parsed
                    .map(|step| {
                        let summary = match step.target {
                            editor_core::StepTarget::Build { config } => {
                                t!("run.form_reference_summary", name = config).to_string()
                            }
                            editor_core::StepTarget::Action { target } => {
                                target.executable().into()
                            }
                        };
                        (step.name, summary)
                    })
                    .unwrap_or((text, t!("run.form_invalid_step").into()));
                let owner = app.clone();
                let edit_id = format!("{}-edit-{index}", field.selector());
                h_flex()
                    .debug_selector(move || format!("{}-row-{index}", field.selector()))
                    .gap_1()
                    .items_center()
                    .py_2()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().truncate().child(name))
                            .child(
                                div()
                                    .text_xs()
                                    .truncate()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(summary),
                            ),
                    )
                    .child(step_control(app, field, StepEdit::Up, index, index > 0))
                    .child(step_control(
                        app,
                        field,
                        StepEdit::Down,
                        index,
                        index + 1 < count,
                    ))
                    .child(step_control(app, field, StepEdit::Remove, index, true))
                    .child(
                        Button::new(edit_id.clone())
                            .debug_selector(move || edit_id.clone())
                            .small()
                            .compact()
                            .ghost()
                            .icon(
                                Icon::default().path(gpui_kit::assets::IconName::SquarePen.path()),
                            )
                            .accessibility_label(t!("run.form_edit_step"))
                            .tooltip(t!("run.form_edit_step"))
                            .on_click(move |_, window, cx| {
                                owner.update(cx, |state, cx| {
                                    state.open_run_step_editor(field, Some(index), window, cx)
                                })
                            }),
                    )
                    .into_any_element()
            }
        };
        list = list.child(element);
    }
    if count == 0 && opaque.is_empty() {
        list = list.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(t!("run.form_unset")),
        );
    }
    list.into_any_element()
}

/// A disabled movement boundary does not mutate rows, and icons publish their accessible meanings.
fn step_control(
    app: &Entity<EditorApp>,
    field: RunField,
    edit: StepEdit,
    index: usize,
    enabled: bool,
) -> Button {
    let (key, icon, hint) = match edit {
        StepEdit::Up => ("up", IconName::ChevronUp, t!("run.step_up_hint")),
        StepEdit::Down => ("down", IconName::ChevronDown, t!("run.step_down_hint")),
        StepEdit::Remove => ("remove", IconName::Close, t!("run.step_remove_hint")),
        StepEdit::Add => unreachable!("add uses the typed editor"),
    };
    let owner = app.clone();
    let id = format!("{}-{key}-{index}", field.selector());
    Button::new(id.clone())
        .debug_selector(move || id.clone())
        .small()
        .compact()
        .ghost()
        .icon(icon)
        .accessibility_label(hint.clone())
        .tooltip(hint)
        .disabled(!enabled)
        .on_click(move |_, window, cx| {
            owner.update(cx, |state, cx| {
                if let Some(form) = &state.run_form {
                    form.update(cx, |form, cx| {
                        form.edit_rows(field, edit, index, window, cx)
                    });
                }
                cx.notify();
            })
        })
}
