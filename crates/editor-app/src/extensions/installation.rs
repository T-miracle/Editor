//! Installation feedback remains in a dialog while work runs independently from the UI event loop.
use super::*;

/// Observe the manager's publications while the native modal window owns its local controls and focus.
struct InstallationView {
    worker: Arc<Worker>,
    control: Option<plugin_runtime::InstallControl>,
    _subscription: Subscription,
}
impl Drop for InstallationView {
    fn drop(&mut self) {
        // Native close and Escape are also explicit cancellation while preparation is in flight.
        if self
            .worker
            .state
            .lock()
            .unwrap()
            .installation
            .as_ref()
            .is_some_and(|report| report.cancellable)
        {
            if let Some(control) = &self.control {
                control.cancel();
            }
        }
    }
}
impl Render for InstallationView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let report = self.worker.state.lock().unwrap().installation.clone();
        let message = report
            .as_ref()
            .map(|report| report.message.clone())
            .unwrap_or_else(|| "安装操作已结束。".into());
        let cancellable = report.is_some_and(|report| report.cancellable);
        v_flex()
            .size_full()
            .p_4()
            .gap_4()
            .child(
                div()
                    .id("plugin-install-progress")
                    .debug_selector(|| "plugin-install-progress".into())
                    .flex_1()
                    .child(message),
            )
            .child(
                h_flex().justify_end().child(
                    div()
                        .id("plugin-install-progress-close-region")
                        .debug_selector(|| "plugin-install-progress-close-region".into())
                        .child(
                            Button::new("plugin-install-progress-close")
                                .label(if cancellable {
                                    "取消安装"
                                } else {
                                    "关闭"
                                })
                                .outline()
                                .on_click(cx.listener(move |this, _, window, _| {
                                    if cancellable {
                                        if let Some(control) = &this.control {
                                            control.cancel();
                                        }
                                    } else {
                                        window.remove_window();
                                    }
                                })),
                        ),
                ),
            )
    }
}

impl ExtensionPanel {
    pub(super) fn open_installation_progress(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let worker = self.worker.clone();
        let owner = cx.entity();
        let view = cx.new(|cx| InstallationView {
            control: worker.state.lock().unwrap().install_control.clone(),
            worker: worker.clone(),
            _subscription: cx.observe(&owner, |_, _, cx| cx.notify()),
        });
        app_dialog::open_dialog_sized(
            "插件安装进度",
            560.,
            280.,
            move |content, _, _| content.child(view.clone()),
            cx,
        );
    }

    /// Only the current immutable plan may report readiness; a late previous-version callback is ignored.
    pub(crate) fn language_service_status(
        &self,
        key: &str,
        plan: &Arc<plugin_runtime::LanguageService>,
        message: String,
        cx: &mut Context<Self>,
    ) {
        let mut state = self.worker.state.lock().unwrap();
        if !state
            .language_services
            .get(key)
            .and_then(|item| item.as_ref().ok())
            .is_some_and(|current| Arc::ptr_eq(current, plan))
        {
            return;
        }
        state.service_states.insert(key.into(), message);
        let text = state
            .service_states
            .iter()
            .filter(|(key, _)| key.starts_with(&format!("{}/", plan.owner)))
            .map(|(key, value)| format!("{key}：{value}"))
            .collect::<Vec<_>>()
            .join("\n");
        if let Some(report) = &mut state.installation {
            if report.id == plan.owner && report.installed {
                report.message = format!("插件已安装。\n{text}");
            }
        }
        drop(state);
        cx.notify();
    }
}
