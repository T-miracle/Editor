//! B1 configuration presentation: one retained draft, a scrollable page and fixed save actions.
//! Native editing and popup behavior come from the editor's local gpui-base controls.
use super::*;

mod picker;
mod steps;
use picker::{PickerKind, picker_button};
use steps::render_step_rows;

/// Render the approved B1 regions in the owning modal, preserving the form's editing state.
/// The caller owns the modal lifetime; this function only reads the retained draft and builds controls.
pub(crate) fn render_run_config_form(
    app: &WeakEntity<EditorApp>,
    content: DialogContent,
    cx: &mut gpui_kit::App,
) -> DialogContent {
    let Some(app) = app.upgrade() else {
        return content;
    };
    let Some(form) = app.read(cx).run_form.clone() else {
        return content;
    };
    let (tab, error, shell, share, focus, popup) = {
        let state = form.read(cx);
        (
            state.tab,
            state.error.clone(),
            state.draft.shell,
            state.draft.share,
            state.tab_focus.clone(),
            state.picker.as_ref().map(|menu| menu.popup.clone()),
        )
    };
    let owner = app.clone();
    let tabs = crate::ui::controls::tab_strip(
        "run-config-tab",
        RunConfigTab::ALL
            .iter()
            .position(|candidate| *candidate == tab)
            .unwrap(),
        RunConfigTab::ALL.map(|candidate| (candidate.label().into(), !candidate.available())),
        [None; 4],
        &focus,
        move |index, _, cx| {
            owner.update(cx, |state, cx| {
                if let Some(form) = state.run_form.as_ref() {
                    form.update(cx, |form, cx| {
                        form.tab = RunConfigTab::ALL[index];
                        cx.notify();
                    });
                }
                cx.notify();
            });
        },
        cx,
    )
    .into_any_element();
    content.child(
        v_flex()
            .debug_selector(|| "run-config-form".into())
            .size_full()
            .min_h_0()
            .text_sm()
            .child(render_header(&app, cx))
            .child(div().px_3().flex_shrink_0().child(tabs))
            .child(render_page(&app, &form, tab, shell, error, cx))
            .child(render_footer(&app, share, cx))
            .when_some(popup, |layout, popup| layout.child(popup)),
    )
}

/// Only page content scrolls: the modal's selection, tabs and save controls have stable regions.
fn render_page(
    app: &Entity<EditorApp>,
    form: &Entity<RunConfigForm>,
    tab: RunConfigTab,
    shell: bool,
    error: Option<String>,
    cx: &mut gpui_kit::App,
) -> AnyElement {
    // The footer and selector stay reachable while long scripts, lists or errors scroll.
    v_flex()
        .id("run-config-page")
        .debug_selector(|| "run-config-page".into())
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .gap_3()
        .p_3()
        .when(
            !app.read(cx).run_controls.discovered_targets().is_empty(),
            |page| page.child(render_discovered_targets(&app, cx)),
        )
        .when(tab == RunConfigTab::Basic, |page| {
            page.child(render_mode(&app, shell))
        })
        .children(render_fields(&app, &form, tab, shell, cx))
        .when(tab == RunConfigTab::Build, |page| {
            page.child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(tab.hint()),
            )
            .children(
                form.read(cx)
                    .draft
                    .provider_build
                    .iter()
                    .map(|(_, step)| div().child(step.name.clone())),
            )
        })
        .when(tab == RunConfigTab::Debug, |page| {
            page.child(render_providers(&app, &form, cx))
        })
        .when(tab == RunConfigTab::Environment, |page| {
            page.child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(tab.hint()),
            )
        })
        .when_some(error, |page, message| {
            page.child(
                div()
                    .debug_selector(|| "run-config-error".into())
                    .text_color(cx.theme().danger)
                    .child(message),
            )
        })
        .into_any_element()
}

