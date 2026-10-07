//! Explicit host message publication keeps editing state and plugin logs out of user-facing history.

use crate::*;

/// Severity is chosen by the originating operation, independent of translated message wording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MessageLevel {
    Info,
    Warning,
    Error,
}

impl EditorApp {
    /// Retain a completed host result without replacing its existing status or notification UI.
    /// Plugin-owned results must continue through the plugin runtime log destination.
    pub(crate) fn record_host_message(
        &mut self,
        level: MessageLevel,
        message: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        self.messages
            .update(cx, |panel, cx| panel.push(level, message.into(), cx));
        cx.notify();
    }

    /// Publish a host operation result to both the transient status and the bounded message history.
    pub(crate) fn report_host_message(
        &mut self,
        level: MessageLevel,
        message: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        self.status = message.into();
        self.record_host_message(level, self.status.clone(), cx);
    }

    /// A closed dock region hides its leaves even when their own visibility flag remains enabled.
    pub(crate) fn messages_visible(&self, cx: &App) -> bool {
        let id = gpui_base::dock::PanelId::from(self.messages.entity_id());
        let area = self.dock_area.read(cx);
        self.messages.read(cx).is_visible()
            && ![
                gpui_base::dock::DockPlacement::Left,
                gpui_base::dock::DockPlacement::Right,
                gpui_base::dock::DockPlacement::Bottom,
            ]
            .into_iter()
            .any(|placement| {
                area.layout(placement)
                    .is_some_and(|tree| tree.panels().any(|panel| panel == id))
                    && !area.is_dock_open(placement)
            })
    }

    /// Toggle only this host panel; other panels sharing a dock retain their own visibility.
    pub(crate) fn toggle_messages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let visible = !self.messages_visible(cx);
        self.messages
            .update(cx, |panel, cx| panel.set_visible(visible, cx));
        if self.messages.read(cx).is_visible() {
            let id = gpui_base::dock::PanelId::from(self.messages.entity_id());
            self.dock_area.update(cx, |area, cx| {
                for placement in [
                    gpui_base::dock::DockPlacement::Left,
                    gpui_base::dock::DockPlacement::Right,
                    gpui_base::dock::DockPlacement::Bottom,
                ] {
                    if area
                        .layout(placement)
                        .is_some_and(|tree| tree.panels().any(|panel| panel == id))
                        && !area.is_dock_open(placement)
                    {
                        area.toggle_dock(placement, window, cx);
                    }
                }
                // Opening must refresh Base's active group as well as this panel's visibility.
                area.select_panel(id, window, cx);
                cx.notify();
            });
        }
    }
}
