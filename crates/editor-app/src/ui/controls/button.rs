//! Product button appearance; gpui-base owns activation, focus, and semantics.

use super::{Icon, Tooltip};
use gpui_base::Button as BaseButton;
use gpui_base::StyledExt as _;
use gpui_kit::component::{ActiveTheme as _, IconName};
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, FocusHandle, Hsla, InteractiveElement, IntoElement,
    ParentElement, RenderOnce, SharedString, StatefulInteractiveElement, StyleRefinement, Styled,
    Window, div, prelude::FluentBuilder as _, px,
};

#[derive(Clone, Copy)]
enum Variant {
    Default,
    Primary,
    Danger,
    Ghost,
    Custom(ButtonCustomVariant),
}

/// Colors supplied by a declarative plugin widget, resolved at its call site.
#[derive(Clone, Copy)]
pub(crate) struct ButtonCustomVariant {
    background: Hsla,
    foreground: Hsla,
    hover: Hsla,
    active: Hsla,
}

impl ButtonCustomVariant {
    pub(crate) fn new(cx: &App) -> Self {
        Self {
            background: cx.theme().button,
            foreground: cx.theme().button_foreground,
            hover: cx.theme().button_hover,
            active: cx.theme().button_active,
        }
    }

    pub(crate) fn color(mut self, color: Hsla) -> Self {
        self.background = color;
        self
    }

    pub(crate) fn foreground(mut self, color: Hsla) -> Self {
        self.foreground = color;
        self
    }

    pub(crate) fn hover(mut self, color: Hsla) -> Self {
        self.hover = color;
        self
    }

    pub(crate) fn active(mut self, color: Hsla) -> Self {
        self.active = color;
        self
    }
}

/// Keeps the editor's visual variants behind one small interface.
#[derive(IntoElement)]
pub(crate) struct Button {
    base: BaseButton,
    style: StyleRefinement,
    variant: Variant,
    label: Option<SharedString>,
    icon: Option<Icon>,
    children: Vec<AnyElement>,
    tooltip: Option<SharedString>,
    small: bool,
    compact: bool,
    outline: bool,
    loading: bool,
    disabled: bool,
    content_full_width: bool,
}

impl Button {
    pub(crate) fn new(id: impl Into<ElementId>) -> Self {
        Self {
            base: BaseButton::new(id),
            style: StyleRefinement::default(),
            variant: Variant::Default,
            label: None,
            icon: None,
            children: Vec::new(),
            tooltip: None,
            small: false,
            compact: false,
            outline: false,
            loading: false,
            disabled: false,
            content_full_width: false,
        }
    }

    pub(crate) fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub(crate) fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.base = self.base.accessibility_label(label);
        self
    }

    /// Use an owner-managed handle for focus observation; Base still owns tab order and activation.
    pub(crate) fn track_focus(mut self, handle: &FocusHandle) -> Self {
        self.base = self.base.track_focus(handle);
        self
    }

    /// Constrain composed card content to the button's width; normal labels keep their intrinsic centered layout.
    pub(crate) fn content_full_width(mut self) -> Self {
        self.content_full_width = true;
        self
    }

    pub(crate) fn icon(mut self, icon: impl Into<Icon>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub(crate) fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub(crate) fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.base = self.base.on_click(handler);
        self
    }

    pub(crate) fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub(crate) fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub(crate) fn primary(mut self) -> Self {
        self.variant = Variant::Primary;
        self
    }

    pub(crate) fn danger(mut self) -> Self {
        self.variant = Variant::Danger;
        self
    }

    pub(crate) fn ghost(mut self) -> Self {
        self.variant = Variant::Ghost;
        self
    }

    pub(crate) fn custom(mut self, custom: ButtonCustomVariant) -> Self {
        self.variant = Variant::Custom(custom);
        self
    }

    /// Keep an accent border and label while retaining a soft variant-colored background.
    pub(crate) fn outline(mut self) -> Self {
        self.outline = true;
        self
    }

    pub(crate) fn small(mut self) -> Self {
        self.small = true;
        self
    }

    pub(crate) fn compact(mut self) -> Self {
        self.compact = true;
        self
    }
}

impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Button {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Button {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = cx.theme();
        let (background, foreground, hover, active) = match self.variant {
            Variant::Default => (
                palette.button,
                palette.button_foreground,
                palette.button_hover,
                palette.button_active,
            ),
            Variant::Primary => (
                palette.button_primary,
                palette.button_primary_foreground,
                palette.button_primary_hover,
                palette.button_primary_active,
            ),
            Variant::Danger => (
                palette.danger,
                palette.danger_foreground,
                palette.danger.opacity(0.85),
                palette.danger.opacity(0.7),
            ),
            Variant::Ghost => (
                palette.transparent,
                palette.foreground,
                palette.list_hover,
                palette.list_active,
            ),
            Variant::Custom(style) => (
                style.background,
                style.foreground,
                style.hover,
                style.active,
            ),
        };
        // Filled variants use their background as the accent; custom variants supply a readable label color.
        let outline_color = match self.variant {
            Variant::Primary | Variant::Danger => background,
            _ => foreground,
        };
        let (background, foreground, hover, active) = if self.outline {
            // Solid primary/danger variants receive a light tint; custom palettes keep their chosen fills.
            let (background, hover, active) = match self.variant {
                Variant::Primary | Variant::Danger => (
                    outline_color.opacity(0.08),
                    outline_color.opacity(0.13),
                    outline_color.opacity(0.20),
                ),
                _ => (background, hover, active),
            };
            (background, outline_color, hover, active)
        } else {
            (background, foreground, hover, active)
        };
        let height = if self.small { 24. } else { 28. };
        let icon = if self.loading {
            Some(Icon::new(IconName::Loader))
        } else {
            self.icon
        };
        let content = div()
            .flex()
            .items_center()
            .gap_1()
            .when(self.content_full_width, |content| {
                content.w_full().min_w_0()
            })
            .when_some(icon, |this, icon| this.child(icon.small()))
            .when_some(self.label.clone(), |this, label| this.child(label))
            .children(self.children);
        self.base
            .disabled(self.disabled || self.loading)
            .h(px(height))
            .min_w(px(if self.compact { height } else { 48. }))
            .px(px(if self.compact { 4. } else { 10. }))
            .rounded(palette.radius)
            .when(self.outline, |button| {
                button.border_1().border_color(outline_color.opacity(0.65))
            })
            .bg(background)
            .text_color(foreground)
            .text_size(px(if self.small { 12. } else { 13. }))
            .cursor_pointer()
            .hover(|style| style.bg(hover))
            .active(|style| style.bg(active))
            .styles(|styles| styles.disabled(|style| style.opacity(0.5)))
            .child(content)
            .refine_style(&self.style)
            .when_some(self.tooltip, |this, tooltip| {
                this.tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            })
    }
}
