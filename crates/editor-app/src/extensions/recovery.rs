//! Restart controls and current runtime publication have separate locations in plugin management.
use super::*;
impl ExtensionPanel {
    /// Enqueue a restart through the existing lifecycle worker; only an enabled instance may restart.
    pub(super) fn restart_action(
        &self,
        id: &str,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state = self.worker.state.lock().unwrap();
        let disabled = !state
            .entries
            .iter()
            .any(|entry| entry.manifest.id == id && entry.enabled);
        let loading = state.progress.as_ref().is_some_and(|progress| {
            progress.id == id && progress.action == LifecycleAction::Restart
        });
        drop(state);
        let id = id.to_owned();
        div()
            .debug_selector(|| "plugin-restart-action".into())
            .child(
                Button::new("plugin-restart")
                    .label(t!("plugins.restart").to_string())
                    .outline()
                    .loading(loading)
                    .disabled(busy || disabled)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.queue_lifecycle(Work::Restart(id.clone()));
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// Show only this plugin's existing service states and diagnostics, without creating a second log store.
    /// Complete run history and shared unread markers are delivered by the following log ticket.
    pub(super) fn runtime_status(&self, id: &str, cx: &Context<Self>) -> AnyElement {
        let state = self.worker.state.lock().unwrap();
        let diagnostics = state.diagnostics.get(id).cloned().unwrap_or_default();
        let services = state
            .service_states
            .iter()
            .filter(|(key, _)| key.starts_with(&format!("{id}/")))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<Vec<_>>();
        drop(state);
        let error = self
            .entries
            .iter()
            .find(|entry| entry.manifest.id == id)
            .and_then(|entry| entry.error.as_ref());
        let operation_error = self
            .status
            .as_ref()
            .filter(|status| status.plugin.as_deref() == Some(id));
        let mut log = v_flex()
            .id("plugin-runtime-log")
            .debug_selector(|| "plugin-runtime-log".into())
            .p_5()
            .gap_3()
            .w_full()
            .min_w(px(0.));
        if services.is_empty()
            && diagnostics.is_empty()
            && error.is_none()
            && operation_error.is_none()
        {
            log = log.child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("plugins.no_runtime_logs").to_string()),
            );
        }
        for (service, status) in services {
            log = log.child(
                v_flex()
                    .debug_selector(|| "plugin-runtime-service-status".into())
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(service),
                    )
                    .child(div().child(status)),
            );
        }
        for report in diagnostics.into_iter().rev().take(8) {
            log = log.child(
                v_flex()
                    .debug_selector(|| "plugin-runtime-diagnostic".into())
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "{} [{}] {}",
                                report.plugin, report.scope, report.operation
                            )),
                    )
                    .child(div().text_color(cx.theme().danger).child(report.message)),
            );
        }
        if let Some(error) = error {
            log = log.child(div().text_color(cx.theme().danger).child(error.clone()));
        }
        if let Some(status) = operation_error {
            log = log.child(
                div()
                    .debug_selector(|| "plugin-runtime-operation-error".into())
                    .text_color(cx.theme().danger)
                    .child(status.message.clone()),
            );
        }
        log.into_any_element()
    }
}
