//! Local file-operation surfaces over base Dialog, Checkbox and Button behavior.

use super::{Button, Checkbox};
use gpui_base::StyledExt as _;
use gpui_kit::{
    AnyElement, App, FocusHandle, InteractiveElement as _, IntoElement, MouseButton, ParentElement,
    SharedString, Styled, Window,
    component::{ActiveTheme as _, h_flex, v_flex},
    div, px,
};
use rust_i18n::t;

/// Intent preview uses the project's local colors, and remains visible for native path offers too.
pub(crate) fn drag_preview(label: String, copy: bool, cx: &App) -> impl IntoElement {
    div()
        .px_2()
        .py_1()
        .rounded(px(4.))
        .border_1()
        .border_color(cx.theme().primary)
        .bg(cx.theme().popover)
        .text_color(cx.theme().popover_foreground)
        .text_sm()
        .child(format!(
            "{} · {label}",
            if copy {
                t!("transfer.copy")
            } else {
                t!("transfer.move")
            }
        ))
}

/// A reviewed choice's identity, label and enabled state; activation is owned by its caller.
pub(crate) struct FileChoice {
    pub id: &'static str,
    pub label: SharedString,
    pub enabled: bool,
    pub activate: Box<dyn Fn(&mut Window, &mut App)>,
}

/// Render a centered modal with a real base focus trap and a pointer-occluding backdrop.
/// `cancel` handles Escape; choices own explicit consent, and disabled choices never activate.
pub(crate) fn conflict_prompt(
    focus: FocusHandle,
    title: String,
    details: Vec<String>,
    choices: Vec<FileChoice>,
    subsequent: Option<(bool, Box<dyn Fn(bool, &mut App)>)>,
    cancel: impl Fn(&mut Window, &mut App) + 'static,
    cx: &mut App,
) -> AnyElement {
    let mut body = v_flex()
        .w(px(480.))
        .max_w_full()
        .gap_3()
        .p_4()
        .rounded(px(8.))
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().popover)
        .text_color(cx.theme().popover_foreground)
        .shadow_lg()
        .child(div().font_semibold().child(title));
    for detail in details {
        body = body.child(div().text_sm().child(detail));
    }
    if let Some((checked, change)) = subsequent {
        body = body.child(
            Checkbox::new("transfer-apply-subsequent")
                .label(t!("transfer.apply_subsequent").to_string())
                .checked(checked)
                .on_change(move |checked, _, cx| change(*checked, cx)),
        );
    }
    body = body.child(
        h_flex()
            .gap_2()
            .flex_wrap()
            .children(choices.into_iter().map(|choice| {
                div()
                    .id(choice.id)
                    .debug_selector(move || choice.id.into())
                    .child(
                        Button::new(choice.id)
                            .label(choice.label)
                            .disabled(!choice.enabled)
                            .on_click(move |_, window, cx| (choice.activate)(window, cx)),
                    )
            })),
    );
    gpui_base::Dialog::new(cx)
        .focus_handle(focus)
        .close_on_backdrop_press(false)
        .on_cancel(move |_, window, cx| {
            cancel(window, cx);
            false
        })
        // Enter never chooses a destructive action implicitly; buttons provide keyboard activation.
        .on_ok(|_, _, _| false)
        .backdrop(
            div()
                .absolute()
                .inset_0()
                .occlude()
                .bg(gpui_kit::rgba(0x00000066))
                .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_up(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation()),
        )
        .popup(gpui_base::DialogPopup::new().child(body))
        .into_any_element()
}

/// A long-running task offers cancellation without taking focus from the editor.
pub(crate) fn progress(
    message: String,
    cancel: impl Fn(&mut Window, &mut App) + 'static,
    cx: &mut App,
) -> AnyElement {
    h_flex()
        .id("transfer-progress")
        .debug_selector(|| "transfer-progress".into())
        .absolute()
        .bottom(px(32.))
        .left(px(16.))
        .w(px(420.))
        .gap_3()
        .p_3()
        .items_center()
        .occlude()
        .rounded(px(8.))
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().popover)
        .text_color(cx.theme().popover_foreground)
        .shadow_lg()
        .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
        .child(div().flex_1().text_sm().child(message))
        .child(
            Button::new("transfer-cancel")
                .label(t!("common.cancel").to_string())
                .on_click(move |_, window, cx| cancel(window, cx)),
        )
        .into_any_element()
}
