//! Render portable trees through gpui-base behavior with editor-owned appearance.
use super::*;
use crate::ui::controls::{Button, ButtonCustomVariant};
use gpui_base::Dialog;
use gpui_kit::{
    AnyElement, InteractiveElement, ParentElement, SharedString, Styled, div,
    prelude::FluentBuilder as _, px,
};
use plugin_runtime::plugin_protocol::ui::Node;

impl PluginView {
    pub(super) fn render_document(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.sync_widgets(window, cx);
        self.sync_code_highlighting(cx);
        let viewport_frame = self.begin_viewport_frame();
        let revision = self.document.revision;
        let root = self.document.root.clone();
        let body = self.node(&root, false, window, cx);
        let mut view = div()
            .w_full()
            .when(!self.content_sized, |view| view.h_full())
            .when(self.content_sized, |view| view.h_auto().flex_shrink_0())
            .flex()
            .flex_col()
            .tab_group()
            .track_focus(&self.view_focus)
            .capture_key_down(cx.listener(|view, _, window, cx| {
                // Register on the focused root's ancestry; an overlay's paint node may be a sibling.
                if view.view_focus.contains_focused(window, cx) {
                    view.viewport.cancel_locate();
                }
            }))
            .relative()
            .overflow_hidden()
            .bg(self.colors("container", cx).background)
            .child(body);
        // Popup anchors use this composed view's native origin, never the containing editor window origin.
        let owner = cx.entity().downgrade();
        let released = cx.entity().downgrade();
        let viewport = cx.entity().downgrade();
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
                move |_, _, window, cx| {
                    // All source blocks have completed prepaint before this composed view paints.
                    let _ = viewport.update(cx, |view, cx| {
                        view.finish_viewport_frame(viewport_frame, revision, cx);
                    });
                    let input = viewport.clone();
                    window.on_mouse_event(
                        move |event: &gpui_kit::ScrollWheelEvent, phase, _, cx| {
                            if phase.capture() {
                                let _ = input.update(cx, |view, _| {
                                    view.viewport_wheel(event.position, revision);
                                });
                            }
                        },
                    );
                    // A scrollbar press supersedes a queued locate before Base begins its native drag.
                    let pointer = viewport.clone();
                    window.on_mouse_event(move |event: &gpui_kit::MouseDownEvent, phase, _, cx| {
                        if phase.capture() {
                            let _ = pointer.update(cx, |view, _| {
                                view.viewport_pointer_down(event.position, revision)
                            });
                        }
                    });
                    let drag = viewport.clone();
                    window.on_mouse_event(move |event: &gpui_kit::MouseMoveEvent, phase, _, cx| {
                        // A request can arrive after the press; every held move renews manual ownership.
                        if phase.capture() && event.pressed_button.is_some() {
                            let _ = drag.update(cx, |view, _| view.viewport_pointer_move());
                        }
                    });
                    // Every release ends ownership, including releases outside the link's hit box.
                    // Defer cleanup so Base can consume a matching link release in this dispatch.
                    let released = released.clone();
                    window.on_mouse_event(move |_: &gpui_kit::MouseUpEvent, phase, _, cx| {
                        if phase.capture() {
                            let _ = released.update(cx, |view, _| view.viewport_pointer_up());
                            let released = released.clone();
                            cx.defer(move |cx| {
                                let _ = released.update(cx, |view, _| view.link_press = None);
                            });
                        }
                    });
                },
            )
            .absolute()
            .size_full(),
        );
        if let Some(popup) = &self.popup {
            // Menus overlay their owner; a full-size widget must not consume a second flex row.
            view = view.child(
                div()
                    .debug_selector(|| "plugin-popup-menu".into())
                    .absolute()
                    .inset_0()
                    .child(popup.clone()),
            );
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

    pub(super) fn node(
        &mut self,
        node: &Node,
        parent_disabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let disabled = node.disabled || parent_disabled;
        // Recursive geometry/editor borrowing stays outside the large leaf control renderer's stack.
        let content = if matches!(
            node.kind,
            Kind::NativeEditor { .. }
                | Kind::Row { .. }
                | Kind::Column { .. }
                | Kind::Scroll { .. }
                | Kind::Tabs { .. }
        ) {
            self.layout_node(node, disabled, window, cx)
        } else {
            self.leaf_node(node, disabled || !self.scene_current.get(), window, cx)
        };
        let content = self.linked_content(
            content,
            node,
            disabled || !self.scene_current.get(),
            window,
            cx,
        );
        self.node_shell(node, disabled, content, cx)
    }

    /// Build decoration after descendants return: GPUI builder temporaries must not occupy
    /// every recursive frame of a nested list/table/tree on the default native thread stack.
    #[inline(never)]
    fn node_shell(
        &self,
        node: &Node,
        disabled: bool,
        content: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = self.colors(node.theme_role(), cx);
        let debug_id = format!("plugin-ui-{}", node.id);
        let owner = cx.entity().downgrade();
        let block = node.id.clone();
        let revision = self.document.revision;
        let pressed_node = node.id.clone();
        self.font(
            div()
                .id(SharedString::from(format!("plugin-ui-{}-wrapper", node.id)))
                .relative()
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
                .child(content)
                .when(
                    self.document.link_events
                        && matches!(node.kind, Kind::RichText { .. })
                        && !disabled,
                    |view| {
                        view.capture_any_mouse_down(cx.listener(move |this, event, _, _| {
                            this.press_link(&pressed_node, revision, event);
                        }))
                    },
                )
                .when(
                    node.source_range.is_some() || !node.links.is_empty(),
                    |view| {
                        view.child(
                            gpui_kit::canvas(
                                move |bounds, _, cx| {
                                    let _ = owner.update(cx, |this, cx| {
                                        this.measure_block(&block, revision, bounds, cx);
                                    });
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_full(),
                        )
                    },
                ),
            node.theme_role(),
        )
        .into_any_element()
    }
}
