//! Multi-line form appearance; gpui-base retains selection, IME, undo, scrolling and keyboard input.
use gpui_base::input::{Textarea as BaseTextarea, TextareaState};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, Entity, IntoElement, ParentElement, RenderOnce, Styled, Window, div, px};

/// A compact ordinary-text field, distinct from both a single-line input and a document editor.
#[derive(IntoElement)]
pub(crate) struct Textarea {
    state: Entity<TextareaState>,
}
impl Textarea {
    /// The caller owns editing state for the form lifetime; painting never resets its value.
    pub(crate) fn new(state: &Entity<TextareaState>) -> Self {
        Self {
            state: state.clone(),
        }
    }
}
impl RenderOnce for Textarea {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = cx.theme();
        div()
            .w_full()
            .min_h(px(52.))
            .px_2()
            .py_1()
            .rounded(palette.radius)
            .border_1()
            .border_color(palette.input)
            .bg(palette.background)
            .text_color(palette.foreground)
            // Multiline fields scale with the same global typography as their labels and inputs.
            .text_sm()
            .child(BaseTextarea::new(&self.state))
    }
}
