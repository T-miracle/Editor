//! Native host message history and its local dock appearance; Base owns focus, buttons and resizing.

use crate::{app::messages::MessageLevel, *};
use gpui_kit::SharedString;
use std::collections::VecDeque;

/// Current-run capacity and progressive disclosure have distinct responsibilities.
const HISTORY_LIMIT: usize = 500;
const DISPLAY_BATCH: usize = 20;

/// Shape supplied by the user; the local icon renderer resolves its size and theme color.
pub(crate) fn notification_icon() -> Icon {
    Icon::default()
        .data(include_bytes!("../../assets/status-icons/notification.svg").as_slice())
        .small()
}

/// Immutable receipt identity makes chronological ties and retained rows deterministic.
struct Message {
    id: u64,
    level: MessageLevel,
    text: String,
}

/// Owns one bounded history for the main window; session persistence stores only its presentation.
pub(crate) struct MessagePanel {
    visible: bool,
    focus: FocusHandle,
    records: VecDeque<Message>,
    next_id: u64,
    display_limit: usize,
    scroll: ScrollHandle,
    /// Base provides the current group through its lifecycle hook; hidden views otherwise lose render dependencies.
    group: Option<WeakEntity<gpui_base::dock::TabGroup>>,
}

impl MessagePanel {
    /// Start a fresh history regardless of saved layout; visibility is the only restored flag.
    pub(crate) fn new(visible: bool, cx: &mut Context<Self>) -> Self {
        Self {
            visible,
            focus: cx.focus_handle(),
            records: VecDeque::new(),
            next_id: 0,
            display_limit: DISPLAY_BATCH,
            scroll: ScrollHandle::new(),
            group: None,
        }
    }

    /// Append a user-facing host result, evicting only the oldest receipt above the global cap.
    pub(crate) fn push(&mut self, level: MessageLevel, text: String, cx: &mut Context<Self>) {
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("host message identity exhausted");
        self.records.push_back(Message {
            id: self.next_id,
            level,
            text,
        });
        if self.records.len() > HISTORY_LIMIT {
            self.records.pop_front();
        }
        cx.notify();
    }

    /// Expose host-panel visibility to the dock and session, without exposing the mutable history.
    pub(crate) fn is_visible(&self) -> bool {
        self.visible
    }

    /// Visibility changes publish a layout event so the shell persists and reclaims occupied space.
    pub(crate) fn toggle(&mut self, cx: &mut Context<Self>) {
        self.set_visible(!self.visible, cx);
    }

    /// Opening a closed dock may retain the same panel flag; Base owns the region's separate state.
    pub(crate) fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        if let Some(group) = self.group.as_ref().and_then(WeakEntity::upgrade) {
            group.update(cx, |_, cx| cx.notify());
        }
        cx.emit(PanelEvent::LayoutChanged);
        cx.notify();
    }
}

impl EventEmitter<PanelEvent> for MessagePanel {}
impl Focusable for MessagePanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl dock::BasePanel for MessagePanel {
    fn panel_name(&self) -> &'static str {
        "HostMessages"
    }
    fn closable(&self, _: &App) -> bool {
        false
    }
    fn zoomable(&self, _: &App) -> bool {
        false
    }
    fn visible(&self, _: &App) -> bool {
        self.visible
    }
    fn on_added_to(
        &mut self,
        group: WeakEntity<gpui_base::dock::TabGroup>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        self.group = Some(group);
    }
    fn on_removed(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.group = None;
    }
}
impl DockPanel for MessagePanel {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .justify_between()
            .items_center()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(notification_icon())
                    .child(t!("messages.title").to_string()),
            )
            .child(
                div()
                    .id("host-messages-hide")
                    .debug_selector(|| "host-messages-hide".into())
                    .child(
                        Button::new("hide-host-messages")
                            .icon(IconName::WindowMinimize)
                            .small()
                            .compact()
                            .ghost()
                            .tooltip(t!("messages.hide").to_string())
                            .accessibility_label(t!("messages.hide").to_string())
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.stop_propagation();
                                this.toggle(cx);
                            })),
                    ),
            )
    }
}

impl Render for MessagePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = v_flex()
            .id("host-message-list")
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .p_3()
            .gap_3();
        if self.records.is_empty() {
            list = list.child(
                div()
                    .debug_selector(|| "host-messages-empty".into())
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("messages.empty").to_string()),
            );
        }
        for message in self.records.iter().rev().take(self.display_limit) {
            let (label, badge, selector) = match message.level {
                MessageLevel::Info => (t!("messages.info").to_string(), None, "host-message-info"),
                MessageLevel::Warning => (
                    t!("messages.warning").to_string(),
                    Some(ui::controls::StatusIcon::Warning),
                    "host-message-warning",
                ),
                MessageLevel::Error => (
                    t!("messages.error").to_string(),
                    Some(ui::controls::StatusIcon::Error),
                    "host-message-error",
                ),
            };
            let id = message.id;
            list = list.child(
                v_flex()
                    .id(SharedString::from(format!("host-message-{id}")))
                    .w_full()
                    .min_w_0()
                    .debug_selector(move || format!("host-message-{id}"))
                    .gap_1()
                    .pb_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .when_some(badge, |row, badge| row.child(badge.icon(cx)))
                            .child(div().debug_selector(move || selector.into()).child(label)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().foreground)
                            .child(message.text.clone()),
                    ),
            );
        }
        // Expansion is an action on this list, not a replacement page or a second copy of messages.
        if self.records.len() > self.display_limit {
            list = list.child(
                div()
                    .id("host-messages-more")
                    .debug_selector(|| "host-messages-more".into())
                    .child(
                        Button::new("more-host-messages")
                            .label(t!("messages.more").to_string())
                            .accessibility_label(t!("messages.more").to_string())
                            .ghost()
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.display_limit =
                                    (this.display_limit + DISPLAY_BATCH).min(HISTORY_LIMIT);
                                cx.notify();
                            })),
                    ),
            );
        }
        v_flex()
            .id("host-messages-panel")
            .debug_selector(|| "host-messages-panel".into())
            .track_focus(&self.focus)
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(cx.theme().background)
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(list)
                    .child(ui::controls::vertical_scrollbar(&self.scroll, cx)),
            )
    }
}

impl EditorApp {
    /// Local button behavior follows the same Base-backed focus and activation path as other controls.
    pub(crate) fn render_messages_button(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .id("host-messages-toggle")
            .debug_selector(|| "host-messages-toggle".into())
            .child(
                Button::new("toggle-host-messages")
                    .icon(notification_icon())
                    .small()
                    .compact()
                    .ghost()
                    .expanded(self.messages_visible(cx))
                    .tooltip(t!("messages.title").to_string())
                    .accessibility_label(t!("messages.title").to_string())
                    .on_click(cx.listener(|this, _, window, cx| {
                        cx.stop_propagation();
                        this.toggle_messages(window, cx);
                    })),
            )
    }
}
