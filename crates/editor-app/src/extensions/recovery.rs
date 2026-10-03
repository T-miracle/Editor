//! Restart controls and current runtime publication have separate locations in plugin management.
use super::*;
use crate::ui::controls::StatusIcon;
use plugin_runtime::logs::{LogLevel, LogRecord};
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

    /// Highest unread anomaly supplies the tab badge; ordinary output never raises an alert.
    pub(super) fn log_badge(&self, id: &str) -> Option<StatusIcon> {
        severity_icon(self.runtime_logs().unread_severity(id))
    }

    /// Render a snapshot and acknowledge only its entries after the page is displayed.
    /// A switch, closed dialog or concurrent arrival cannot make an old view read new records.
    pub(super) fn runtime_status(&mut self, id: &str, cx: &mut Context<Self>) -> AnyElement {
        let records = self.runtime_logs().records(id);
        if self
            .manager_log_view
            .as_ref()
            .map(|(plugin, _)| plugin.as_str())
            != Some(id)
        {
            let through = records.last().map_or(0, |record| record.id);
            self.manager_log_view = Some((id.to_owned(), through));
            let owner = cx.entity().downgrade();
            let plugin = id.to_owned();
            cx.defer(move |cx| {
                let _ = owner.update(cx, |panel, cx| {
                    if panel.manager_open
                        && panel.manager_detail_tab == management::DetailTab::RuntimeLog
                        && panel.manager_selected.as_deref() == Some(&plugin)
                        && panel.manager_log_view.as_ref() == Some(&(plugin.clone(), through))
                        && panel.runtime_logs().view_through(&plugin, through)
                    {
                        cx.notify();
                    }
                });
            });
        }
        let mut log = v_flex()
            .id("plugin-runtime-log")
            .debug_selector(|| "plugin-runtime-log".into())
            .p_5()
            .gap_4()
            .w_full()
            .min_w(px(0.));
        if records.is_empty() {
            log = log.child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("plugins.no_runtime_logs").to_string()),
            );
        }
        for record in records {
            let badge = severity_icon(Some(record.level));
            let color = badge.map_or(cx.theme().foreground, |badge| badge.color(cx));
            let record_id = record.id;
            let source_selector = if record.source.starts_with("host.operation") {
                "plugin-runtime-operation-error"
            } else if record.source.starts_with("language") || record.source.starts_with("lsp") {
                "plugin-runtime-service-status"
            } else {
                "plugin-runtime-diagnostic"
            };
            log = log.child(
                v_flex()
                    .id(SharedString::from(format!("plugin-log-record-{record_id}")))
                    .debug_selector(move || format!("plugin-log-record-{record_id}").into())
                    .w_full()
                    .min_w(px(0.))
                    .gap_1()
                    .pb_3()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .text_sm()
                            .when_some(badge, |row, badge| row.child(badge.icon(cx)))
                            .child(
                                div()
                                    .font_semibold()
                                    .text_color(cx.theme().foreground)
                                    .px_1()
                                    .rounded_sm()
                                    .when_some(badge, |tag, _| tag.bg(color.opacity(0.12)))
                                    .child(level_label(record.level)),
                            )
                            .child(
                                div()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(log_time(&record)),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{} · {}", record.plugin, record.source)),
                    )
                    .child(
                        div()
                            .debug_selector(move || source_selector.into())
                            .relative()
                            .min_w(px(0.))
                            // Severity stays in the icon/tint; long text keeps readable contrast in both themes.
                            .text_color(cx.theme().foreground)
                            .child(record.message)
                            .child(self.read_visible_log(id, record_id, cx)),
                    ),
            );
        }
        log.into_any_element()
    }

    /// The paint mask includes detail scrolling; offscreen arrivals stay unread without auto-follow.
    fn read_visible_log(&self, plugin: &str, record_id: u64, cx: &Context<Self>) -> AnyElement {
        let owner = cx.entity().downgrade();
        let plugin = plugin.to_owned();
        gpui_kit::canvas(
            |bounds, window, _| bounds.intersects(&window.content_mask().bounds),
            move |_, visible, _, cx| {
                if visible {
                    cx.defer(move |cx| {
                        let _ = owner.update(cx, |panel, cx| {
                            if panel.manager_open
                                && panel.manager_detail_tab == management::DetailTab::RuntimeLog
                                && panel.manager_selected.as_deref() == Some(&plugin)
                                && panel.runtime_logs().mark_read(&plugin, &[record_id])
                            {
                                cx.notify();
                            }
                        });
                    });
                }
            },
        )
        .absolute()
        .inset_0()
        .into_any_element()
    }
}

/// Map runtime severity to shared local artwork without coupling controls to plugin runtime types.
pub(crate) fn severity_icon(level: Option<LogLevel>) -> Option<StatusIcon> {
    match level {
        Some(LogLevel::Warning) => Some(StatusIcon::Warning),
        Some(LogLevel::Error) => Some(StatusIcon::Error),
        _ => None,
    }
}

/// Normal records retain a readable level even though they have no unread alert icon.
pub(crate) fn level_label(level: LogLevel) -> String {
    match level {
        LogLevel::Info => t!("plugins.log_info"),
        LogLevel::Warning => t!("plugins.log_warning"),
        LogLevel::Error => t!("plugins.log_error"),
    }
    .to_string()
}

/// An explicit UTC clock preserves event time without adding a timezone dependency.
pub(crate) fn log_time(record: &LogRecord) -> String {
    let time = record
        .time
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = time.as_secs() % 86_400;
    format!(
        "{:02}:{:02}:{:02}.{:03} UTC",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60,
        time.subsec_millis()
    )
}
