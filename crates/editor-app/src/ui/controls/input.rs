//! Single-line input appearance around gpui-base's shared editing state.

use gpui_base::StyledExt as _;
use gpui_base::input::{Input as BaseInput, InputState};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    App, Entity, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StyleRefinement, Styled, Window, div, prelude::FluentBuilder as _, px,
};

#[derive(IntoElement)]
pub(crate) struct Input {
    state: Entity<InputState>,
    style: StyleRefinement,
    appearance: bool,
    bordered: bool,
    focus_bordered: bool,
    /// The native input publishes its semantic name independently of the placeholder text.
    accessibility_label: Option<SharedString>,
}

impl Input {
    pub(crate) fn new(state: &Entity<InputState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
            appearance: true,
            bordered: true,
            focus_bordered: true,
            accessibility_label: None,
        }
    }

    pub(crate) fn appearance(mut self, appearance: bool) -> Self {
        self.appearance = appearance;
        self
    }
    /// Expose an explicit native accessibility label without changing shared editing behavior.
    pub(crate) fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }

    pub(crate) fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }

    pub(crate) fn focus_bordered(mut self, focus_bordered: bool) -> Self {
        self.focus_bordered = focus_bordered;
        self
    }
}

impl Styled for Input {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Input {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = cx.theme();
        div()
            .w_full()
            // Base's single-line engine fills its frame. A minimum height alone leaves its
            // percentage-height hit region at zero, so painted fields cannot receive pointer/IME input.
            .h(px(28.))
            .min_h(px(28.))
            .flex()
            .items_center()
            .text_color(palette.foreground)
            // Rem-based text follows the single global typography size, including form zoom.
            .text_sm()
            .when(self.appearance, |this| {
                this.px_2()
                    .rounded(palette.radius)
                    .bg(palette.background)
                    .when(self.bordered, |this| {
                        this.border_1().border_color(palette.input)
                    })
            })
            .when(self.focus_bordered && self.appearance, |this| {
                this.in_focus(|style| style.border_color(palette.ring))
            })
            .child(
                gpui_base::input::InputBase::new(("local-input", self.state.entity_id()))
                    .size_full()
                    .when_some(self.accessibility_label, |frame, label| {
                        frame.accessibility_label(label)
                    })
                    .child(BaseInput::new(&self.state)),
            )
            .refine_style(&self.style)
    }
}
