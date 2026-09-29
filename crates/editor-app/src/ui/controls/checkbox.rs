//! Checkbox appearance stays local while gpui-base handles toggle and focus.

use gpui_base::{Checkbox as BaseCheckbox, CheckboxState};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    App, ClickEvent, ElementId, IntoElement, ParentElement, RenderOnce, SharedString, Styled,
    Window, div, prelude::FluentBuilder as _, px,
};

#[derive(IntoElement)]
pub(crate) struct Checkbox {
    base: BaseCheckbox,
    label: SharedString,
    checked: bool,
    disabled: bool,
}

impl Checkbox {
    pub(crate) fn new(id: impl Into<ElementId>) -> Self {
        Self {
            base: BaseCheckbox::new(id),
            label: SharedString::default(),
            checked: false,
            disabled: false,
        }
    }

    pub(crate) fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    pub(crate) fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub(crate) fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub(crate) fn on_change(
        mut self,
        handler: impl Fn(&bool, &ClickEvent, &mut App) + 'static,
    ) -> Self {
        self.base = self.base.on_change(move |state, event, _, cx| {
            let checked = state == CheckboxState::Checked;
            handler(&checked, event, cx);
        });
        self
    }
}

impl RenderOnce for Checkbox {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = cx.theme();
        let mark = div()
            .size(px(15.))
            .rounded(px(3.))
            .border_1()
            .border_color(if self.checked {
                palette.primary
            } else {
                palette.border
            })
            .bg(if self.checked {
                palette.primary
            } else {
                palette.background
            })
            .text_color(palette.primary_foreground)
            .text_size(px(12.))
            .flex()
            .items_center()
            .justify_center()
            .when(self.checked, |this| this.child("✓"));
        self.base
            .checked(self.checked)
            .disabled(self.disabled)
            .accessibility_label(self.label.clone())
            .flex()
            .items_center()
            .gap_2()
            .text_color(palette.foreground)
            .text_size(px(13.))
            .when(self.disabled, |this| this.opacity(0.5))
            .child(mark)
            .child(self.label)
    }
}
