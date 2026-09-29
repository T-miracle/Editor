//! Local tab strip appearance over gpui-base tab selection semantics.

use std::rc::Rc;

use gpui_base::{Tab, Tabs};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    App, ElementId, InteractiveElement, IntoElement, ParentElement, Styled, Window, px,
};

pub(crate) fn tab_strip(
    id: impl Into<ElementId>,
    selected: usize,
    labels: [&'static str; 2],
    on_change: impl Fn(usize, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let palette = cx.theme();
    let on_change = Rc::new(on_change);
    let mut tabs = Tabs::new(id)
        .flex()
        .w_full()
        .border_b_1()
        .border_color(palette.border);
    for (index, label) in labels.into_iter().enumerate() {
        let handler = on_change.clone();
        tabs = tabs.child(
            Tab::new(format!("manager-tab-{index}"))
                .selected(index == selected)
                .accessibility_label(label)
                .set_position(index + 1, labels.len())
                .on_click(move |_, window, cx| handler(index, window, cx))
                .h(px(30.))
                .flex_1()
                .px_3()
                .border_b_2()
                .border_color(if index == selected {
                    palette.primary
                } else {
                    palette.transparent
                })
                .bg(palette.sidebar)
                .text_color(if index == selected {
                    palette.foreground
                } else {
                    palette.muted_foreground
                })
                .hover(|style| style.bg(palette.list_hover))
                .child(label),
        );
    }
    tabs
}
