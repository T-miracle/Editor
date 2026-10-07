//! Editor dialog content layout and appearance.

use gpui_base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    AnyElement, App, IntoElement, ParentElement, RenderOnce, StyleRefinement, Styled, Window,
};

/// Native dialog body over gpui-base's focus trap and Escape/Enter handling.
///
/// AppDialog owns the title bar and native HWND; this surface fills only its body, leaving window
/// dragging and close controls reachable. Callers own staged edits and may veto dismissal/save.
pub(crate) fn native_modal(
    focus: gpui_kit::FocusHandle,
    content: AnyElement,
    cancel: impl Fn(&mut Window, &mut App) -> bool + 'static,
    confirm: impl Fn(&mut Window, &mut App) -> bool + 'static,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    use gpui_kit::{div, px};
    let viewport = window.viewport_size();
    let title_height = px(crate::PANEL_HEADER_HEIGHT);
    gpui_base::Dialog::new(cx)
        .focus_handle(focus)
        .close_on_backdrop_press(false)
        .on_cancel(move |_, window, cx| cancel(window, cx))
        .on_ok(move |_, window, cx| confirm(window, cx))
        // Base Dialog's deferred host starts at the viewport origin; reserve AppDialog's title bar.
        .top(title_height)
        .h((viewport.height - title_height).max(px(0.)))
        .popup(
            gpui_base::DialogPopup::new()
                .size_full()
                .flex()
                .flex_col()
                .bg(cx.theme().tokens.background)
                .text_color(cx.theme().foreground)
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
            // Expanded content must shrink into the popup so its own scroller, not the footer, grows.
            .min_h_0()
            .rounded(cx.theme().radius_lg)
            .children(self.children)
            .refine_style(&self.style)
    }
}
