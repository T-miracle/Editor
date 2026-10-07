//! Composes shortcut state with local Base-backed controls and localized descriptions.

use super::*;
use crate::ui::controls::{
    Icon, Input, shortcut_footer, shortcut_keycaps, shortcut_list, shortcut_modal, shortcut_row,
    shortcut_search, shortcut_tabs,
};

impl Render for ShortcutPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.search.read(cx).value().to_lowercase();
        // A draft stays on screen while text filtering changes; discarding it remains explicit.
        let operations = self
            .operations
            .iter()
            .filter(|operation| {
                self.is_editing(&operation.id)
                    || (operation.scope
                        == if self.tab == 0 {
                            Scope::Panel
                        } else {
                            Scope::Global
                        }
                        && if self.key_search {
                            self.capture.strokes.is_empty()
                                || operation
                                    .defaults
                                    .iter()
                                    .any(|binding| binding.starts_with(&self.capture.strokes))
                        } else {
                            operation.title.to_lowercase().contains(&query)
                        })
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut rows = Vec::new();
        for operation in operations {
            let selector = match &operation.target {
                catalog::Target::Native { action, .. } => {
                    format!("shortcut-operation-{}", action.name())
                }
                catalog::Target::Plugin { plugin, command } => {
                    format!("shortcut-operation-{plugin}/{command}")
                }
            };
            let suspended = cx
                .try_global::<engine::BindingEngine>()
                .is_some_and(|engine| !engine.suspended_conflicts(&operation.id).is_empty());
            let conflict_selector = match &operation.target {
                catalog::Target::Plugin { plugin, command } => {
                    format!("shortcut-conflict-{plugin}/{command}")
                }
                catalog::Target::Native { action, .. } => {
                    format!("shortcut-conflict-{}", action.name())
                }
            };
            let description = div()
                .debug_selector(move || selector.clone())
                .flex()
                .flex_col()
                .gap_1()
                .child(div().truncate().child(operation.title.clone()))
                .when(self.is_editing(&operation.id), |description| {
                    description.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("shortcuts.edit.press_new").to_string()),
                    )
                })
                .when(suspended, |description| {
                    // The retained keycaps describe configuration, not an active conflicting key.
                    description.child(
                        div()
                            .debug_selector(move || conflict_selector.clone())
                            .text_xs()
                            .text_color(cx.theme().danger)
                            .child(t!("shortcuts.restored_conflict").to_string()),
                    )
                })
                .into_any_element();
            let binding = self.binding_controls(&operation, cx);
            let draft = self.render_draft(&operation.id, cx);
            rows.push(shortcut_row(
                operation.id.clone(),
                description,
                binding,
                draft,
                cx,
            ));
        }
        if rows.is_empty() {
            rows.push(
                div()
                    .debug_selector(|| "shortcuts-empty".into())
                    .p_4()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("shortcuts.empty").to_string())
                    .into_any_element(),
            );
        }
        let panel = cx.entity().downgrade();
        let tabs = shortcut_tabs(
            self.tab,
            [
                t!("shortcuts.panel_tab").to_string().into(),
                t!("shortcuts.global_tab").to_string().into(),
            ],
            &self.tabs_focus,
            move |tab, window, cx| {
                let _ = panel.update(cx, |panel, cx| {
                    panel.request_edit_intent(editing::Intent::Tab(tab), window, cx);
                });
            },
            cx,
        );
        let field = div()
            .id("shortcuts-key-search")
            .debug_selector(|| "shortcuts-search".into())
            .h(px(36.))
            .when(self.key_search, |field| {
                field
                    .flex()
                    .items_center()
                    .px_2()
                    // Keep long recorded sequences inside the fixed search height, as text input
                    // does. Horizontal scrolling exposes every cap without overlapping the list.
                    .overflow_x_scroll()
                    .child(if self.capture.strokes.is_empty() {
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("shortcuts.press_keys").to_string())
                            .into_any_element()
                    } else {
                        shortcut_keycaps(&capture::display(&self.capture.strokes), cx)
                            .debug_selector(|| "shortcuts-search-keycaps".into())
                            .flex_nowrap()
                            .flex_shrink_0()
                            .into_any_element()
                    })
            })
            .when(!self.key_search, |field| {
                field.child(
                    Input::new(&self.search)
                        .appearance(false)
                        .h(px(36.))
                        .text_base(),
                )
            })
            .into_any_element();
        let toggle = Button::new("shortcuts-capture")
            .debug_selector(|| "shortcuts-capture".into())
            .compact()
            .outline()
            .w(px(44.))
            .h(px(40.))
            .disabled(self.confirm.is_some())
            .icon(Icon::default().path("icons/keyboard.svg"))
            .accessibility_label(t!("shortcuts.capture").to_string())
            .tooltip(t!("shortcuts.capture").to_string())
            .on_click(cx.listener(Self::toggle_capture_click))
            .into_any_element();
        let recording =
            (self.key_search && self.draft.is_none()) || self.is_recording_binding(window);
        let navigation = div()
            .flex()
            .items_center()
            .gap_2()
            .when(recording, |row| row.opacity(0.5))
            .child(t!("shortcuts.switch_tabs").to_string())
            .child(shortcut_keycaps(&["Alt+←".into()], cx))
            .child(shortcut_keycaps(&["Alt+→".into()], cx))
            .into_any_element();
        let footer = shortcut_footer(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(shortcut_keycaps(&["Esc".into()], cx))
                .child(if recording {
                    t!("shortcuts.help.capture").to_string()
                } else if self.draft.is_some() {
                    t!("shortcuts.help.edit").to_string()
                } else {
                    t!("shortcuts.help.close").to_string()
                })
                .into_any_element(),
            navigation,
            cx,
        );
        let content = div()
            .key_context("ShortcutPanel")
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .child(tabs)
            .child(shortcut_search(field, toggle, self.key_search, cx))
            .child(shortcut_list(rows, &self.scroll, cx))
            .children(self.render_edit_confirmation(cx))
            .child(footer)
            .into_any_element();
        let panel = cx.entity().downgrade();
        shortcut_modal(
            self.focus.clone(),
            content,
            move |window, cx| {
                let _ = panel.update(cx, |panel, cx| {
                    panel.request_edit_intent(editing::Intent::Close, window, cx);
                });
                false
            },
            window,
            cx,
        )
    }
}

impl ShortcutPanel {
    /// Forward Base's pointer/keyboard activation to the recording state.
    fn toggle_capture_click(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_edit_intent(editing::Intent::ToggleCapture, window, cx);
    }
}