/// The top selector shows the current draft and its plugin/source instead of a saved-count placeholder.
fn render_header(app: &Entity<EditorApp>, cx: &mut gpui_kit::App) -> AnyElement {
    let form = app.read(cx).run_form.as_ref().unwrap().read(cx);
    let name = form.text(RunField::Name, cx);
    let label = if name.trim().is_empty() {
        t!("run.form_new_title").to_string()
    } else {
        name
    };
    let source = form
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
            if let Some(editor_core::RunTarget::Provided { provider, .. }) = &form.draft.provided {
                t!("run.form_source_provider", provider = provider).to_string()
            } else {
                t!("run.form_source_manual").to_string()
            }
        });
    let discover = app.clone();
    let create = app.clone();
    v_flex()
        .flex_shrink_0()
        .px_3()
        .py_3()
        .gap_2()
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .child(div().flex_1().min_w_0().child(picker_button(
                    app,
                    PickerKind::Configuration,
                    label,
                    cx,
                )))
                .child(
                    Button::new("run-config-discover")
                        .debug_selector(|| "run-config-discover".into())
                        .icon(IconName::Search)
                        .accessibility_label(t!("run.discover"))
                        .label(t!("run.discover"))
                        .border_1()
                        .border_color(cx.theme().input)
                        .on_click(move |_, _, cx| {
                            discover.update(cx, |state, cx| state.discover_run_targets(cx))
                        }),
                )
                .child(
                    Button::new("run-config-new")
                        .debug_selector(|| "run-config-new".into())
                        .icon(IconName::Plus)
                        .accessibility_label(t!("run.form_new"))
                        .label(t!("run.form_new"))
                        .border_1()
                        .border_color(cx.theme().input)
                        .on_click(move |_, window, cx| {
                            create.update(cx, |state, cx| {
                                let key = state.workspace_key();
                                state.run_form = Some(cx.new(|cx| {
                                    RunConfigForm::open(&state.run_controls, &key, None, window, cx)
                                }));
                                cx.notify();
                            })
                        }),
                ),
        )
        .child(
            div()
                .debug_selector(|| "run-config-source".into())
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(source),
        )
        .into_any_element()
}

/// An explicit mode choice only belongs to Basic; changing other pages cannot alter launch semantics.
fn render_mode(app: &Entity<EditorApp>, shell: bool) -> AnyElement {
    let owner = app.clone();
    h_flex()
        .items_center()
        .gap_2()
        .child(div().w(px(128.)).flex_shrink_0().child(t!("run.form_mode")))
        .child(
            div().w(px(220.)).max_w_full().child(
                crate::ui::controls::SegmentedTabs::new("run-config-mode")
                    .labels([t!("run.field_program"), t!("run.form_shell")])
                    .selected_index(usize::from(shell))
                    .on_change(move |index, _, cx| {
                        owner.update(cx, |state, cx| {
                            if let Some(form) = &state.run_form {
                                form.update(cx, |form, cx| {
                                    form.draft.shell = index == 1;
                                    form.error = None;
                                    cx.notify();
                                });
                            }
                            cx.notify();
                        });
                    }),
            ),
        )
        .into_any_element()
}

/// Select only fields owned by the active page; arguments remain literal rows, never a shell string.
fn page_fields(tab: RunConfigTab, shell: bool) -> &'static [RunField] {
    match tab {
        RunConfigTab::Build => &[RunField::Build, RunField::Prelaunch],
        RunConfigTab::Environment => &[RunField::Environment, RunField::ToolPaths],
        RunConfigTab::Debug => &[RunField::Breakpoints],
        RunConfigTab::Basic if shell => &[
            RunField::Name,
            RunField::Program,
            RunField::Arguments,
            RunField::Script,
            RunField::Directory,
        ],
        RunConfigTab::Basic => &[
            RunField::Name,
            RunField::Program,
            RunField::Arguments,
            RunField::Directory,
        ],
    }
}

