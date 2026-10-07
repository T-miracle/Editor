//! Local notification cards own their appearance, dismissal and lifetime without modal focus.

use super::{Button, Icon};
use gpui_kit::component::{ActiveTheme as _, IconName, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    Context, InteractiveElement as _, IntoElement, MouseButton, ParentElement, Render,
    SharedString, StatefulInteractiveElement as _, Styled, Window, div, px,
};
use rust_i18n::t;
use std::time::Duration;

/// One replaceable informational card; it never takes focus from the editor or blocks other panels.
pub(crate) struct Notification {
    title: SharedString,
    message: SharedString,
    dismissed: bool,
}

impl Notification {
    /// File-operation failures remain readable until explicitly dismissed; they never take focus.
    pub(crate) fn persistent(
        title: impl Into<SharedString>,
        message: impl Into<SharedString>,
    ) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            dismissed: false,
        }
    }

    /// Display a message for 1.2 seconds; replacing the entity also replaces its expiration timer.
    pub(crate) fn new(
        title: impl Into<SharedString>,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1200))
                .await;
            // A dropped older card cannot dismiss the newer notification in the same window.
            let _ = this.update(cx, |this, cx| {
                this.dismissed = true;
                cx.notify();
            });
        })
        .detach();
        Self {
            title: title.into(),
            message: message.into(),
            dismissed: false,
        }
    }
}

impl Render for Notification {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.dismissed {
            return div().into_any_element();
        }
        h_flex()
            .id("local-notification")
            .debug_selector(|| "local-notification".into())
            .role(gpui_kit::Role::Status)
            .occlude()
            .w_full()
            .items_start()
            .gap_2()
            .p_3()
            .rounded(px(8.))
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().popover)
            .text_color(cx.theme().popover_foreground)
            .shadow_lg()
            // Only the card consumes pointer events; the surrounding editor remains usable.
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(
                Icon::new(IconName::Info)
                    .small()
                    .text_color(cx.theme().primary),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .gap_1()
                    .child(div().font_semibold().text_sm().child(self.title.clone()))
                    .child(div().text_sm().child(self.message.clone())),
            )
            .child(
                div()
                    .id("local-notification-close")
                    .debug_selector(|| "local-notification-close".into())
                    .flex_shrink_0()
                    .child(
                        Button::new("dismiss-local-notification")
                            .icon(IconName::Close)
                            .small()
                            .compact()
                            .ghost()
                            .tooltip(t!("notification.close").to_string())
                            .accessibility_label(t!("notification.close").to_string())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.dismissed = true;
                                cx.stop_propagation();
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }
}
