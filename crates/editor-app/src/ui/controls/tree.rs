//! Local tree-row appearance; gpui-base owns selection, keyboard navigation and virtualization.
use super::Icon;
use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::component::{ActiveTheme as _, IconName};
use gpui_kit::{
    App, Div, InteractiveElement, IntoElement, ParentElement, Stateful, Styled, div, px,
};

/// Common row geometry keeps disclosure, icon and label aligned in any native tree.
pub(crate) fn tree_row(
    id: String,
    label: String,
    depth: usize,
    icon: Icon,
    folder: bool,
    expanded: bool,
    selected: bool,
    invalid: bool,
    cx: &App,
    toggle: Option<std::rc::Rc<dyn Fn(&mut gpui_kit::Window, &mut App)>>,
) -> Stateful<Div> {
    let disclosure = if folder {
        Icon::new(if expanded {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        })
        .size(px(12.))
        .into_any_element()
    } else {
        div().size(px(12.)).into_any_element()
    };
    div()
        .id(id)
        .h(px(34.))
        .w_full()
        .flex()
        .items_center()
        .gap(px(5.))
        .pl(px(8. + depth as f32 * 14.))
        .pr(px(6.))
        .rounded(px(4.))
        .bg(if selected {
            cx.theme().accent
        } else {
            cx.theme().transparent
        })
        .text_color(if invalid {
            cx.theme().danger
        } else {
            cx.theme().foreground
        })
        .text_sm()
        .child(
            div()
                .id("disclosure")
                .size(px(12.))
                .child(disclosure)
                .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
                    if folder {
                        cx.stop_propagation();
                    }
                })
                .on_click(move |_, window, cx| {
                    if let Some(toggle) = &toggle {
                        cx.stop_propagation();
                        toggle(window, cx);
                    }
                }),
        )
        .child(icon.size(px(16.)))
        .child(div().min_w_0().flex_1().truncate().child(label))
}
