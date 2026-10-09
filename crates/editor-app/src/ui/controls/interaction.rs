//! Native plugin prompts share Base input, button and dialog behavior with editor-owned styling.
use super::{Button, Input};
use gpui_base::StyledExt as _;
use gpui_base::input::{InputEvent, InputState, MoveDown, MoveEnd, MoveHome, MoveUp};
use gpui_kit::{
    App, AppContext as _, Context, Entity, EntityInputHandler as _, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement, Render, ScrollHandle,
    SharedString, StatefulInteractiveElement as _, Styled, Subscription, Window,
    component::ActiveTheme as _, div, prelude::FluentBuilder as _, px,
};
use plugin_runtime::{
    EditorRequest,
    plugin_protocol::{
        api::{CancelMode, EditorValue},
        interaction::{Operation, Value},
    },
};
use rust_i18n::t;

#[cfg(test)]
mod tests;

/// One instance-owned prompt; dismissal and dropping the native owner seal its pending result.
pub(crate) struct HostInteraction {
    source: String,
    request: EditorRequest,
    operation: Operation,
    input: Entity<InputState>,
    focus: FocusHandle,
    previous: Option<FocusHandle>,
    selected: usize,
    /// Only choices scroll; text entry and confirmation remain reachable throughout navigation.
    choices_scroll: ScrollHandle,
    closed: bool,
    _subscription: Subscription,
}

impl HostInteraction {
    /// All request metadata has already passed runtime admission; focus belongs to this native prompt.
    pub(crate) fn new(
        source: String,
        request: EditorRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let plugin_runtime::plugin_protocol::api::EditorOperation::Interaction { operation } =
            request.operation()
        else {
            unreachable!()
        };
        let operation = operation.clone();
        let value = match &operation {
            Operation::Input { value, .. } => value.clone(),
            _ => String::new(),
        };
        let (masked, placeholder) = match &operation {
            Operation::Input {
                password,
                placeholder,
                ..
            } => (*password, placeholder.clone().unwrap_or_default()),
            _ => (false, t!("interaction.search").to_string()),
        };
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(value)
                .masked(masked)
                .placeholder(placeholder)
        });
        let previous = window.focused(cx);
        let focus = cx.focus_handle();
        let modal = matches!(
            operation,
            Operation::Input { .. } | Operation::QuickPick { .. } | Operation::Confirm { .. }
        );
        if matches!(
            operation,
            Operation::Input { .. } | Operation::QuickPick { .. }
        ) {
            input.read(cx).focus_handle(cx).focus(window, cx);
        } else if modal {
            focus.focus(window, cx);
        }
        let subscription =
            cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } => {
                    this.confirm(window, cx);
                }
                InputEvent::Change => {
                    this.selected = 0;
                    this.choices_scroll.scroll_to_item(0);
                    cx.notify();
                }
                _ => {}
            });
        // Expiry and retirement close an already visible prompt even if no further UI input occurs.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(50))
                    .await;
                let ended = this.update(cx, |this, cx| {
                    cx.notify();
                    this.closed || this.request.status().is_terminal()
                });
                if !matches!(ended, Ok(false)) {
                    break;
                }
            }
        })
        .detach();
        Self {
            source,
            request,
            operation,
            input,
            focus,
            previous: modal.then_some(previous).flatten(),
            selected: 0,
            choices_scroll: ScrollHandle::new(),
            closed: false,
            _subscription: subscription,
        }
    }

    /// Modal input is serialized globally; notices and progress never steal editor focus.
    pub(crate) fn is_modal(&self) -> bool {
        !self.closed
            && matches!(
                self.operation,
                Operation::Input { .. } | Operation::QuickPick { .. } | Operation::Confirm { .. }
            )
    }

    /// Release terminal controls on the owning window before the shell drops their native state.
    pub(crate) fn prune(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.closed && self.request.status().is_terminal() {
            self.close(window, cx);
        }
        self.closed
    }

    /// Filter only presentation fields; the result always retains the declared stable ID.
    fn choices(&self, cx: &App) -> Vec<(String, String)> {
        let Operation::QuickPick { items, .. } = &self.operation else {
            return Vec::new();
        };
        let query = self.input.read(cx).value().to_lowercase();
        items
            .iter()
            .filter(|item| {
                item.label.to_lowercase().contains(&query)
                    || item
                        .description
                        .as_ref()
                        .is_some_and(|text| text.to_lowercase().contains(&query))
            })
            .map(|item| (item.id.clone(), item.label.clone()))
            .collect()
    }

    /// User confirmation is admitted through the same immutable completion gate as runtime cancellation.
    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        // Enter commits IME composition first; marked text is not a confirmed plugin value.
        if matches!(
            self.operation,
            Operation::Input { .. } | Operation::QuickPick { .. }
        ) && self.input.update(cx, |state, cx| {
            state.marked_text_range(window, cx).is_some()
        }) {
            return false;
        }
        let value = match &self.operation {
            Operation::Input { max_bytes, .. } => {
                let value = self.input.read(cx).value().to_string();
                if value.len() > *max_bytes {
                    return false;
                }
                Value::Input(value)
            }
            Operation::QuickPick { .. } => {
                let choices = self.choices(cx);
                let Some((id, _)) = choices.get(self.selected) else {
                    return false;
                };
                Value::Picked(id.clone())
            }
            Operation::Confirm { .. } => Value::Confirmed,
            Operation::Notify { .. } => Value::Dismissed,
            _ => return false,
        };
        self.request.finish(Ok(EditorValue::Interaction(value)));
        self.close(window, cx);
        true
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.closed = true;
        // Expiry must not pull focus back from an unrelated dialog or another native control.
        let owns_focus = self.focus.contains_focused(window, cx)
            || self
                .input
                .read(cx)
                .focus_handle(cx)
                .contains_focused(window, cx);
        if let Some(previous) = self.previous.take().filter(|_| owns_focus) {
            previous.focus(window, cx);
        }
        cx.notify();
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request.cancel_from_host(CancelMode::TryTerminate);
        self.close(window, cx);
    }

    /// Arrow keys navigate the filtered choices; Base Input owns text entry and IME composition.
    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.modifiers != Default::default() {
            return;
        }
        if self.navigate(&event.keystroke.key, window, cx) {
            cx.stop_propagation();
        }
    }

    /// Base dispatches bound input actions before raw keys; both paths use this one IME-aware step.
    fn navigate(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !matches!(self.operation, Operation::QuickPick { .. }) {
            return false;
        }
        // IME candidate navigation belongs to Base Input until composition is committed.
        if self.input.update(cx, |input, cx| {
            input.marked_text_range(window, cx).is_some()
        }) {
            return false;
        }
        let count = self.choices(cx).len();
        if count == 0 {
            return false;
        }
        match key {
            "up" => self.selected = (self.selected + count - 1) % count,
            "down" => self.selected = (self.selected + 1) % count,
            "home" => self.selected = 0,
            "end" => self.selected = count - 1,
            _ => return false,
        }
        self.choices_scroll.scroll_to_item(self.selected);
        cx.notify();
        true
    }
}