/// Uniform left labels and right native inputs keep the draft, IME, multiline selection and tab order.
fn render_fields(
    app: &Entity<EditorApp>,
    form: &Entity<RunConfigForm>,
    tab: RunConfigTab,
    shell: bool,
    cx: &mut gpui_kit::App,
) -> Vec<AnyElement> {
    page_fields(tab, shell)
        .iter()
        .copied()
        .map(|field| {
            let state = form.read(cx);
            let label = match (field, shell) {
                (RunField::Program, true) => t!("run.field_interpreter").to_string(),
                (RunField::Arguments, true) => t!("run.field_interpreter_arguments").to_string(),
                _ => field.label(),
            };
            let input = state
                .inputs
                .iter()
                .find(|(key, _)| *key == field)
                .map(|(_, input)| input.clone());
            let textarea = state
                .textareas
                .iter()
                .find(|(key, _)| *key == field)
                .map(|(_, input)| input.clone());
            let value = state.text(field, cx);
            let editor = if matches!(field, RunField::Build | RunField::Prelaunch) {
                render_step_rows(app, field, shell, cx)
            } else if let Some(input) = textarea {
                crate::ui::controls::Textarea::new(&input).into_any_element()
            } else if let Some(input) = input {
                crate::ui::controls::Input::new(&input).into_any_element()
            } else {
                div().child(value).into_any_element()
            };
            h_flex()
                .gap_2()
                .items_start()
                .child(div().w(px(128.)).flex_shrink_0().pt_1().child(label))
                .child(
                    div()
                        .debug_selector(move || field.selector().into())
                        .flex_1()
                        .min_w_0()
                        .child(editor),
                )
                .into_any_element()
        })
        .collect()
}

/// Provider selection reads current availability, so missing providers retain their visible reason.
fn render_providers(
    app: &Entity<EditorApp>,
    form: &Entity<RunConfigForm>,
    cx: &mut gpui_kit::App,
) -> AnyElement {
    let chosen = form.read(cx).draft.provider.clone();
    let providers = app
        .read(cx)
        .extensions
        .read(cx)
        .run_providers()
        .unwrap_or_default();
    let mut options = vec![(None, t!("run.provider_default").to_string(), None)];
    options.extend(providers.iter().map(|provider| {
        (
            Some(provider.plugin.clone()),
            provider.plugin.clone(),
            provider.unavailable.clone(),
        )
    }));
    let mut page = v_flex()
        .gap_2()
        .child(div().text_color(cx.theme().muted_foreground).child(t!(
            "run.provider_current",
            provider = providers
                .iter()
                .find(|provider| provider.selected)
                .map(|provider| provider.plugin.clone())
                .unwrap_or_else(|| t!("run.provider_none").to_string())
        )));
    for (plugin, label, unavailable) in options {
        let selector = format!(
            "run-provider-{}",
            plugin.clone().unwrap_or_else(|| "default".into())
        );
        let owner = app.clone();
        let requested = plugin.clone();
        let text = unavailable
            .as_ref()
            .map(|reason| format!("{label} ({reason})"))
            .unwrap_or(label);
        page = page.child(
            Button::new(selector.clone())
                .debug_selector(move || selector.clone())
                .disabled(unavailable.is_some())
                .when(chosen == plugin, |button| button.primary())
                .label(text)
                .on_click(move |_, _, cx| {
                    owner.update(cx, |state, cx| {
                        if let Some(form) = &state.run_form {
                            form.update(cx, |form, cx| {
                                form.draft.provider = requested.clone();
                                cx.notify();
                            });
                        }
                        state.apply_provider_choice(cx);
                        cx.notify();
                    });
                }),
        );
    }
    page.child(
        div()
            .text_color(cx.theme().muted_foreground)
            .child(t!("run.form_debug_panel_hint")),
    )
    .into_any_element()
}

