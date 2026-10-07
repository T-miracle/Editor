//! Generic provider preparation output uses native bounded scrolling and independent histories.
use super::*;
impl EditorApp {
    /// Hiding this presentation never stops a build; the unified dropdown restores the same history.
    pub(crate) fn render_build_output(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
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
