//! Rich text colors and spacing are resolved from the active editor theme.

use gpui_base::{TextView, TextViewStyle};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    App, HighlightStyle, Hsla, ImageSource, Pixels, SharedString, StyleRefinement, Styled, rems,
};
use std::sync::Arc;

/// Render selectable Markdown with a highlight that keeps its glyphs readable.
pub(crate) fn markdown_view(
    id: &'static str,
    markdown: String,
    font_size: Pixels,
    cx: &App,
) -> TextView {
    TextView::markdown(id, markdown)
        .style(rich_text_style(font_size, cx))
        .selectable(true)
}

/// Render guest-produced rich markup natively with project colors and no implicit external access.
/// Navigation and images must use separately negotiated host capabilities instead of Base defaults.
pub(crate) fn rich_text_view(
    id: SharedString,
    html: String,
    font_size: Pixels,
    colors: RichTextColors,
    cx: &App,
) -> TextView {
    TextView::html(id, html)
        .style(document_style(font_size, colors, cx))
        .selectable(true)
        .on_link_click(|_, _, _, _| {})
        .image_source(|_| ImageSource::Custom(Arc::new(|_, _| None)))
}

/// Keep all native rich-text color roles, including inline code, on the same resolved palette.
fn document_style(font_size: Pixels, colors: RichTextColors, cx: &App) -> TextViewStyle {
    rich_text_style(font_size, cx)
        .with_foreground(colors.foreground)
        .with_code_background(colors.background)
        .with_inline_code(HighlightStyle {
            background_color: Some(colors.background),
            ..Default::default()
        })
        .with_border(colors.border)
        .with_link(colors.link)
        .with_heading(move |level| {
            // Full-size document previews need a readable hierarchy independent of compact hover cards.
            StyleRefinement::default().text_size(
                font_size
                    * match level {
                        1 => 1.8,
                        2 => 1.5,
                        3 => 1.25,
                        4 => 1.1,
                        _ => 1.,
                    },
            )
        })
}

/// The caller resolves plugin role overrides; the local renderer preserves them on native glyphs.
pub(crate) struct RichTextColors {
    pub foreground: Hsla,
    pub background: Hsla,
    pub border: Hsla,
    pub link: Hsla,
}

/// Existing hover Markdown and plugin rich markup share typography without sharing their parsers.
fn rich_text_style(font_size: Pixels, cx: &App) -> TextViewStyle {
    let palette = cx.theme();
    TextViewStyle::default()
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
        )
}

/// Verify the style actually passed to Base inherits live and custom colors instead of Base defaults.
#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{TestAppContext, gpui, px, rgb};

    #[gpui::test]
    fn document_rich_text_preserves_role_colors_in_both_themes(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::ui::typography::init(cx);
            for light in [false, true] {
                crate::ui::theme::apply_theme(crate::ui::theme::builtin_theme(light), cx);
                let palette = cx.theme();
                let style = document_style(
                    px(14.),
                    RichTextColors {
                        foreground: palette.foreground,
                        background: palette.background,
                        border: palette.border,
                        link: palette.primary,
                    },
                    cx,
                );
                assert_eq!(style.foreground(), palette.foreground);
                assert_eq!(
                    style.inline_code().background_color,
                    Some(palette.background)
                );
                let color = rgb(0x123456).into();
                let custom = document_style(
                    px(14.),
                    RichTextColors {
                        foreground: color,
                        background: color,
                        border: color,
                        link: color,
                    },
                    cx,
                );
                assert_eq!(custom.foreground(), color);
                assert_eq!(custom.link(), color);
                assert_eq!(custom.border(), color);
                assert_eq!(custom.inline_code().background_color, Some(color));
            }
        });
    }
}
