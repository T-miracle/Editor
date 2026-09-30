//! Rich text colors and spacing are resolved from the active editor theme.

use gpui_base::{TextView, TextViewStyle};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, Pixels, StyleRefinement, Styled, rems};

/// Render selectable Markdown with a highlight that keeps its glyphs readable.
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
        // Base draws selected ranges over the glyphs. Cap the overlay opacity
        // so an opaque theme selection color cannot hide the selected text.
        .with_selection(palette.selection.alpha(palette.selection.a.min(0.25)))
        .with_code_background(palette.background)
        .with_border(palette.border)
        .with_paragraph_gap(rems(0.5))
        // Base 0.7 accepts a heading style; preserve the existing size hierarchy.
        .with_heading(move |level| {
            StyleRefinement::default().text_size(match level {
                1..=3 => font_size,
                4 => font_size * 0.9,
                _ => font_size * 0.8,
            })
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
