//! Local window chrome surrounds provider-owned native forms; only the content panes scroll.
use super::*;
use crate::ui::controls::{Button, Icon};

/// Create keyed native views in the actual dialog window, preserving input and composition on refresh.
pub(in crate::run::ui) fn render(
    app: &Entity<EditorApp>,
    form: &Entity<RunConfigForm>,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) -> AnyElement {
    let environment = crate::extensions::environment(app.read(cx).workspace.root(), cx);
    let selected = form.read(cx).plugin.as_ref().unwrap().selected.clone();
    if let Some(id) = &selected {
        let current = form.read(cx).plugin.as_ref().unwrap();
        let document = current.documents.get(id).cloned();
        let provider = current
            .draft
            .plugin_configurations
            .get(id)
            .map(|data| data.provider.clone());
        if let (Some(document), Some(provider)) = (document, provider) {
            let owner = app.downgrade();
            let configuration = id.clone();
            form.update(cx, |form, cx| {
                let state = form.plugin.as_mut().unwrap();
                if let Some(view) = state.views.get(id) {
                    view.update(cx, |view, cx| {
                        view.update_document(document, environment.clone(), window, cx)
                    });
                } else {
                    let view = cx.new(|cx| {
                        crate::ui::plugin::PluginView::new(
                            provider,
                            document,
                            environment.clone(),
                            move |event, cx| {
                                let _ = owner.update(cx, |app, cx| {
                                    app.plugin_configuration_event(&configuration, event, cx)
                                });
                            },
                            window,
                            cx,
                        )
                    });
                    state.views.insert(id.clone(), view);
                }
            });
        }
    }
    let state = form.read(cx).plugin.as_ref().unwrap();
    let right = match selected.as_ref() {
        Some(id) => state
            .views
            .get(id)
            .map(|view| div().size_full().child(view.clone()).into_any_element())
            .unwrap_or_else(|| empty(t!("run.plugin_loading").into(), cx)),
        None => empty(t!("run.plugin_add_prompt").into(), cx),
    };
    let error = state.error.clone();
    let body = h_flex()
        // h_flex centers children by default; both panes must instead fill the native dialog body.
        // Otherwise the percentage-height plugin scroller has no usable input/paint area.
        .items_stretch()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .child(sidebar(app, form, cx))
        .child(
            v_flex()
                .debug_selector(|| "run-config-page".into())
                .h_full()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .p_5()
                .child(div().flex_1().min_h_0().child(right))
                .when_some(error, |pane, error| {
                    pane.child(
                        div()
                            .debug_selector(|| "plugin-configuration-error".into())
                            .text_sm()
                            .text_color(cx.theme().danger)
                            .child(error),
                    )
                }),
        );
    v_flex()
        .debug_selector(|| "run-config-form".into())
        .size_full()
        .min_h_0()
        .child(body)
        .child(footer(app, form, cx))
        .into_any_element()
}

fn empty(text: String, cx: &gpui_kit::App) -> AnyElement {
    div()
        .debug_selector(|| "plugin-configuration-empty".into())
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_color(cx.theme().muted_foreground)
        .child(text)
        .into_any_element()
}

