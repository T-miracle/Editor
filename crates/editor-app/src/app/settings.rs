//! Owns the settings navigation and the content shown by each category.

use crate::ui::controls::{Checkbox, DialogContent};
use crate::*;
use gpui_kit::{AnyElement, App, Entity, component::scroll::ScrollableElement};

/// Top-level settings categories shown in the navigation sidebar.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsSection {
    AppearanceAndBehavior,
    Keymap,
    Editor,
}

impl SettingsSection {
    /// Returns the localized category name used in the sidebar and page heading.
    fn label(self) -> String {
        match self {
            Self::AppearanceAndBehavior => t!("settings.appearance_and_behavior").to_string(),
            Self::Keymap => t!("settings.keymap").to_string(),
            Self::Editor => t!("settings.editor").to_string(),
        }
    }

    /// Provides a stable selector for interaction tests and accessibility tooling.
    fn selector(self) -> &'static str {
        match self {
            Self::AppearanceAndBehavior => "settings-nav-appearance",
            Self::Keymap => "settings-nav-keymap",
            Self::Editor => "settings-nav-editor",
        }
    }
}

impl EditorApp {
    /// Renders the settings trigger and connects its modal window to this app instance.
    pub(super) fn render_settings_dialog(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let owner = view.clone();
        let existing_owner = view.clone();

        div()
            .debug_selector(|| "settings-trigger".into())
            .child(app_dialog::app_dialog(
                Button::new("open-settings")
                    .icon(IconName::Settings)
                    .small()
                    .compact()
                    .ghost()
                    .tooltip(t!("settings.title").to_string()),
                t!("settings.title").to_string(),
                move |content, _, cx| render_settings_content(&view, content, cx),
                move |cx| existing_owner.read(cx).dialog_window,
                move |dialog, handle, _, cx| {
                    let weak_owner = owner.downgrade();
                    let window_id = handle.window_id();
                    let subscription = cx.on_window_closed(move |cx, closed_id| {
                        if closed_id == window_id {
                            let _ = weak_owner.update(cx, |this, cx| {
                                this.dialog = None;
                                this.dialog_window = None;
                                cx.notify();
                            });
                        }
                    });
                    owner.update(cx, |this, cx| {
                        this.dialog = Some(dialog);
                        this.dialog_window = Some(handle);
                        this._dialog_closed_subscription = Some(subscription);
                        cx.notify();
                    });
                },
            ))
    }
}

/// Places persistent category navigation beside the independently scrollable page.
fn render_settings_content(
    view: &Entity<EditorApp>,
    content: DialogContent,
    cx: &mut App,
) -> DialogContent {
    let selected = view.read(cx).settings_section;
    let nav = [
        SettingsSection::AppearanceAndBehavior,
        SettingsSection::Keymap,
        SettingsSection::Editor,
    ]
    .into_iter()
    .map(|section| {
        let nav_view = view.clone();
        div()
            .id(section.selector())
            .debug_selector(move || section.selector().into())
            .w_full()
            .h(px(34.))
            .px_3()
            .flex()
            .items_center()
            .rounded(cx.theme().radius)
            .cursor_pointer()
            .when(selected == section, |item| {
                item.bg(cx.theme().list_active)
                    .text_color(cx.theme().foreground)
                    .font_semibold()
            })
            .when(selected != section, |item| {
                item.text_color(cx.theme().sidebar_foreground)
                    .hover(|style| style.bg(cx.theme().list_hover))
            })
            .child(section.label())
            .on_click(move |_, _, cx| {
                nav_view.update(cx, |this, cx| {
                    this.settings_section = section;
                    this.refresh_dialog(cx);
                    cx.notify();
                });
            })
    });

    content.h_full().child(
        h_flex()
            .h_full()
            .w_full()
            .items_start()
            .child(
                v_flex()
                    .id("settings-sidebar")
                    .w(px(214.))
                    .h_full()
                    .flex_shrink_0()
                    .gap_1()
                    .p_2()
                    .border_r_1()
                    .border_color(cx.theme().sidebar_border)
                    .bg(cx.theme().sidebar)
                    .overflow_y_scrollbar()
                    .children(nav),
            )
            .child(
                v_flex()
                    .id("settings-page")
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .gap_5()
                    .p_5()
                    .overflow_y_scrollbar()
                    .child(div().text_lg().font_semibold().child(selected.label()))
                    .child(render_section(view, selected, cx)),
            ),
    )
}

/// Keeps each category's controls in one content pane.
fn render_section(view: &Entity<EditorApp>, section: SettingsSection, cx: &mut App) -> AnyElement {
    match section {
        SettingsSection::AppearanceAndBehavior => render_appearance(view, cx),
        SettingsSection::Keymap => render_keymap(cx),
        SettingsSection::Editor => render_editor(view, cx),
    }
}

