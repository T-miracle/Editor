//! Scrollbar paint states from the editor theme over gpui-base scrolling.

use gpui_base::{Scrollbar, ScrollbarHandle};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, px};
use plugin_schema::ThemeComponent;

use crate::theme::component_styles;

pub(crate) fn vertical_scrollbar<H: ScrollbarHandle + Clone>(handle: &H, cx: &App) -> Scrollbar {
    let palette = cx.theme();
    let styles = component_styles(cx, ThemeComponent::Scrollbar);
    let normal = styles
        .base
        .background
        .unwrap_or(palette.primary)
        .opacity(0.55);
    let hover = styles
        .hover
        .background
        .unwrap_or(palette.primary)
        .opacity(0.7);
    let active = styles
        .active
        .background
        .unwrap_or(palette.primary)
        .opacity(0.8);
    Scrollbar::vertical(handle).styles(|styles| {
        styles
            .track(|track| track.width(px(8.)))
            .thumb(|thumb| thumb.bg(normal).width(px(6.)).radius(px(3.)))
            .thumb_hover(|thumb| thumb.bg(hover))
            .thumb_active(|thumb| thumb.bg(active))
    })
}
