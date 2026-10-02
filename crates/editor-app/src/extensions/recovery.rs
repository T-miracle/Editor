//! Native recovery controls show bounded host diagnostics and enqueue independent worker restarts.
use super::*;
impl ExtensionPanel {
    pub(super) fn recovery_controls(
        &self,
        id: &str,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state = self.worker.state.lock().unwrap();
        let diagnostics = state.diagnostics.get(id).cloned().unwrap_or_default();
        let disabled = !state
            .entries
            .iter()
            .any(|entry| entry.manifest.id == id && entry.enabled);
        let services = state
            .service_states
            .iter()
            .filter(|(key, _)| key.starts_with(&format!("{id}/")))
            .map(|(key, value)| format!("{key}: {value}"))
            .collect::<Vec<_>>();
        let loading = state.progress.as_ref().is_some_and(|progress| {
            progress.id == id && progress.action == LifecycleAction::Restart
        });
        drop(state);
        let id = id.to_owned();
        v_flex()
            .px_5()
            .py_3()
            .gap_2()
            .child(
                div()
                    .debug_selector(|| "plugin-restart-action".into())
                    .child(
                        Button::new("plugin-restart")
                            .label("重启插件")
                            .outline()
                            .loading(loading)
                            .disabled(busy || disabled)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.queue_lifecycle(Work::Restart(id.clone()));
                                cx.notify();
                            })),
                    ),
            )
            .children(services.into_iter().map(|text| div().text_sm().child(text)))
            .children(diagnostics.into_iter().rev().take(8).map(|report| {
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "{} [{}] {}: {}",
                        report.plugin, report.scope, report.operation, report.message
                    ))
            }))
            .into_any_element()
    }
}
