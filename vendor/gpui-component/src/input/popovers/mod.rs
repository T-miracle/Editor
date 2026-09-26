mod code_action_menu;
mod completion_menu;
mod diagnostic_popover;
mod hover_popover;

pub(crate) use code_action_menu::*;
pub(crate) use completion_menu::CompletionMenu;
// Expose only the optional style contract; the menu implementation stays private.
pub use completion_menu::CompletionMenuStyle;
pub(crate) use diagnostic_popover::*;
pub(crate) use hover_popover::*;

use gpui::{
    App, Div, ElementId, InteractiveElement as _, Pixels, SharedString, Stateful, StyleRefinement,
    Styled as _, Window, div, px, rems,
};

use crate::{
    ActiveTheme, ThemeStyled as _,
    text::{TextView, TextViewStyle},
};

pub(super) fn render_markdown(
    id: impl Into<ElementId>,
    markdown: impl Into<SharedString>,
    font_size: Option<Pixels>,
    _: &mut Window,
    cx: &mut App,
) -> TextView {
    // Hover content can follow the editor size without resizing other markdown popovers.
    let mut style = TextViewStyle::default()
        .paragraph_gap(rems(0.5))
        .heading_font_size(|level, rem_size| match level {
            1..=3 => rem_size * 1,
            4 => rem_size * 0.9,
            _ => rem_size * 0.8,
        })
        .code_block(
            StyleRefinement::default()
                .bg(cx.theme().transparent)
                .p_0()
                .text_size(font_size.unwrap_or(px(11.))),
        );
    if let Some(font_size) = font_size {
        style.heading_base_font_size = font_size;
    }
    TextView::markdown(id, markdown)
        .style(style)
        .selectable(true)
}

pub(super) fn editor_popover(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .occlude()
        .popover_style(cx)
        .shadow_md()
        .text_xs()
        .p_1()
}
