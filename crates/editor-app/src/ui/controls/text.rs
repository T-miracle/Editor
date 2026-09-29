//! Rich text colors and spacing are resolved from the active editor theme.

use gpui_base::{TextView, TextViewStyle};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, Pixels, StyleRefinement, Styled, rems};

pub(crate) fn markdown_view(
    id: &'static str,
    markdown: String,
    font_size: Pixels,
    cx: &App,
) -> TextView {
    let palette = cx.theme();
    let style = TextViewStyle::default()
        .with_foreground(palette.foreground)
        .with_muted_foreground(palette.muted_foreground)
        .with_link(palette.primary)
        .with_selection(palette.selection)
        .with_code_background(palette.background)
        .with_border(palette.border)
        .with_paragraph_gap(rems(0.5))
        .with_heading_base_font_size(font_size)
        .with_heading_font_size(|level, size| match level {
            1..=3 => size,
            4 => size * 0.9,
            _ => size * 0.8,
        })
        .with_code_block(
            StyleRefinement::default()
                .bg(palette.transparent)
                .p_0()
                .text_size(font_size),
        );
    TextView::markdown(id, markdown)
        .style(style)
        .selectable(true)
}
