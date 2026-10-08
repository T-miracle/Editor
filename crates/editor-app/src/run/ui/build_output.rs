//! Generic provider preparation output uses native bounded scrolling and independent histories.
use super::*;
impl EditorApp {
    /// Hiding this presentation never stops a build; the unified dropdown restores the same history.
    pub(crate) fn render_build_output(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if let Some(config) = self.run_controls.selected().cloned() {
            if let Some(report) = self.plugin_configuration_bridge.jobs.report(&config.id) {
                let id = config.id.clone();
                let reload = self
                    .run_controls
                    .configuration_set()
                    .plugin_configurations
                    .get(&id)
                    .is_some_and(|data| data.template == "development");
                return Some(
                    v_flex()
                        .id("plugin-development-output")
                        .debug_selector(|| "plugin-development-output".into())
                        .h(px(180.))
                        .w_full()
                        .flex_shrink_0()
                        .gap_1()
                        .p_2()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().background)
                        .text_color(cx.theme().foreground)
                        .text_sm()
                        .child(
                            h_flex()
                                .justify_between()
                                .child(format!(
                                    "{} · {}",
                                    config.name,
                                    run_state_label(report.snapshot.state)
                                ))
                                .when(reload, |row| {
                                    row.child(
                                        Button::new("plugin-development-reload")
                                            .debug_selector(|| "plugin-development-reload".into())
                                            .label(t!("plugin_dev.reload"))
                                            .small()
                                            .ghost()
                                            .disabled(!report.snapshot.state.is_active())
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.plugin_configuration_bridge.jobs.reload(&id);
                                                cx.notify();
                                            })),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .id("plugin-development-output-text")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .child(report.output),
                        )
                        .into_any_element(),
                );
            }
        }
        let view = self.run_controls.preparation_output_view()?.clone();
        let name = self
            .run_controls
            .configuration(&view.config)
            .map(|config| config.name.clone())
            .unwrap_or(view.config);
        Some(
            v_flex()
                .id("run-build-output")
                .debug_selector(|| "run-build-output".into())
                .h(px(160.))
                .max_h(px(240.))
                .w_full()
                .flex_shrink_0()
                .gap_1()
                .p_2()
                .border_t_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().background)
                .text_color(cx.theme().foreground)
                // Preparation output follows the same rem-based global zoom as its native controls.
                .text_sm()
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .child(format!(
                            "{name} · {} · {}",
                            view.name,
                            run_state_label(view.snapshot.state)
                        ))
                        .child(
                            Button::new("run-build-hide")
                                .debug_selector(|| "run-build-hide".into())
                                .label(t!("run.hide_output"))
                                .small()
                                .ghost()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.run_controls.preparation_output_open = false;
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    div()
                        .id("run-build-output-text")
                        .debug_selector(|| "run-build-output-text".into())
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .whitespace_normal()
                        .child(view.snapshot.output),
                )
                .into_any_element(),
        )
    }
}
