//! Controlled disclosure appearance; gpui-base owns visibility, keyboard activation and semantics.
use super::Button;
use gpui_base::Collapsible;
use gpui_kit::component::{ActiveTheme as _, IconName};
use gpui_kit::{
    AnyElement, App, FocusHandle, IntoElement, ParentElement, SharedString, Styled, Window, div, px,
};

/// A full-width summary with an optional panel, styled once for all editor configuration sections.
/// The owner retains `open` and input state; collapsing never destroys that retained editing state.
pub(crate) fn disclosure(
    id: &'static str,
    label: impl Into<SharedString>,
    summary: impl Into<SharedString>,
    open: bool,
    focus: &FocusHandle,
    content: AnyElement,
    change: impl Fn(bool, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let label = label.into();
    Collapsible::new()
        .open(open)
        .flex()
        .flex_col()
        .child(
            Button::new(id)
                .ghost()
                .content_full_width()
                .track_focus(focus)
                .expanded(open)
                .debug_selector(move || id.into())
                .accessibility_label(label.clone())
                .on_click(move |_, window, cx| change(!open, window, cx))
                .w_full()
                .h(px(42.))
                .border_t_1()
                .border_color(cx.theme().border)
                .text_color(cx.theme().foreground)
                .icon(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .child(label)
                .child(div().flex_1())
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(summary.into()),
                ),
        )
        .content(content)
        .into_any_element()
}
