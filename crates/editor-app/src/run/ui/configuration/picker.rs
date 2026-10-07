//! Native popup choices retain keyboard/focus behavior and bind callbacks to the opening form.
use super::*;

/// Every choice edits the draft; target discovery and default preparation keep their existing APIs.
#[derive(Clone, Copy)]
pub(super) enum PickerKind {
    Target,
    Destination,
    New,
    More,
    Provider,
    Reference,
}

/// The same native selector is used for targets, scope and optional providers.
pub(super) fn picker_button(
    app: &Entity<EditorApp>,
    kind: PickerKind,
    label: String,
    cx: &gpui_kit::App,
) -> Button {
    let id = match kind {
        PickerKind::Target => "run-config-target-picker",
        PickerKind::Destination => "run-config-save-location",
        PickerKind::New => "run-config-new",
        PickerKind::More => "run-config-actions",
        PickerKind::Provider => "run-config-provider",
        PickerKind::Reference => "run-step-reference",
    };
    let owner = app.clone();
    Button::new(id)
        .debug_selector(move || id.into())
        .ghost()
        .content_full_width()
        .w_full()
        .min_w_0()
        .border_1()
        .border_color(cx.theme().input)
        .accessibility_label(if label.is_empty() {
            match kind {
                PickerKind::New => t!("run.form_new").into(),
                PickerKind::More => t!("run.form_actions").into(),
                _ => t!("run.form_choose_target").into(),
            }
        } else {
            label.clone()
        })
        .when(!label.is_empty(), |button| {
            button.child(div().flex_1().min_w_0().truncate().child(label))
        })
        .child(Icon::new(match kind {
            PickerKind::New => IconName::Plus,
            PickerKind::More => IconName::Ellipsis,
            _ => IconName::ChevronDown,
        }))
        .on_click(move |event, window, cx| open_picker(&owner, kind, event.position(), window, cx))
}

/// IDs, availability and provider provenance are captured, never inferred from displayed names.
fn options(
    app: &Entity<EditorApp>,
    kind: PickerKind,
    cx: &gpui_kit::App,
) -> Vec<(String, String, bool)> {
    match kind {
        PickerKind::Target | PickerKind::New => {
            let mut result = app
                .read(cx)
                .run_controls
                .discovered_targets()
                .iter()
                .map(|target| {
                    (
                        format!("target:{}", target.id),
                        format!("{} · {}", target.label, target.provider),
                        false,
                    )
                })
                .collect::<Vec<_>>();
            result.extend([
                ("new:program".into(), t!("run.field_program").into(), false),
                ("new:script".into(), t!("run.form_shell").into(), false),
                ("discover".into(), t!("run.discover").into(), false),
            ]);
            result
        }
        PickerKind::Destination => vec![
            ("local".into(), t!("run.form_local").into(), false),
            ("shared".into(), t!("run.form_shared").into(), false),
        ],
        PickerKind::More => vec![
            ("copy".into(), t!("run.form_copy").into(), false),
            ("delete".into(), t!("run.form_delete").into(), false),
        ],
        PickerKind::Provider => {
            let mut result = vec![("default".into(), t!("run.provider_default").into(), false)];
            result.extend(
                app.read(cx)
                    .extensions
                    .read(cx)
                    .run_providers()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|provider| {
                        (
                            format!("provider:{}", provider.plugin),
                            provider
                                .unavailable
                                .as_ref()
                                .map(|reason| format!("{} ({reason})", provider.plugin))
                                .unwrap_or(provider.plugin),
                            provider.unavailable.is_some(),
                        )
                    }),
            );
            result
        }
        PickerKind::Reference => {
            let current = &app.read(cx).run_form.as_ref().unwrap().read(cx).draft.id;
            app.read(cx)
                .run_controls
                .configurations()
                .iter()
                .filter(|config| &config.id != current)
                .map(|config| (config.name.clone(), config.name.clone(), false))
                .collect()
        }
    }
}

/// A superseded form cannot receive a late menu action; dropping the form also revokes its popup.
fn open_picker(
    app: &Entity<EditorApp>,
    kind: PickerKind,
    position: Point<Pixels>,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let Some(form) = app.read(cx).run_form.clone() else {
        return;
    };
    let choices = options(app, kind, cx);
    let items = if choices.is_empty() {
        vec![plugin_runtime::plugin_protocol::ui::MenuItem {
            id: "empty".into(),
            label: t!("run.menu_empty").into(),
            disabled: true,
            separator_before: false,
        }]
    } else {
        choices
            .into_iter()
            .map(
                |(id, label, disabled)| plugin_runtime::plugin_protocol::ui::MenuItem {
                    id,
                    label,
                    disabled,
                    separator_before: false,
                },
            )
            .collect()
    };
    let owner = app.downgrade();
    let form_id = form.entity_id();
    let popup = cx.new(|cx| {
        NativePopupMenu::new(
            items,
            MenuStyle::current(cx),
            position,
            move |action, window, cx| {
                let plugin_runtime::plugin_protocol::ui::Action::Select(id) = action else {
                    return;
                };
                let _ = owner.update(cx, |state, cx| {
                    let Some(form) = state
                        .run_form
                        .clone()
                        .filter(|form| form.entity_id() == form_id)
                    else {
                        return;
                    };
                    form.update(cx, |form, cx| {
                        form.picker = None;
                        cx.notify();
                    });
                    match kind {
                        PickerKind::Target | PickerKind::New | PickerKind::More => {
                            let selection = if let Some(target) = id.strip_prefix("target:") {
                                Some(FormSelection::Candidate(target.into()))
                            } else {
                                match id.as_str() {
                                    "new:program" => Some(FormSelection::New(false)),
                                    "new:script" => Some(FormSelection::New(true)),
                                    "copy" => Some(FormSelection::Copy),
                                    "delete" => Some(FormSelection::Delete),
                                    "discover" => {
                                        state.discover_run_targets(cx);
                                        None
                                    }
                                    _ => None,
                                }
                            };
                            if let Some(selection) = selection {
                                state.select_run_form(selection, window, cx);
                            }
                        }
                        PickerKind::Destination => form.update(cx, |form, cx| {
                            form.draft.share = id == "shared";
                            form.error = None;
                            cx.notify();
                        }),
                        PickerKind::Provider => form.update(cx, |form, cx| {
                            form.draft.provider = id.strip_prefix("provider:").map(str::to_owned);
                            form.error = None;
                            cx.notify();
                        }),
                        PickerKind::Reference => {
                            if let Some(FormEditor::Step { state: editor, .. }) =
                                &form.read(cx).editor
                            {
                                editor.clone().update(cx, |editor, cx| {
                                    editor
                                        .reference
                                        .update(cx, |input, cx| input.set_value(id, window, cx))
                                });
                            }
                        }
                    }
                    cx.notify();
                });
            },
            window,
            cx,
        )
        .width(match kind {
            PickerKind::Destination => 160.,
            PickerKind::More => 200.,
            _ => 360.,
        })
    });
    let popup_id = popup.entity_id();
    form.update(cx, |form, cx| {
        let dismiss = cx.subscribe(&popup, move |form, _, _: &DismissEvent, cx| {
            if form
                .picker
                .as_ref()
                .is_some_and(|menu| menu.popup.entity_id() == popup_id)
            {
                form.picker = None;
                cx.notify();
            }
        });
        form.picker = Some(RunMenu {
            popup,
            _dismiss: dismiss,
        });
        cx.notify();
    });
}
