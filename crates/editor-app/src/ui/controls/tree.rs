//! Local tree-row appearance; gpui-base owns selection, keyboard navigation and virtualization.
use super::Icon;
use crate::ui::{theme::component_styles, typography};
use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::component::{ActiveTheme as _, IconName};
use gpui_kit::{
    App, Div, InteractiveElement, IntoElement, ParentElement, Stateful, Styled, div, px,
};
use plugin_schema::ThemeComponent;

/// Select the compact Explorer theme contract without changing configuration-tree presentation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TreeRowAppearance {
    Standard,
    Explorer,
}

impl TreeRowAppearance {
    /// Apply Explorer's live typography and selected/base tokens to a native row.
    fn apply(self, row: Stateful<Div>, depth: usize, selected: bool, cx: &App) -> Stateful<Div> {
        if self == Self::Standard {
            return row;
        }
        let styles = component_styles(cx, ThemeComponent::ExplorerRow);
        let style = if selected {
            styles.selected
        } else {
            styles.base
        };
        let tree_style = component_styles(cx, ThemeComponent::ExplorerTree).base;
        // Resolve the same theme tokens as Explorer, including live font sizing and selection.
        row.h_auto()
            .min_h(px(24.))
            .gap_1()
            .px(px(style.padding_x_px.unwrap_or(4.)))
            .py(px(style.padding_y_px.unwrap_or(0.3)))
            .pl(px(8. + depth as f32 * 14.))
            .rounded(px(if selected {
                style.radius_px.unwrap_or(5.)
            } else {
                0.
            }))
            // Match Explorer's background-only selection, without a leading accent border.
            .bg(style.background.unwrap_or(cx.theme().background))
            .text_color(style.foreground.unwrap_or(cx.theme().foreground))
            .text_size(
                tree_style
                    .font_size_px
                    .map(px)
                    .unwrap_or(typography::font_size(cx)),
            )
            .font_family(cx.theme().mono_font_family.clone())
    }
}

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
    appearance: TreeRowAppearance,
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
    let row = div()
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
        .text_sm();
    appearance
        .apply(row, depth, selected, cx)
        .child(
            div()
                .id("disclosure")
                // Explorer centers its 12px disclosure glyph in a 16px slot beside the icon.
                .size(px(if appearance == TreeRowAppearance::Explorer {
                    16.
                } else {
                    12.
                }))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
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
        .child(icon.size(px(16.)).flex_shrink_0())
        .child(div().min_w_0().flex_1().truncate().child(label))
}
