//! Render portable trees through gpui-base behavior with editor-owned appearance.
use super::*;
use crate::ui::controls::{Button, ButtonCustomVariant, Input, vertical_scrollbar};
use gpui_base::{Checkbox, CheckboxState, Dialog, Progress, Radio, RadioGroup, Tab, Tabs};
use gpui_kit::{
    AnyElement, InteractiveElement, ParentElement, SharedString, StatefulInteractiveElement,
    Styled, div, prelude::FluentBuilder as _, px, relative,
};
use plugin_runtime::plugin_protocol::ui::Node;

impl PluginView {
    pub(super) fn render_document(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.sync_widgets(window, cx);
        let root = self.document.root.clone();
        let body = self.node(&root, false, window, cx);
        let mut view = div()
            .w_full()
            .when(!self.content_sized, |view| view.h_full())
            .when(self.content_sized, |view| view.h_auto().flex_shrink_0())
            .flex()
            .flex_col()
            .tab_group()
            .relative()
            .overflow_hidden()
            .bg(self.colors("container", cx).background)
            .child(body);
        // Popup anchors use this composed view's native origin, never the containing editor window origin.
        let owner = cx.entity().downgrade();
        view = view.child(
            gpui_kit::canvas(
                move |bounds, _, cx| {
                    let _ = owner.update(cx, |this, cx| {
                        if this.origin != bounds.origin {
                            this.origin = bounds.origin;
                            cx.notify();
                        }
                    });
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        );
        if let Some(popup) = &self.popup {
            // Menus overlay their owner; a full-size widget must not consume a second flex row.
            view = view.child(div().absolute().inset_0().child(popup.clone()));
        }
        if let Some(dialog) = self.document.dialog.clone() {
            // Dismiss stays blocked until the guest acknowledges it by removing the modal.
            let content = self.node(&dialog.content, false, window, cx);
            let colors = self.colors("dialog", cx);
            let id = dialog.id.clone();
            let close = self.font(
                Button::new("plugin-dialog-close")
                    .label("×")
                    .accessibility_label("关闭")
                    .custom(
                        ButtonCustomVariant::new(cx)
                            .color(colors.background)
                            .foreground(colors.foreground)
                            .hover(colors.hover)
                            .active(colors.active),
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.emit(&id, Action::Dismiss, cx)),
                    ),
                "dialog",
            );
            let popup = self.font(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_4()
                    .w(px(dialog.width))
                    .max_w(window.viewport_size().width - px(32.))
                    .max_h(window.viewport_size().height - px(32.))
                    .bg(colors.background)
                    .text_color(colors.foreground)
                    .border_1()
                    .border_color(colors.border)
                    .rounded(px(6.))
                    .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(dialog.title)
                            .child(close),
                    )
                    .child(content),
                "dialog",
            );
            let id = dialog.id.clone();
            let owner = cx.entity().downgrade();
            view = view.child(
                Dialog::new(cx)
                    .focus_handle(self.dialog_focus.clone())
                    .on_ok(|_, _, _| false)
                    .on_cancel(move |_, _, cx| {
                        let _ = owner.update(cx, |this, cx| this.emit(&id, Action::Dismiss, cx));
                        false
                    })
                    .backdrop(
                        div()
                            .size_full()
                            .bg(self.colors("dialog", cx).background.opacity(0.7)),
                    )
                    .popup(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(popup),
                    ),
            );
        }
        view.into_any_element()
    }

