//! Tracks startup plugin loading and presents its progress beside the status bar.

use crate::ui::controls::Spinner;
use crate::*;

/// The status bar separates in-progress work from actionable failures.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PluginPopupKind {
    Loading,
    Error,
}

impl EditorApp {
    /// Persist authority outside the repository and immediately withdraw all workspace presentation.
    pub(crate) fn set_workspace_trusted(&mut self, trusted: bool, cx: &mut Context<Self>) {
        self.session_state.workspace_trusted = trusted;
        self.persist_session();
        self.extensions
            .update(cx, |panel, cx| panel.set_workspace_trusted(trusted, cx));
        // The editor window (not the settings dialog) owns dock and editor synchronization.
        self.pending_contribution_sync = true;
        self.refresh_dialog(cx);
        cx.notify();
    }

    /// Reconcile every installed declaration through the same generation-checked provider registry.
    pub(crate) fn sync_runtime_contributions(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        apply_theme(&theme::active_theme(self.dark_theme), cx);
        self.sync_dynamic_languages(cx);
        self.reset_syntax_diagnostics(cx);
        cx.notify();
    }

    pub(crate) fn plugin_count(&self, kind: PluginPopupKind, cx: &App) -> usize {
        let runtime = self.extensions.read(cx);
        self.dynamic_language_status(kind).len()
            + self.language_service_details(kind).len()
            + match kind {
                PluginPopupKind::Loading => runtime.startup.len(),
                PluginPopupKind::Error => runtime
                    .entries
                    .iter()
                    .filter(|entry| entry.error.is_some())
                    .count(),
            }
    }

    /// Place the detail card above the pointer that activated its indicator.
    fn toggle_plugin_popup(
        &mut self,
        kind: PluginPopupKind,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.plugin_popup = match self.plugin_popup {
            Some((current, _)) if current == kind => None,
            _ => Some((kind, event.position)),
        };
        cx.notify();
    }

    /// Render the supplied loading and error artwork at the far right of the bar.
    pub(crate) fn render_plugin_indicator(
        &self,
        kind: PluginPopupKind,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let count = self.plugin_count(kind, cx);
        let (id, label) = match kind {
            PluginPopupKind::Loading => ("plugin-loading-indicator", t!("plugins.loading")),
            PluginPopupKind::Error => ("plugin-error-indicator", t!("plugins.error")),
        };
        h_flex()
            .id(id)
            .debug_selector(move || id.into())
            .items_center()
            .gap_1()
            .px_2()
            .rounded_sm()
            .hover(|style| style.bg(cx.theme().secondary_hover))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event, _, cx| this.toggle_plugin_popup(kind, event, cx)),
            )
            .child(match kind {
                PluginPopupKind::Loading => Spinner::new()
                    .icon(
                        Icon::default()
                            .data(include_bytes!("../../assets/plugin-status/loading.svg")),
                    )
                    .small()
                    .into_any_element(),
                PluginPopupKind::Error => Icon::default()
                    .data(include_bytes!("../../assets/plugin-status/error.svg"))
                    .small()
                    .into_any_element(),
            })
            .child(format!("{label} {count}"))
    }

    /// Keep clicks outside a plugin popup from reaching the dock or title bar beneath it.
    pub(crate) fn render_plugin_popup_blocker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.plugin_popup.is_none() {
            return div().into_any_element();
        }
        div()
            .id("plugin-popup-blocker")
            .debug_selector(|| "plugin-popup-blocker".into())
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .on_any_mouse_down(cx.listener(|this, _, _, cx| {
                this.plugin_popup = None;
                cx.stop_propagation();
                cx.notify();
            }))
            .into_any_element()
    }

    /// Show the selected plugin list at the click location, with failure reasons.
    pub(crate) fn render_plugin_popup(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let Some((kind, position)) = self.plugin_popup else {
            return div().into_any_element();
        };
        let width = px(340.).min(window.viewport_size().width - px(16.));
        let title = match kind {
            PluginPopupKind::Loading => t!("plugins.loading_list"),
            PluginPopupKind::Error => t!("plugins.error_list"),
        };
        // Runtime plugin names and startup failures share the existing status popup.
        let runtime = self.extensions.read(cx);
        let mut runtime_details: Vec<_> = match kind {
            PluginPopupKind::Loading => runtime
                .startup
                .values()
                .map(|name| (name.clone(), t!("plugins.loading").to_string()))
                .collect(),
            PluginPopupKind::Error => runtime
                .entries
                .iter()
                .filter_map(|entry| {
                    entry
                        .error
                        .as_ref()
                        .map(|error| (entry.manifest.name.clone(), error.clone()))
                })
                .collect(),
        };
        // GPUI measures the card before placing it above the clicked window point.
        runtime_details.extend(self.dynamic_language_status(kind));
        runtime_details.extend(self.language_service_details(kind));
        gpui_base::Positioner::side(Bounds::new(position, size(px(1.), px(1.))))
            .placement(gpui_base::Placement::Top)
            .align(gpui_base::Align::End)
            .offset(px(8.))
            .margin(px(8.))
            .occlude()
            .child(
                v_flex()
                    .id("plugin-status-popup")
                    .debug_selector(|| "plugin-status-popup".into())
                    .w(width)
                    .max_h(px(210.))
                    .overflow_y_scroll()
                    .p_3()
                    .gap_2()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().popover)
                    .text_color(cx.theme().foreground)
                    .shadow_md()
                    .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                    .child(div().font_semibold().child(title.to_string()))
                    .children(runtime_details.into_iter().map(|(name, detail)| {
                        v_flex()
                            .gap_1()
                            .child(div().font_semibold().child(name))
                            .child(div().text_xs().child(detail))
                    })),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests;