/// The status and save bar never scroll with fields, and wraps its controls in a constrained window.
fn render_footer(app: &Entity<EditorApp>, share: bool, cx: &mut gpui_kit::App) -> AnyElement {
    let form = app.read(cx).run_form.as_ref().unwrap().read(cx);
    let count = form.rows_of(RunField::Prelaunch).len() + form.draft.provider_prelaunch.len();
    let debug = app
        .read(cx)
        .run_controls
        .configuration(&form.draft.id)
        .map(|config| {
            app.read(cx)
                .run_controls
                .debug_blocker(&config.id)
                .unwrap_or_else(|| t!("run.form_debug_ready").to_string())
        })
        .unwrap_or_else(|| t!("run.form_debug_after_save").to_string());
    let cancel = app.clone();
    let save = app.clone();
    v_flex()
        .flex_shrink_0()
        .child(
            h_flex()
                .debug_selector(|| "run-config-readiness".into())
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .border_t_1()
                .border_color(cx.theme().border)
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(t!("run.form_prelaunch_count", count = count))
                .child(div().flex_1().min_w_0().truncate().child(debug)),
        )
        .child(
            h_flex()
                .debug_selector(|| "run-config-footer".into())
                .items_center()
                .justify_between()
                .flex_wrap()
                .gap_2()
                .px_3()
                .py_3()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(
                    h_flex()
                        .debug_selector(|| "run-config-destination".into())
                        .items_center()
                        .gap_2()
                        .child(t!("run.form_destination"))
                        .child(
                            div().w(px(116.)).child(picker_button(
                                app,
                                PickerKind::Destination,
                                if share {
                                    t!("run.form_shared")
                                } else {
                                    t!("run.form_local")
                                }
                                .to_string(),
                                cx,
                            )),
                        ),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            Button::new("run-config-cancel")
                                .debug_selector(|| "run-config-cancel".into())
                                .accessibility_label(t!("run.form_cancel"))
                                .border_1()
                                .border_color(cx.theme().input)
                                .label(t!("run.form_cancel"))
                                .on_click(move |_, _, cx| {
                                    cancel.update(cx, |state, cx| state.close_run_form(cx))
                                }),
                        )
                        .child(
                            Button::new("run-config-save")
                                .debug_selector(|| "run-config-save".into())
                                .accessibility_label(t!("run.form_save"))
                                .primary()
                                .label(t!("run.form_save"))
                                .on_click(move |_, _, cx| {
                                    save.update(cx, |state, cx| state.commit_run_form(cx))
                                }),
                        ),
                ),
        )
        .into_any_element()
}

/// Render bounded discovered candidates in B1 without implicitly storing or executing them.
fn render_discovered_targets(app: &Entity<EditorApp>, cx: &mut gpui_kit::App) -> AnyElement {
    let targets = app.read(cx).run_controls.discovered_targets().to_vec();
    let mut list = v_flex()
        .id("run-config-targets")
        .max_h(px(120.))
        .overflow_y_scroll()
        .gap_1();
    for target in targets {
        let owner = app.clone();
        let identity = target.id.clone();
        let selector = format!("run-config-target-{}", target.id);
        list = list.child(
            Button::new(selector.clone())
                .debug_selector(move || selector.clone())
                .label(format!(
                    "{} · {} · {}",
                    target.label, target.provider, target.found_in
                ))
                .tooltip(t!("run.confirm_candidate").to_string())
                .on_click(move |_, window, cx| {
                    owner.update(cx, |state, cx| {
                        let workspace = state.workspace_key();
                        match state.run_controls.confirm_target(&identity, &workspace) {
                            Ok(id) => {
                                // Replace editing state atomically as well as the draft. Reusing old
                                // InputState values would save the previous target's visible fields.
                                state.run_form = Some(cx.new(|cx| {
                                    RunConfigForm::open(
                                        &state.run_controls,
                                        &workspace,
                                        Some(&id),
                                        window,
                                        cx,
                                    )
                                }));
                                state.status = t!(
                                    "run.target_added",
                                    name = state.run_controls.configuration(&id).unwrap().name
                                )
                                .into();
                            }
                            Err(error) => state.status = error,
                        }
                        cx.notify();
                    });
                }),
        );
    }
    list.into_any_element()
}
