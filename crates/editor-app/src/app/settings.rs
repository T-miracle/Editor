//! Renders editor appearance and layout settings.

use crate::*;

impl EditorApp {
    pub(super) fn render_settings_dialog(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let owner = view.clone();

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
                move |content, _, cx| {
                    let (dark, explorer_visible) = {
                        let settings = view.read(cx);
                        (settings.dark_theme, settings.explorer_visible)
                    };
                    let font_size = typography::font_size(cx) / px(1.);
                    let light_view = view.clone();
                    let dark_view = view.clone();
                    let decrease_view = view.clone();
                    let increase_view = view.clone();
                    let explorer_view = view.clone();
                    content.child(
                        v_flex()
                            .gap_6()
                            .py_2()
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
                                                                light_view.update(
                                                                    cx,
                                                                    |this, cx| {
                                                                        if this.dark_theme {
                                                                            this.toggle_theme(
                                                                                window, cx,
                                                                            );
                                                                        }
                                                                    },
                                                                );
                                                            }),
                                                    )
                                                    .child(
                                                        Button::new("settings-theme-dark")
                                                            .label(t!("settings.dark").to_string())
                                                            .when(dark, |button| button.primary())
                                                            .on_click(move |_, window, cx| {
                                                                dark_view.update(cx, |this, cx| {
                                                                    if !this.dark_theme {
                                                                        this.toggle_theme(
                                                                            window, cx,
                                                                        );
                                                                    }
                                                                });
                                                            }),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div().child(t!("settings.font_size").to_string()),
                                            )
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .gap_2()
                                                    .child(
                                                        Button::new("font-size-decrease")
                                                            .label("−")
                                                            .on_click(move |_, window, cx| {
                                                                decrease_view.update(
                                                                    cx,
                                                                    |this, cx| {
                                                                        typography::step_by(cx, -1);
                                                                        theme::sync_font_sizes(cx);
                                                                        this.refresh_dialog(cx);
                                                                        cx.notify();
                                                                        window.refresh();
                                                                    },
                                                                );
                                                            }),
                                                    )
                                                    .child(
                                                        div().w(px(52.)).text_center().child(
                                                            t!(
                                                                "settings.font_size_value",
                                                                size = format!("{font_size:.0}")
                                                            )
                                                            .to_string(),
                                                        ),
                                                    )
                                                    .child(
                                                        Button::new("font-size-increase")
                                                            .label("+")
                                                            .on_click(move |_, window, cx| {
                                                                increase_view.update(
                                                                    cx,
                                                                    |this, cx| {
                                                                        typography::step_by(cx, 1);
                                                                        theme::sync_font_sizes(cx);
                                                                        this.refresh_dialog(cx);
                                                                        cx.notify();
                                                                        window.refresh();
                                                                    },
                                                                );
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
                                            .child(
                                                div().child(
                                                    t!("settings.file_explorer").to_string(),
                                                ),
                                            )
                                            .child(
                                                Button::new("settings-explorer-visibility")
                                                    .label(if explorer_visible {
                                                        t!("settings.visible").to_string()
                                                    } else {
                                                        t!("settings.hidden").to_string()
                                                    })
                                                    .on_click(move |_, _, cx| {
                                                        explorer_view.update(cx, |this, cx| {
                                                            this.toggle_explorer(cx)
                                                        });
                                                    }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .child(t!("settings.resize_hint").to_string()),
                                    ),
                            ),
                    )
                },
                move |dialog, _, cx| {
                    owner.update(cx, |this, cx| {
                        this.dialog = Some(dialog);
                        cx.notify();
                    });
                },
            ))
    }
}
