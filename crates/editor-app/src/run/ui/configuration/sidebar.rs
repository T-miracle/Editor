//! Flat configuration management with plugin defaults visible as unsaved candidate choices.
use super::*;

/// Stored configurations and unconfirmed defaults share a compact list but retain stable identities.
pub(super) fn render_sidebar(
    app: &Entity<EditorApp>,
    form: &Entity<RunConfigForm>,
    narrow: bool,
    cx: &gpui_kit::App,
) -> AnyElement {
    let state = form.read(cx);
    let current_id = state.draft.id.clone();
    let current_name = state.text(RunField::Name, cx);
    let current_name = if current_name.is_empty() {
        t!("run.form_new_title").into()
    } else {
        current_name
    };
    let stored = app.read(cx).run_controls.configurations();
    let mut list = v_flex()
        .id("run-config-list")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .gap_1()
        .p_2();
    for config in stored {
        let selected = config.id == current_id;
        list = list.child(row(
            app,
            format!("run-config-existing-{}", config.id),
            if selected {
                current_name.clone()
            } else {
                config.name.clone()
            },
            selected,
            FormSelection::Configuration(config.id.clone()),
            cx,
        ));
    }
    if !stored.iter().any(|config| config.id == current_id) {
        list = list.child(row(
            app,
            "run-config-draft".into(),
            current_name,
            true,
            FormSelection::Configuration(current_id),
            cx,
        ));
    }
    // Discovery is an offer only: selecting a row stages its defaults and does not create a file.
    for target in app.read(cx).run_controls.discovered_targets() {
        if stored.iter().any(|config| config.claims_target(target))
            || state.draft.from_target.as_deref() == Some(&target.id)
        {
            continue;
        }
        list = list.child(
            row(
                app,
                format!("run-config-target-{}", target.id),
                target.label.clone(),
                false,
                FormSelection::Candidate(target.id.clone()),
                cx,
            )
            .tooltip(format!("{} · {}", target.provider, target.found_in)),
        );
    }
    let discover = app.clone();
    v_flex()
        .debug_selector(|| "run-config-sidebar".into())
        .flex_shrink_0()
        .when(narrow, |side| side.w_full().h(px(118.)).border_b_1())
        .when(!narrow, |side| side.w(px(204.)).border_r_1())
        .border_color(cx.theme().border)
        .bg(cx.theme().muted)
        .child(
            h_flex()
                .h(px(44.))
                .items_center()
                .gap_1()
                .px_3()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .flex_1()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!("run.form_configurations")),
                )
                .child(
                    // Discovery shares the header toolbar in both layouts; its label remains
                    // available to assistive technology and as a tooltip without occupying a row.
                    Button::new("run-config-discover")
                        .debug_selector(|| "run-config-discover".into())
                        .ghost()
                        .small()
                        .compact()
                        .w(px(28.))
                        .icon(IconName::Search)
                        .accessibility_label(t!("run.discover"))
                        .tooltip(t!("run.discover"))
                        .on_click(move |_, _, cx| {
                            discover.update(cx, |state, cx| state.discover_run_targets(cx))
                        }),
                )
                .child(
                    picker_button(app, PickerKind::New, String::new(), cx)
                        .compact()
                        .w(px(28.))
                        .border_0()
                        .tooltip(t!("run.form_new")),
                )
                .child(
                    picker_button(app, PickerKind::More, String::new(), cx)
                        .compact()
                        .w(px(28.))
                        .border_0()
                        .tooltip(t!("run.form_actions")),
                ),
        )
        .child(list)
        .into_any_element()
}

/// The local button owns pointer/keyboard activation; the owner handles unsaved navigation guards.
fn row(
    app: &Entity<EditorApp>,
    id: String,
    label: String,
    selected: bool,
    selection: FormSelection,
    cx: &gpui_kit::App,
) -> Button {
    let owner = app.clone();
    Button::new(id.clone())
        .debug_selector(move || id.clone())
        .ghost()
        .content_full_width()
        .w_full()
        .min_w_0()
        .h(px(37.))
        .bg(if selected {
            cx.theme().accent
        } else {
            cx.theme().transparent
        })
        .text_color(if selected {
            cx.theme().accent_foreground
        } else {
            cx.theme().foreground
        })
        .accessibility_label(label.clone())
        .tooltip(label.clone())
        .child(div().min_w_0().flex_1().truncate().child(label))
        .on_click(move |_, window, cx| {
            owner.update(cx, |state, cx| {
                state.select_run_form(selection.clone(), window, cx)
            })
        })
}
