//! The editor status bar's alignment and density live in the local UI layer.

use gpui_base::h_flex;
use gpui_kit::{AnyElement, App, IntoElement, ParentElement, RenderOnce, Styled, Window, px};

#[derive(IntoElement, Default)]
pub(crate) struct StatusBar {
    left: Vec<AnyElement>,
    right: Vec<AnyElement>,
}

impl StatusBar {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn left(mut self, child: impl IntoElement) -> Self {
        self.left.push(child.into_any_element());
        self
    }

    pub(crate) fn right(mut self, child: impl IntoElement) -> Self {
        self.right.push(child.into_any_element());
        self
    }
}

impl RenderOnce for StatusBar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        h_flex()
            // Keep status contents vertically centered within the requested 32px bar.
            .h(px(32.))
            .w_full()
            .items_center()
            .gap_3()
            .px_2()
            .children(self.left)
            .child(h_flex().flex_1())
            .children(self.right)
    }
}
