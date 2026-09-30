//! Vertical tab list: native selection, keyed editing, scrolling, reorder and captured resize.
use super::{Input, menu::MenuStyle, vertical_scrollbar};
#[cfg(test)]
mod tests;
use gpui_base::{
    Tab, Tabs,
    input::{InputEvent, InputState},
};
use gpui_kit::{
    App, AppContext as _, Bounds, Context, DispatchPhase, Entity, FocusHandle, Focusable as _,
    FontWeight, Hsla, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, canvas, div, px,
};
use plugin_runtime::plugin_protocol::ui::{Action, SideTabs};
use std::{cell::Cell, rc::Rc};

#[derive(Clone)]
pub(crate) struct SideTabsStyle {
    pub background: Hsla,
    pub border: Hsla,
    pub menu: MenuStyle,
    pub active: Hsla,
    pub active_foreground: Hsla,
    pub close_background: Option<Hsla>,
    pub close_foreground: Option<Hsla>,
    pub rename_background: Option<Hsla>,
    pub rename_foreground: Option<Hsla>,
}
struct Editing {
    id: String,
    state: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}
pub(crate) struct SideTabBar {
    pub model: SideTabs,
    pub style: SideTabsStyle,
    scroll: ScrollHandle,
    bounds: Bounds<Pixels>,
    focus: FocusHandle,
    editing: Option<Editing>,
    requested_rename: Option<String>,
    drag: Option<String>,
    resizing: Option<(Pixels, f32)>,
    preview_width: Rc<Cell<Option<f32>>>,
    suppress_click: bool,
    sink: Rc<dyn Fn(Action, &mut Window, &mut App)>,
    preview: Rc<dyn Fn(&mut App)>,
}
impl SideTabBar {
    pub fn new(
        model: SideTabs,
        style: SideTabsStyle,
        focus: FocusHandle,
        sink: impl Fn(Action, &mut Window, &mut App) + 'static,
        preview_width: Rc<Cell<Option<f32>>>,
        preview: impl Fn(&mut App) + 'static,
        cx: &mut Context<Self>,
    ) -> Self {
        let _ = cx;
        Self {
            model,
            style,
            focus,
            scroll: ScrollHandle::new(),
            bounds: Bounds::default(),
            editing: None,
            requested_rename: None,
            drag: None,
            resizing: None,
            preview_width,
            suppress_click: false,
            sink: Rc::new(sink),
            preview: Rc::new(preview),
        }
    }
    pub fn update(
        &mut self,
        model: SideTabs,
        style: SideTabsStyle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rename = model.rename.clone();
        let new_request = rename != self.requested_rename;
        if self
            .editing
            .as_ref()
            .is_some_and(|e| !model.items.iter().any(|i| i.id == e.id && !i.disabled))
        {
            self.editing = None;
        }
        if self.model.selected != model.selected {
            if let Some(index) = model
                .items
                .iter()
                .position(|item| Some(&item.id) == model.selected.as_ref())
            {
                self.scroll.scroll_to_item(index);
            }
        }
        self.model = model;
        self.style = style;
        if new_request {
            self.requested_rename = rename.clone();
            if let Some(id) = rename {
                self.begin_edit(&id, window, cx);
            }
        }
        cx.notify();
    }
    fn emit(&self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
        (self.sink)(action, window, cx);
    }
    fn begin_edit(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self
            .model
            .items
            .iter()
            .find(|item| item.id == id && !item.disabled)
        else {
            return;
        };
        let id = item.id.clone();
        let label = item.label.clone();
        self.drag = None;
        let state = cx.new(|cx| InputState::new(window, cx).default_value(label));
        let enter = cx.subscribe_in(&state, window, |this, _, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.finish_edit(true, window, cx);
            }
        });
        let blur = cx.on_focus_out(&state.focus_handle(cx), window, |this, _, window, cx| {
            this.finish_edit(true, window, cx)
        });
        self.editing = Some(Editing {
            id,
            state: state.clone(),
            _subscriptions: vec![enter, blur],
        });
        window.defer(cx, move |window, cx| {
            state.update(cx, |state, cx| {
                state.focus(window, cx);
                state.select_all(window, cx);
            })
        });
        cx.notify();
    }
    fn finish_edit(&mut self, commit: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editing) = self.editing.take() else {
            return;
        };
        let value = editing.state.read(cx).value().to_string();
        self.focus.focus(window, cx);
        if commit {
            (self.sink)(
                Action::Rename {
                    id: editing.id,
                    value,
                },
                window,
                cx,
            );
        }
        cx.notify();
    }
    fn pointer_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.drag.is_some() {
            cx.stop_propagation();
        }
        if let Some((start, width)) = self.resizing {
            if event.pressed_button != Some(MouseButton::Left) {
                // A lost release cancels the preview instead of leaving the divider displaced.
                self.resizing = None;
                self.model.width = width;
                self.preview_width.set(None);
                (self.preview)(cx);
                cx.notify();
                return;
            }
            let width = (width + (start - event.position.x) / px(1.))
                .clamp(self.model.min_width, self.model.max_width);
            if width != self.model.width {
                // Preview the divider locally; avoid a WASM round trip and PTY resize per pixel.
                self.model.width = width;
                self.preview_width.set(Some(width));
                (self.preview)(cx);
                cx.notify();
            }
            cx.stop_propagation();
        }
    }
    fn pointer_up(&mut self, event: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.button == MouseButton::Left
            && let Some((_, start_width)) = self.resizing.take()
        {
            // Commit exactly once; retain the preview until the guest returns its new layout.
            if self.model.width != start_width {
                self.emit(Action::Resize(self.model.width), window, cx);
            } else {
                self.preview_width.set(None);
                (self.preview)(cx);
            }
            cx.stop_propagation();
            return;
        }
        let Some(from) = self.drag.take() else {
            return;
        };
        if event.button != MouseButton::Left || !self.bounds.contains(&event.position) {
            return;
        }
        let index = ((event.position.y - self.bounds.top() - self.scroll.offset().y) / px(32.))
            .floor()
            .max(0.) as usize;
        if let Some(item) = self
            .model
            .items
            .get(index)
            .filter(|item| item.id != from && !item.disabled)
        {
            self.suppress_click = true;
            self.emit(
                Action::Move {
                    from,
                    to: item.id.clone(),
                },
                window,
                cx,
            );
            cx.stop_propagation();
        }
    }
}
impl Render for SideTabBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let style = self.style.clone();
        let mut tabs = Tabs::new("side-tabs-list").flex().flex_col().w_full();
        for (index, item) in self.model.items.iter().enumerate() {
            let id = item.id.clone();
            let debug = format!("side-tab-{}", id);
            let active = self.model.selected.as_ref() == Some(&id);
            let mut row = div()
                .id(SharedString::from(debug.clone()))
                .debug_selector(move || debug.clone())
                .h(px(32.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .bg(if active {
                    style.active
                } else {
                    style.menu.surface
                })
                .border_b_1()
                .border_color(style.menu.border);
            if !active {
                row = row.border_l_1().border_color(style.border);
            }
            if let Some(editing) = self.editing.as_ref().filter(|edit| edit.id == id) {
                row = row
                    .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if event.keystroke.key == "escape" {
                            this.finish_edit(false, window, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .child(
                        Input::new(&editing.state)
                            .bg(style.rename_background.unwrap_or(style.active))
                            .text_color(style.rename_foreground.unwrap_or(style.active_foreground))
                            .font_family(style.menu.font_family.clone())
                            .text_size(px(style.menu.font_size)),
                    );
            } else {
                let clicked = id.clone();
                let pressed = id.clone();
                let context = id.clone();
                let middle = id.clone();
                let tab = Tab::new(SharedString::from(format!("select-{id}")))
                    .selected(active)
                    .disabled(item.disabled)
                    .accessibility_label(item.label.clone())
                    .set_position(index + 1, self.model.items.len())
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .px_2()
                    .text_color(if active {
                        style.active_foreground
                    } else {
                        style.menu.foreground
                    })
                    .hover(|s| s.bg(style.menu.hover))
                    .capture_any_mouse_down(cx.listener(
                        move |this, event: &MouseDownEvent, window, cx| {
                            if event.button != MouseButton::Left {
                                return;
                            }
                            if this
                                .model
                                .items
                                .iter()
                                .any(|i| i.id == pressed && i.disabled)
                            {
                                return;
                            }
                            this.suppress_click = false;
                            if event.click_count >= 2 {
                                this.begin_edit(&pressed, window, cx);
                                cx.stop_propagation();
                            } else {
                                this.drag = Some(pressed.clone());
                            }
                        },
                    ))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            if this
                                .model
                                .items
                                .iter()
                                .any(|item| item.id == context && !item.disabled)
                            {
                                this.emit(
                                    Action::Context {
                                        id: context.clone(),
                                        x: (event.position.x - this.bounds.left()) / px(1.),
                                        y: (event.position.y - this.bounds.top()) / px(1.),
                                    },
                                    window,
                                    cx,
                                );
                            }
                            cx.stop_propagation();
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Middle,
                        cx.listener(move |this, _, window, cx| {
                            if this
                                .model
                                .items
                                .iter()
                                .any(|i| i.id == middle && i.closable && !i.disabled)
                            {
                                this.emit(Action::Close(middle.clone()), window, cx);
                            }
                            cx.stop_propagation();
                        }),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if !this.suppress_click && this.editing.is_none() {
                            this.emit(Action::Select(clicked.clone()), window, cx);
                        }
                        this.suppress_click = false;
                    }))
                    .child(div().min_w_0().whitespace_nowrap().text_ellipsis().child(
                        match &item.status {
                            Some(status) => format!("{} · {status}", item.label),
                            None => item.label.clone(),
                        },
                    ));
                row = row.child(tab);
                if item.closable {
                    let close = id.clone();
                    row = row.child(
                        gpui_base::Button::new(SharedString::from(format!("close-{id}")))
                            .accessibility_label("关闭标签")
                            .disabled(item.disabled)
                            .w(px(28.))
                            .h_full()
                            .bg(style.close_background.unwrap_or(if active {
                                style.active
                            } else {
                                style.menu.surface
                            }))
                            .text_color(style.close_foreground.unwrap_or(if active {
                                style.active_foreground
                            } else {
                                style.menu.foreground
                            }))
                            .hover(|s| s.bg(style.menu.hover))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.emit(Action::Close(close.clone()), window, cx)
                            }))
                            .child("×"),
                    );
                }
            }
            tabs = tabs.child(row);
        }
        let owner = cx.entity().downgrade();
        let paint_owner = owner.clone();
        div()
            .id("native-side-tabs")
            .debug_selector(|| "native-side-tabs".into())
            .relative()
            .size_full()
            .bg(style.background)
            .font_family(style.menu.font_family.clone())
            .text_size(px(style.menu.font_size))
            .font_weight(if style.menu.bold {
                FontWeight::BOLD
            } else {
                FontWeight::NORMAL
            })
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Middle, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .w(px(1.))
                    .h_full()
                    .bg(style.border),
            )
            .child(
                div()
                    .id("side-tabs-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(tabs),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .child(vertical_scrollbar(&self.scroll, cx)),
            )
            .child(
                div()
                    .id("side-tabs-resize")
                    .debug_selector(|| "side-tabs-resize".into())
                    .absolute()
                    .left_0()
                    .top_0()
                    .w(px(6.))
                    .h_full()
                    .cursor_col_resize()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            if this.editing.is_none() {
                                this.resizing = Some((event.position.x, this.model.width));
                                this.drag = None;
                            }
                            cx.stop_propagation();
                        }),
                    ),
            )
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let _ = owner.update(cx, |this, _| this.bounds = bounds);
                    },
                    move |_, _, window, _| {
                        let moved = paint_owner.clone();
                        let released = paint_owner.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ = moved
                                    .update(cx, |this, cx| this.pointer_move(event, window, cx));
                            }
                        });
                        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ = released
                                    .update(cx, |this, cx| this.pointer_up(event, window, cx));
                            }
                        });
                    },
                )
                .absolute()
                .inset_0()
                .size_full(),
            )
    }
}
