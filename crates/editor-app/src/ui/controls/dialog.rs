//! Editor dialog content layout and appearance.

use gpui_base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    AnyElement, App, IntoElement, ParentElement, RenderOnce, StyleRefinement, Styled, Window,
};

/// Content region styled by the editor while callers own its children.
#[derive(IntoElement)]
pub(crate) struct DialogContent {
    style: StyleRefinement,
    children: Vec<AnyElement>,
}

impl DialogContent {
    pub(crate) fn new() -> Self {
        Self {
            style: StyleRefinement::default(),
            children: Vec::new(),
        }
    }
}

impl ParentElement for DialogContent {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for DialogContent {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for DialogContent {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        gpui_kit::div()
            .flex()
            .flex_col()
            .w_full()
            .flex_1()
            .rounded(cx.theme().radius_lg)
            .children(self.children)
            .refine_style(&self.style)
    }
}