    fn node(
        &mut self,
        node: &Node,
        parent_disabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let disabled = node.disabled || parent_disabled;
        let colors = self.colors(node.theme_role(), cx);
        let id = node.id.clone();
        let native_id = SharedString::from(format!("plugin-ui-{}", node.id));
        let content = match &node.kind {
            Kind::SideTabs(_) => self.collections[&node.id].clone().into_any_element(),
            Kind::Canvas(_) => self.canvases[&node.id].clone().into_any_element(),
            Kind::Column { children } | Kind::Row { children } => {
                let children: Vec<_> = children
                    .iter()
                    .map(|n| self.node(n, disabled, window, cx))
                    .collect();
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
            Kind::Scroll { content } => {
                let child = self.node(content, disabled, window, cx);
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
            Kind::Text { text } => div().child(text.clone()).into_any_element(),
            Kind::RichText { html } => crate::ui::controls::rich_text_view(
                native_id.clone(),
                html.clone(),
                px(self
                    .environment
                    .font_style(&self.plugin, node.theme_role(), false)
                    .size_px
                    .unwrap_or(14.)),
                crate::ui::controls::RichTextColors {
                    foreground: colors.foreground,
                    background: colors.background,
                    border: colors.border,
                    link: colors.accent,
                },
                cx,
            )
            .into_any_element(),
            Kind::CodeBlock { text, .. } => {
                // Each literal line keeps its whitespace; the host never parses guest code as markup.
                let font = self
                    .environment
                    .font_style(&self.plugin, node.theme_role(), true);
                div()
                    .id(native_id.clone())
                    .w_full()
                    .overflow_x_scroll()
                    .bg(colors.background)
                    .border_1()
                    .border_color(colors.border)
                    .rounded(px(4.))
                    .p_3()
                    .font_family(font.family.unwrap_or_else(|| "Consolas".into()))
                    .text_size(px(font.size_px.unwrap_or(14.)))
                    .child(div().flex().flex_col().children(text.lines().map(|line| {
                        div()
                            .flex_shrink_0()
                            .whitespace_nowrap()
                            .child(if line.is_empty() { " " } else { line }.to_owned())
                    })))
                    .into_any_element()
            }
            Kind::Button { label } => self
                .font(
                    Button::new(native_id.clone())
                        .label(label.clone())
                        .when_some(node.tooltip.clone(), |button, tooltip| {
                            button.accessibility_label(tooltip.clone()).tooltip(tooltip)
                        })
                        .disabled(disabled)
                        .border_color(colors.border)
                        .custom(
                            ButtonCustomVariant::new(cx)
                                .color(colors.background)
                                .foreground(colors.foreground)
                                .hover(colors.hover)
                                .active(colors.active),
                        )
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.emit(&id, Action::Click, cx)),
                        ),
                    node.theme_role(),
                )
                .into_any_element(),
            Kind::Input(_) => self
                .font(
                    Input::new(&self.inputs[&node.id].state)
                        .bg(colors.background)
                        .text_color(colors.foreground)
                        .border_color(colors.border),
                    node.theme_role(),
                )
                .into_any_element(),
            Kind::Checkbox { label, checked } => {
                let owner = cx.entity().downgrade();
                let marker = div()
                    .size(px(16.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_1()
                    .border_color(colors.border)
                    .rounded(px(3.))
                    .bg(if *checked {
                        colors.accent
                    } else {
                        colors.background
                    })
                    .text_color(colors.accent_foreground)
                    .when(*checked, |v| v.child("✓"));
                self.font(
                    Checkbox::new(native_id.clone())
                        .checked(*checked)
                        .disabled(disabled)
                        .accessibility_label(label.clone())
                        .flex()
                        .items_center()
                        .gap_2()
                        .on_change(move |state, _, _, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                this.emit(&id, Action::Toggle(state == CheckboxState::Checked), cx)
                            });
                        })
                        .child(marker)
                        .child(label.clone()),
                    node.theme_role(),
                )
                .into_any_element()
            }
            Kind::Choice { options, selected } => {
                let mut group = RadioGroup::new(native_id.clone()).flex().flex_col().gap_2();
                for (index, option) in options.iter().enumerate() {
                    let owner = cx.entity().downgrade();
                    let checked = selected.as_ref() == Some(&option.id);
                    let node_id = node.id.clone();
                    let option_id = option.id.clone();
                    let marker = div()
                        .size(px(14.))
                        .rounded_full()
                        .border_1()
                        .border_color(colors.border)
                        .bg(if checked {
                            colors.accent
                        } else {
                            colors.background
                        });
                    group = group.child(
                        Radio::new(SharedString::from(format!("{}-option-{index}", node.id)))
                            .checked(checked)
                            .disabled(disabled || option.disabled)
                            .accessibility_label(option.label.clone())
                            .set_position(index + 1, options.len())
                            .flex()
                            .items_center()
                            .gap_2()
                            .on_change(move |_, _, _, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    this.emit(&node_id, Action::Select(option_id.clone()), cx)
                                });
                            })
                            .child(marker)
                            .child(option.label.clone()),
                    );
                }
                group.into_any_element()
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
            Kind::List { items } => div()
                .flex()
                .flex_col()
                .gap_1()
                .children(items.iter().map(|text| div().py_1().child(text.clone())))
                .into_any_element(),
            Kind::Table { headers, rows } => {
                let mut table = div()
                    .flex()
                    .flex_col()
                    .border_1()
                    .border_color(colors.border);
                for (index, row) in std::iter::once(headers).chain(rows).enumerate() {
                    table = table.child(
                        div()
                            .flex()
                            .when(index == 0, |v| v.bg(colors.active))
                            .border_b_1()
                            .border_color(colors.border)
                            .children(
                                row.iter()
                                    .map(|cell| div().flex_1().min_w_0().p_2().child(cell.clone())),
                            ),
                    );
                }
                table.into_any_element()
            }
            Kind::Separator => div()
                .w_full()
                .h(px(1.))
                .bg(colors.border)
                .into_any_element(),
            Kind::Progress { label, value } => Progress::new(native_id.clone())
                .value(*value)
                .accessibility_label(label.clone())
                .h(px(8.))
                .w_full()
                .rounded(px(4.))
                .bg(colors.background)
                .border_1()
                .border_color(colors.border)
                .child(
                    div()
                        .h_full()
                        .w(relative(*value / 100.))
                        .bg(colors.accent)
                        .rounded(px(4.)),
                )
                .into_any_element(),
            Kind::Spacer => div().min_h(px(8.)).into_any_element(),
        };
        let debug_id = format!("plugin-ui-{}", node.id);
        self.font(
            div()
                .id(SharedString::from(format!("plugin-ui-{}-wrapper", node.id)))
                .debug_selector(move || debug_id.clone())
                .flex()
                .flex_col()
                .flex_shrink_0()
                // Wrapped groups need a bounded outer box as well as a wrapping inner flex row.
                .when(node.layout.wrap, |wrapper| wrapper.max_w_full())
                .min_w_0()
                .min_h_0()
                .when(node.layout.grow, |v| v.flex_1())
                .when_some(node.layout.width, |v, w| v.w(px(w)))
                .when_some(node.layout.height, |v, h| v.h(px(h)))
                .p(px(node.layout.padding))
                .bg(colors.background)
                .text_color(colors.foreground)
                .when(disabled, |v| v.opacity(0.5))
                .child(content),
            node.theme_role(),
        )
        .into_any_element()
    }
}