impl Drop for HostInteraction {
    fn drop(&mut self) {
        self.request.cancel_from_host(CancelMode::TryTerminate);
    }
}

impl HostInteraction {
    /// Shared content owns source attribution, bounded native input and stable quick-pick IDs.
    fn content(&self, title: String, cx: &mut Context<Self>) -> gpui_kit::Stateful<gpui_kit::Div> {
        let mut content = div()
            .id("plugin-interaction")
            .debug_selector(|| "plugin-interaction".into())
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(cx.theme().popover)
            .text_color(cx.theme().foreground)
            // Navigation precedes the single-line input's Home/End bindings; IME remains guarded.
            .capture_key_down(cx.listener(Self::key_down))
            // Input's bound movement actions run before low-level key listeners in GPUI.
            .capture_action(cx.listener(|this, _: &MoveUp, window, cx| {
                if this.navigate("up", window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, window, cx| {
                if this.navigate("down", window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveHome, window, cx| {
                if this.navigate("home", window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveEnd, window, cx| {
                if this.navigate("end", window, cx) {
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .debug_selector({
                        let source = self.source.clone();
                        move || format!("plugin-interaction-source-{source}")
                    })
                    .text_sm()
                    .child(self.source.clone()),
            )
            .child(div().font_semibold().child(title.clone()))
            .when(
                matches!(
                    self.operation,
                    Operation::Input { .. } | Operation::QuickPick { .. }
                ),
                |body| {
                    body.child(
                        div()
                            .id("plugin-interaction-input")
                            .debug_selector(|| "plugin-interaction-input".into())
                            .child(Input::new(&self.input).accessibility_label(title)),
                    )
                },
            );
        match &self.operation {
            Operation::Confirm { message, .. } | Operation::Notify { message, .. } => {
                content = content.child(div().child(message.clone()))
            }
            Operation::Progress { message, .. } => {
                let (latest, percent) = self.request.progress();
                content = content.child(div().child(if latest.is_empty() && percent.is_none() {
                    message.clone()
                } else {
                    latest
                }));
                if let Some(percent) = percent {
                    content = content.child(div().text_sm().child(format!("{percent}%")));
                }
            }
            _ => {}
        }
        let choices = self.choices(cx);
        let mut list = div()
            .id("plugin-interaction-choices")
            .debug_selector(|| "plugin-interaction-choices".into())
            .h(px(280.))
            .overflow_y_scroll()
            .track_scroll(&self.choices_scroll);
        for (index, (id, label)) in choices.into_iter().enumerate() {
            list = list.child(
                Button::new(SharedString::from(format!("plugin-pick-{id}")))
                    .accessibility_label(label.clone())
                    .label(label)
                    .h(px(32.))
                    .w_full()
                    .flex_shrink_0()
                    .ghost()
                    .when(index == self.selected, |button| button.primary())
                    .debug_selector(move || format!("plugin-pick-{id}"))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.selected = index;
                        this.confirm(window, cx);
                    })),
            );
        }
        if matches!(self.operation, Operation::QuickPick { .. }) {
            content = content.child(list);
        }
        content
    }

    /// A notice/progress card uses the existing local host message appearance without modal focus.
    fn message(
        &self,
        mut content: gpui_kit::Stateful<gpui_kit::Div>,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        // Nonmodal notices form an attributed stack; dismissing one cannot close another owner's task.
        content = match &self.operation {
            Operation::Notify { severity, .. } => content
                .child(
                    div().text_sm().child(
                        t!(match severity {
                            plugin_runtime::plugin_protocol::interaction::Severity::Information =>
                                "interaction.information",
                            plugin_runtime::plugin_protocol::interaction::Severity::Warning =>
                                "interaction.warning",
                            plugin_runtime::plugin_protocol::interaction::Severity::Error =>
                                "interaction.error",
                        })
                        .to_string(),
                    ),
                )
                .child(
                    Button::new("plugin-notice-dismiss")
                        .label(t!("interaction.dismiss").to_string())
                        .accessibility_label(t!("interaction.dismiss").to_string())
                        .debug_selector(|| "plugin-notice-dismiss".into())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.confirm(window, cx);
                        })),
                ),
            Operation::Progress { cancellable, .. } => content.when(*cancellable, |body| {
                body.child(
                    Button::new("plugin-progress-cancel")
                        .label(t!("interaction.cancel").to_string())
                        .accessibility_label(t!("interaction.cancel").to_string())
                        .debug_selector(|| "plugin-progress-cancel".into())
                        .on_click(cx.listener(|this, _, window, cx| this.cancel(window, cx))),
                )
            }),
            _ => content,
        };
        super::notification::message_card("plugin-message", cx)
            .w(px(380.))
            .flex()
            .flex_col()
            .child(content.p_0())
            .into_any_element()
    }

    /// Modal chrome keeps Base dialog actions and the original focus lease around shared content.
    fn modal(
        &self,
        content: gpui_kit::Stateful<gpui_kit::Div>,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let footer = div()
            .flex()
            .gap_2()
            .child(
                Button::new("plugin-interaction-cancel")
                    .label(t!("interaction.cancel").to_string())
                    .accessibility_label(t!("interaction.cancel").to_string())
                    .debug_selector(|| "plugin-interaction-cancel".into())
                    .on_click(cx.listener(|this, _, window, cx| this.cancel(window, cx))),
            )
            .child(
                Button::new("plugin-interaction-confirm")
                    .label(t!("interaction.confirm").to_string())
                    .primary()
                    .accessibility_label(t!("interaction.confirm").to_string())
                    .debug_selector(|| "plugin-interaction-confirm".into())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.confirm(window, cx);
                    })),
            );
        let cancel = cx.entity().downgrade();
        let confirm = cancel.clone();
        gpui_base::Dialog::new(cx)
            .focus_handle(self.focus.clone())
            .close_on_backdrop_press(false)
            .on_cancel(move |_, window, cx| {
                let _ = cancel.update(cx, |this, cx| this.cancel(window, cx));
                true
            })
            .on_ok(move |_, window, cx| {
                confirm
                    .update(cx, |this, cx| this.confirm(window, cx))
                    .unwrap_or(true)
            })
            .top(px(80.))
            .popup(
                gpui_base::DialogPopup::new()
                    .w(px(500.))
                    .max_h(px(600.))
                    .rounded(cx.theme().radius_lg)
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(content.max_h(px(580.)).overflow_y_scroll().child(footer)),
            )
            .into_any_element()
    }
}

impl Render for HostInteraction {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.closed || self.request.status().is_terminal() {
            if !self.closed {
                self.close(window, cx);
            }
            return div().into_any_element();
        }
        let title = match &self.operation {
            Operation::Input { title, .. }
            | Operation::QuickPick { title, .. }
            | Operation::Confirm { title, .. }
            | Operation::Notify { title, .. }
            | Operation::Progress { title, .. } => title.clone(),
            _ => return div().into_any_element(),
        };
        let content = self.content(title, cx);
        if self.is_modal() {
            self.modal(content, cx)
        } else {
            self.message(content, cx)
        }
    }
}
