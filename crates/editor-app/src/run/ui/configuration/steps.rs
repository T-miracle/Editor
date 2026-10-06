//! Native prepared-action rows keep their literal inputs, ordering and independent controls.
use super::*;

/// Render one prepared-action list: a row per action with its own move and remove controls, then an
/// add control for the next action.
///
/// Each row is an ordinary single-line field, so a row edit never rewrites another row's text.
pub(super) fn render_step_rows(
    app: &Entity<EditorApp>,
    field: RunField,
    shell: bool,
    cx: &mut gpui_kit::App,
) -> AnyElement {
    let Some(form) = app.read(cx).run_form.clone() else {
        return div().into_any_element();
    };
    let rows = form.read(cx).rows_of(field);
    let count = rows.len();
    let mut list = v_flex().gap_1();
    for (index, input) in rows {
        let owner = app.clone();
        list = list.child(
            h_flex()
                .debug_selector(move || format!("{}-row-{index}", field.selector()))
                .gap_1()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .child(crate::ui::controls::Input::new(&input)),
                )
                .child(step_control(
                    &owner,
                    field,
                    StepEdit::Up,
                    index,
                    index > 0,
                    shell,
                ))
                .child(step_control(
                    &owner,
                    field,
                    StepEdit::Down,
                    index,
                    index + 1 < count,
                    shell,
                ))
                .child(step_control(
                    &owner,
                    field,
                    StepEdit::Remove,
                    index,
                    true,
                    shell,
                )),
        );
    }
    list.child(step_control(app, field, StepEdit::Add, count, true, shell))
        .into_any_element()
}

/// One structural control for a prepared-action list.
///
/// A control that cannot act — moving the first row up, for example — is disabled rather than
/// silently doing nothing, and each carries the row it addresses.
fn step_control(
    app: &Entity<EditorApp>,
    field: RunField,
    edit: StepEdit,
    index: usize,
    enabled: bool,
    shell: bool,
) -> AnyElement {
    let (label, hint) = match edit {
        StepEdit::Add => (
            t!("run.step_add").to_string(),
            t!("run.step_add_hint").to_string(),
        ),
        StepEdit::Remove => (
            t!("run.step_remove").to_string(),
            t!("run.step_remove_hint").to_string(),
        ),
        StepEdit::Up => (
            t!("run.step_up").to_string(),
            t!("run.step_up_hint").to_string(),
        ),
        // A pre-launch step may require another configuration's build by naming it after `@`.
        StepEdit::Down => (
            t!("run.step_down").to_string(),
            t!("run.step_down_hint").to_string(),
        ),
    };
    let _ = shell;
    let owner = app.clone();
    let edit_key = match edit {
        StepEdit::Add => "add",
        StepEdit::Remove => "remove",
        StepEdit::Up => "up",
        StepEdit::Down => "down",
    };
    let id = format!("{}-{edit_key}-{index}", field.selector());
    let selector = id.clone();
    div()
        .debug_selector(move || selector.clone())
        .child(
            Button::new(id)
                .label(label)
                .small()
                .compact()
                .ghost()
                .disabled(!enabled)
                .tooltip(hint)
                .on_click(move |_, window, cx| {
                    owner.update(cx, |state, cx| {
                        let Some(form) = state.run_form.clone() else {
                            return;
                        };
                        form.update(cx, |form, cx| {
                            form.edit_rows(field, edit, index, window, cx);
                        });
                        cx.notify();
                    });
                }),
        )
        .into_any_element()
}
