//! Recursive containers and editor references have a small stack independent of leaf control complexity.
use super::*;
use crate::ui::controls::vertical_scrollbar;
use gpui_base::{Tab, Tabs};
use gpui_kit::{
    AnyElement, InteractiveElement, ParentElement, SharedString, StatefulInteractiveElement,
    Styled, div, prelude::FluentBuilder as _, px,
};
use plugin_runtime::plugin_protocol::ui::Node;

impl PluginView {
    /// Borrow native editor state or arrange/scroll children through generic publicly declared geometry.
    pub(super) fn layout_node(
        &mut self,
        node: &Node,
        disabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let native_id = SharedString::from(format!("plugin-ui-{}", node.id));
        let colors = self.colors(node.theme_role(), cx);
        match &node.kind {
            Kind::NativeEditor { document } => self
                .native_editor
                .as_ref()
                .filter(|_| !disabled)
                .and_then(|render| render(document, window, cx))
                .unwrap_or_else(|| div().into_any_element()),
            Kind::Column { children } | Kind::Row { children } => {
                let children: Vec<_> = children
                    .iter()
                    .map(|n| self.node(n, disabled, window, cx))
                    .collect();
                if node.layout.resizable {
                    crate::ui::controls::split_container(
                        SharedString::from(node.id.clone()),
                        matches!(node.kind, Kind::Column { .. }),
                        children,
                        cx,
                    )
                } else {
                    div()
                        .flex()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .gap(px(node.layout.gap))
                        .when(matches!(node.kind, Kind::Column { .. }), |v| v.flex_col())
                        .when(
                            matches!(node.kind, Kind::Row { .. }) && node.layout.wrap,
                            |v| v.flex_wrap(),
                        )
                        .children(children)
                        .into_any_element()
                }
            }
            Kind::Scroll { content } => {
                // Materialize mapped content near the viewport while retaining native reveal targets.
                let content = self.windowed_content(content, &node.id);
                let child = self.node(&content, disabled, window, cx);
                let handle = self.scrolls.get(&node.id).cloned().unwrap_or_default();
                div()
                    .relative()
                    .size_full()
                    .min_h_0()
                    .child(
                        div()
                            .id(native_id.clone())
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&handle)
                            .child(child),
                    )
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .child(vertical_scrollbar(&handle, cx)),
                    )
                    .into_any_element()
            }
            Kind::Tabs { tabs, selected } => {
                let mut strip = Tabs::new(native_id.clone())
                    .flex()
                    .gap_1()
                    .border_b_1()
                    .border_color(colors.border);
                for (index, tab) in tabs.iter().enumerate() {
                    let node_id = node.id.clone();
                    let tab_id = tab.id.clone();
                    strip = strip.child(
                        Tab::new(SharedString::from(format!("{}-tab-{index}", node.id)))
                            .selected(&tab.id == selected)
                            .disabled(disabled)
                            .accessibility_label(tab.label.clone())
                            .set_position(index + 1, tabs.len())
                            .px_3()
                            .py_2()
                            .bg(if &tab.id == selected {
                                colors.active
                            } else {
                                colors.background
                            })
                            .hover(|s| s.bg(colors.hover))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.emit(&node_id, Action::Select(tab_id.clone()), cx)
                            }))
                            .child(tab.label.clone()),
                    );
                }
                let mut body = div().flex().flex_col().gap_2().child(strip);
                if let Some(tab) = tabs.iter().find(|t| &t.id == selected) {
                    body = body.child(self.node(&tab.content, disabled, window, cx));
                }
                body.into_any_element()
            }
            _ => unreachable!("layout kinds are dispatched separately"),
        }
    }
}