/// Template discovery overlays the tree while the toolbar and right form keep their positions.
fn sidebar(
    app: &Entity<EditorApp>,
    form: &Entity<RunConfigForm>,
    cx: &gpui_kit::App,
) -> AnyElement {
    let state = form.read(cx).plugin.as_ref().unwrap();
    let add = app.clone();
    let header = h_flex()
        .h(px(42.))
        .gap_1()
        .px_2()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            toolbar("run-config-add", IconName::Plus, t!("run.form_new").into()).on_click(
                move |_, _, cx| {
                    let Some(form) = add.read(cx).run_form.clone() else {
                        return;
                    };
                    form.update(cx, |form, cx| {
                        let state = form.plugin.as_mut().unwrap();
                        state.drawer = !state.drawer;
                        cx.notify();
                    });
                },
            ),
        )
        .child(
            toolbar(
                "run-config-delete",
                Icon::default().data(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13M10 10v7M14 10v7"/></svg>"#),
                t!("run.form_delete").into(),
            )
            .disabled(true),
        )
        .child(
            toolbar(
                "run-config-copy",
                IconName::Copy,
                t!("run.form_copy").into(),
            )
            .disabled(true),
        )
        .child(
            toolbar(
                "run-config-folder",
                IconName::Folder,
                t!("run.plugin_add_folder").into(),
            )
            .disabled(true),
        );
    let mut rows = Vec::new();
    let drawer = state.drawer;
    if state.drawer {
        let mut groups: BTreeMap<String, Vec<(String, contract::Template)>> = BTreeMap::new();
        for (provider, template) in &state.catalog {
            groups
                .entry(template.group.clone())
                .or_default()
                .push((provider.clone(), template.clone()));
        }
        for (group, templates) in groups {
            rows.push(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(group)
                    .into_any_element(),
            );
            for (provider, template) in templates {
                let owner = app.clone();
                let target = template.clone();
                let label = template.label.clone();
                let disabled = template.unavailable.is_some();
                let selector = format!("run-template-{provider}-{}", template.id);
                rows.push(
                    Button::new(format!("run-template-{provider}-{}", template.id))
                        .debug_selector(move || selector.clone())
                        .ghost()
                        .w_full()
                        .min_w_0()
                        .content_full_width()
                        .h(px(32.))
                        .icon(template_icon(&template.icon))
                        .label(label)
                        .disabled(disabled)
                        .tooltip(template.unavailable.unwrap_or_else(|| provider.clone()))
                        .on_click(move |_, _, cx| {
                            owner.update(cx, |app, cx| {
                                app.add_plugin_configuration(provider.clone(), target.clone(), cx)
                            })
                        })
                        .into_any_element(),
                );
            }
        }
        if rows.is_empty() {
            rows.push(empty(t!("run.plugin_no_templates").into(), cx));
        }
    } else {
        for configuration in &state.draft.configurations {
            let Some(data) = state.draft.plugin_configurations.get(&configuration.id) else {
                continue;
            };
            let id = configuration.id.clone();
            let selector = format!("run-config-tree-{id}");
            let owner = app.clone();
            let label = if data.name.is_empty() {
                t!("run.plugin_unnamed").into()
            } else {
                data.name.clone()
            };
            let valid = matches!(data.validation, ConfigurationValidation::Valid);
            rows.push(
                Button::new(format!("run-config-tree-{id}"))
                    .debug_selector(move || selector.clone())
                    .ghost()
                    .w_full()
                    .content_full_width()
                    .min_w_0()
                    .h(px(34.))
                    .icon(IconName::Play)
                    .label(label.clone())
                    .tooltip(actions::validation_reason(&data.validation))
                    .bg(if state.selected.as_deref() == Some(&id) {
                        cx.theme().accent
                    } else {
                        cx.theme().transparent
                    })
                    .text_color(if valid {
                        cx.theme().foreground
                    } else {
                        cx.theme().danger
                    })
                    .on_click(move |_, _, cx| {
                        owner.update(cx, |app, cx| app.select_plugin_configuration(&id, cx))
                    })
                    .into_any_element(),
            );
        }
    }
    v_flex()
        .debug_selector(|| "run-config-sidebar".into())
        .h_full()
        .w(px(220.))
        .flex_shrink_0()
        .min_h_0()
        .bg(cx.theme().muted)
        .border_r_1()
        .border_color(cx.theme().border)
        .child(header)
        .child(
            v_flex()
                .id("run-config-tree-list")
                .debug_selector(move || {
                    if drawer {
                        "run-config-drawer"
                    } else {
                        "run-config-tree-list"
                    }
                    .into()
                })
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&state.scroll)
                .p_2()
                .children(rows),
        )
        .into_any_element()
}

/// Icon-only controls retain accessible names and the same native button behavior as other windows.
fn toolbar(id: &'static str, icon: impl Into<Icon>, label: String) -> Button {
    Button::new(id)
        .debug_selector(move || id.into())
        .ghost()
        .small()
        .compact()
        .w(px(30.))
        .h(px(30.))
        .icon(icon)
        .tooltip(label.clone())
        .accessibility_label(label)
}

fn template_icon(name: &str) -> Icon {
    // Plugins may provide a bounded self-contained SVG instead of using the stock command artwork.
    if name.trim_start().starts_with("<svg") {
        return Icon::default().data(name.as_bytes());
    }
    match name {
        "terminal" => Icon::default().data(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3M13 15h4"/></svg>"#),
        "build" => Icon::default().path("icons/run-build.svg"),
        "debug" => Icon::default().path("icons/run-debug.svg"),
        "code" => Icon::default().data(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="m8 6-6 6 6 6m8-12 6 6-6 6M14 4l-4 16"/></svg>"#),
        _ => Icon::default().path("icons/run-start.svg"),
    }
}

/// Window-level submission is always host-owned; plugin controls cannot replace these actions.
fn footer(app: &Entity<EditorApp>, form: &Entity<RunConfigForm>, cx: &gpui_kit::App) -> AnyElement {
    let state = form.read(cx).plugin.as_ref().unwrap();
    let applying = state.commit.is_some();
    let apply = app.clone();
    let save = app.clone();
    let cancel = app.clone();
    h_flex()
        .debug_selector(|| "run-config-footer".into())
        .flex_shrink_0()
        .justify_end()
        .gap_2()
        .px_4()
        .py_3()
        .border_t_1()
        .border_color(cx.theme().border)
        .child(
            Button::new("run-config-apply")
                .debug_selector(|| "run-config-apply".into())
                .label(t!("run.plugin_apply"))
                .disabled(applying || state.selected.is_none())
                .on_click(move |_, _, cx| {
                    apply.update(cx, |app, cx| app.begin_plugin_commit(CommitMode::Apply, cx))
                }),
        )
        .child(
            Button::new("run-config-cancel")
                .debug_selector(|| "run-config-cancel".into())
                .label(t!("run.form_cancel"))
                .on_click(move |_, _, cx| cancel.update(cx, |app, cx| app.close_run_form(cx))),
        )
        .child(
            Button::new("run-config-save")
                .debug_selector(|| "run-config-save".into())
                .primary()
                .label(t!("run.form_save_short"))
                .disabled(applying)
                .on_click(move |_, _, cx| {
                    save.update(cx, |app, cx| app.begin_plugin_commit(CommitMode::Save, cx))
                }),
        )
        .into_any_element()
}
