//! Editor dialog content layout and appearance.

use gpui_base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    AnyElement, App, IntoElement, ParentElement, RenderOnce, StyleRefinement, Styled, Window,
};

/// Local modal chrome over gpui-base's focus trap, Escape/Enter and backdrop handling.
///
/// The content stays in the owning HWND so closing an input does not retire a native window
/// while Windows IME or accessibility clients still refer to its fields. Width/height shrink
/// with the viewport; callers supply a scrollable body and veto confirmation on validation errors.
pub(crate) fn modal(
    focus: gpui_kit::FocusHandle,
    title: String,
    content: AnyElement,
    cancel: impl Fn(&mut Window, &mut App) + 'static,
    confirm: impl Fn(&mut Window, &mut App) -> bool + 'static,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    use gpui_kit::{div, px};
    let cancel = std::rc::Rc::new(cancel);
    let dismiss = cancel.clone();
    let viewport = window.viewport_size();
    // B1's proportions fit its compact form; only the body scrolls as the viewport shrinks.
    let width = px(640.).min((viewport.width - px(24.)).max(px(0.)));
    let height = px(520.).min((viewport.height - px(48.)).max(px(0.)));
    gpui_base::Dialog::new(cx)
        .focus_handle(focus)
        .close_on_backdrop_press(false)
        .on_cancel(move |_, window, cx| {
            cancel(window, cx);
            true
        })
        .on_ok(move |_, window, cx| confirm(window, cx))
        .backdrop(div().absolute().inset_0().bg(gpui_kit::rgba(0x00000055)))
        .popup(
            gpui_base::DialogPopup::new()
                .w(width)
                .h(height)
                .flex()
                .flex_col()
                .rounded(cx.theme().radius_lg)
                .bg(cx.theme().tokens.background)
                .text_color(cx.theme().foreground)
                .border_1()
                .border_color(cx.theme().border)
                .shadow_lg()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .h(px(40.))
                        .flex_shrink_0()
                        .px_3()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(div().flex_1().font_semibold().child(title))
                        .child(
                            super::Button::new("run-form-close")
                                .small()
                                .compact()
                                .ghost()
                                .icon(gpui_kit::component::IconName::Close)
                                .accessibility_label(rust_i18n::t!("run.form_cancel"))
                                .tooltip(rust_i18n::t!("run.form_cancel"))
                                .on_click(move |_, window, cx| dismiss(window, cx)),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .child(content),
                ),
        )
        .into_any_element()
}

/// Content region styled by the editor while callers own its children.
#[derive(IntoElement)]
pub(crate) struct DialogContent {
    style: StyleRefinement,
    children: Vec<AnyElement>,
}

impl DialogContent {
    pub(crate) fn new() -> Self {
        Self {
            style: StyleRefinement::default(),
            children: Vec::new(),
        }
    }
}

impl ParentElement for DialogContent {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for DialogContent {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for DialogContent {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        gpui_kit::div()
            .flex()
            .flex_col()
            .w_full()
            .flex_1()
            .rounded(cx.theme().radius_lg)
            .children(self.children)
            .refine_style(&self.style)
    }
}
