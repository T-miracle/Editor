//! Local loading motion around the editor's SVG icon.

use std::time::Duration;

use gpui_kit::{
    Animation, AnimationExt as _, App, IntoElement, ParentElement, RenderOnce, Transformation,
    Window, div, percentage,
};

use super::Icon;
use gpui_kit::component::IconName;

#[derive(IntoElement)]
pub(crate) struct Spinner {
    icon: Icon,
    small: bool,
}

impl Spinner {
    pub(crate) fn new() -> Self {
        Self {
            icon: Icon::new(IconName::Loader),
            small: false,
        }
    }

    pub(crate) fn icon(mut self, icon: Icon) -> Self {
        self.icon = icon;
        self
    }

    pub(crate) fn small(mut self) -> Self {
        self.small = true;
        self
    }
}

impl RenderOnce for Spinner {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div().child(
            (if self.small {
                self.icon.small()
            } else {
                self.icon
            })
            .with_animation(
                "editor-spinner",
                Animation::new(Duration::from_millis(800)).repeat(),
                |icon, delta| icon.transform(Transformation::rotate(percentage(delta))),
            ),
        )
    }
}
