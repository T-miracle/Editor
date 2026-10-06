//! Configuration and save-location dropdowns reuse the editor's retained native popup behavior.
use super::*;

/// These selectors edit the modal draft; neither one starts a program.
#[derive(Clone, Copy)]
pub(super) enum PickerKind {
    Configuration,
    Destination,
}

/// A bordered, full-width selector with a trailing chevron, matching B1's top and bottom controls.
pub(super) fn picker_button(
    app: &Entity<EditorApp>,
    kind: PickerKind,
    label: String,
    cx: &mut gpui_kit::App,
) -> Button {
    let id = match kind {
        PickerKind::Configuration => "run-config-existing",
        PickerKind::Destination => "run-config-save-location",
    };
    let owner = app.clone();
    Button::new(id)
        .debug_selector(move || id.into())
        .ghost()
        .w_full()
        .min_w_0()
        .content_full_width()
        .border_1()
        .border_color(cx.theme().input)
        .accessibility_label(label.clone())
        .child(div().flex_1().min_w_0().truncate().child(label))
        .child(Icon::new(IconName::ChevronDown))
        .on_click(move |event, window, cx| {
            open_picker(&owner, kind, event.position(), window, cx);
        })
}

/// The popup snapshots option identities and retains focus/dismissal with the form that opened it.
/// A delayed selection from a superseded draft cannot modify the replacement form.
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
    let options = match kind {
        PickerKind::Configuration => app
            .read(cx)
            .run_controls
            .configurations()
            .iter()
            .map(|config| (config.id.clone(), config.name.clone()))
            .collect::<Vec<_>>(),
        PickerKind::Destination => vec![
            ("local".into(), t!("run.form_local").to_string()),
            ("shared".into(), t!("run.form_shared").to_string()),
        ],
    };
    let items = if options.is_empty() {
        vec![plugin_runtime::plugin_protocol::ui::MenuItem {
            id: "empty".into(),
            label: t!("run.menu_empty").to_string(),
            disabled: true,
            separator_before: false,
        }]
    } else {
        options
            .into_iter()
            .map(
                |(id, label)| plugin_runtime::plugin_protocol::ui::MenuItem {
                    id,
                    label,
                    disabled: false,
                    separator_before: false,
                },
            )
            .collect()
    };
    let owner = app.downgrade();
    let form_id = form.entity_id();
    let style = MenuStyle::current(cx);
    let popup = cx.new(|cx| {
        NativePopupMenu::new(
            items,
            style,
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
                    match kind {
                        PickerKind::Configuration => {
                            let key = state.workspace_key();
                            // Replace both the draft and its editing states, never only the visible labels.
                            state.run_form = Some(cx.new(|cx| {
                                RunConfigForm::open(
                                    &state.run_controls,
                                    &key,
                                    Some(&id),
                                    window,
                                    cx,
                                )
                            }));
                        }
                        PickerKind::Destination => {
                            form.update(cx, |form, cx| {
                                form.draft.share = id == "shared";
                                form.error = None;
                                cx.notify();
                            });
                        }
                    }
                    cx.notify();
                });
            },
            window,
            cx,
        )
        .width(match kind {
            PickerKind::Configuration => 320.,
            PickerKind::Destination => 160.,
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
