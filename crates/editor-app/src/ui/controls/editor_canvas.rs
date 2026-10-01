//! Empty editor canvas appearance, without document input or tab-strip behavior.

use gpui_base::v_flex;
use gpui_kit::{Hsla, InteractiveElement, IntoElement, ParentElement, Styled, div};

/// Fill the editor panel and keep the localized guidance centered as docks resize.
pub(crate) fn empty_editor_canvas(
    message: String,
    background: Hsla,
    foreground: Hsla,
) -> impl IntoElement {
    v_flex()
        .debug_selector(|| "editor-empty-canvas".into())
        .size_full()
        .min_h_0()
        .items_center()
        .justify_center()
        .bg(background)
        .text_color(foreground)
        .child(
            div()
                .debug_selector(|| "editor-empty-message".into())
                .text_sm()
                .child(message),
        )
}