/// Renders the existing theme and file explorer controls under appearance.
fn render_appearance(view: &Entity<EditorApp>, cx: &mut App) -> AnyElement {
    let (dark, explorer_visible, reveal_on_tab_switch) = {
        let settings = view.read(cx);
        (
            settings.dark_theme,
            settings.explorer_visible,
            settings.session_state.explorer_reveal_on_tab_switch,
        )
    };
    let light_view = view.clone();
    let dark_view = view.clone();
    let explorer_view = view.clone();
    let reveal_view = view.clone();

    v_flex()
        .gap_6()
        .child(
            v_flex()
                .gap_3()
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .child(t!("settings.appearance").to_string()),
                )
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .child(div().child(t!("settings.theme").to_string()))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new("settings-theme-light")
                                        .label(t!("settings.light").to_string())
                                        .when(!dark, |button| button.primary())
                                        .on_click(move |_, window, cx| {
                                            light_view.update(cx, |this, cx| {
                                                if this.dark_theme {
                                                    this.toggle_theme(window, cx);
                                                }
                                            });
                                        }),
                                )
                                .child(
                                    Button::new("settings-theme-dark")
                                        .label(t!("settings.dark").to_string())
                                        .when(dark, |button| button.primary())
                                        .on_click(move |_, window, cx| {
                                            dark_view.update(cx, |this, cx| {
                                                if !this.dark_theme {
                                                    this.toggle_theme(window, cx);
                                                }
                                            });
                                        }),
                                ),
                        ),
                ),
        )
        .child(
            v_flex()
                .gap_3()
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .child(t!("settings.layout").to_string()),
                )
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .child(div().child(t!("settings.file_explorer").to_string()))
                        .child(
                            Button::new("settings-explorer-visibility")
                                .label(if explorer_visible {
                                    t!("settings.visible").to_string()
                                } else {
                                    t!("settings.hidden").to_string()
                                })
                                .on_click(move |_, _, cx| {
                                    explorer_view.update(cx, |this, cx| this.toggle_explorer(cx));
                                }),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!("settings.resize_hint").to_string()),
                )
                .child(
                    div()
                        .id("settings-explorer-reveal")
                        .debug_selector(|| "settings-explorer-reveal".into())
                        .child(
                            Checkbox::new("settings-explorer-reveal-checkbox")
                                .label(t!("settings.explorer_reveal_on_tab_switch").to_string())
                                .checked(reveal_on_tab_switch)
                                .on_change(move |checked, _, cx| {
                                    // Apply immediately and retain the choice when reopening this workspace.
                                    reveal_view.update(cx, |this, cx| {
                                        this.session_state.explorer_reveal_on_tab_switch = *checked;
                                        this.persist_session();
                                        this.refresh_dialog(cx);
                                        cx.notify();
                                    });
                                }),
                        ),
                ),
        )
        .into_any_element()
}

/// Shows the category without inventing shortcut editing before a keymap model exists.
fn render_keymap(cx: &mut App) -> AnyElement {
    div()
        .debug_selector(|| "settings-keymap-empty".into())
        .text_color(cx.theme().muted_foreground)
        .child(t!("settings.keymap_empty").to_string())
        .into_any_element()
}

/// Renders the shared interface and editor font size control.
fn render_editor(view: &Entity<EditorApp>, cx: &mut App) -> AnyElement {
    let font_size = typography::font_size(cx) / px(1.);
    let decrease_view = view.clone();
    let increase_view = view.clone();

    v_flex()
        .gap_3()
        .child(
            h_flex()
                .debug_selector(|| "settings-font-size".into())
                .items_center()
                .justify_between()
                .child(div().child(t!("settings.font_size").to_string()))
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(Button::new("font-size-decrease").label("−").on_click(
                            move |_, window, cx| {
                                decrease_view.update(cx, |this, cx| {
                                    typography::step_by(cx, -1);
                                    theme::sync_font_sizes(cx);
                                    this.refresh_dialog(cx);
                                    cx.notify();
                                    window.refresh();
                                });
                            },
                        ))
                        .child(
                            div().w(px(52.)).text_center().child(
                                t!("settings.font_size_value", size = format!("{font_size:.0}"))
                                    .to_string(),
                            ),
                        )
                        .child(Button::new("font-size-increase").label("+").on_click(
                            move |_, window, cx| {
                                increase_view.update(cx, |this, cx| {
                                    typography::step_by(cx, 1);
                                    theme::sync_font_sizes(cx);
                                    this.refresh_dialog(cx);
                                    cx.notify();
                                    window.refresh();
                                });
                            },
                        )),
                ),
        )
        .into_any_element()
}
