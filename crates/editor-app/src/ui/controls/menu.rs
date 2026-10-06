//! Shared explorer-style menu appearance and a native popup with keyboard navigation.
use gpui_kit::{
    App, Context, Div, ElementId, FocusHandle, FontWeight, Hsla, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, ParentElement, Pixels, Point, Render, Role, ScrollHandle,
    SharedString, StatefulInteractiveElement, Styled, Window, anchored, deferred, div, point, px,
};
use gpui_kit::{component::ActiveTheme as _, prelude::FluentBuilder as _};
use plugin_runtime::plugin_protocol::ui::{Action, MenuItem};
use std::rc::Rc;
#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(crate) struct MenuStyle {
    pub surface: Hsla,
    pub foreground: Hsla,
    pub border: Hsla,
    pub hover: Hsla,
    pub hover_foreground: Hsla,
    pub radius: f32,
    pub font_size: f32,
    pub padding_x: f32,
    pub padding_y: f32,
    pub font_family: SharedString,
    pub bold: bool,
}
impl MenuStyle {
    pub fn current(cx: &App) -> Self {
        let styles =
            crate::ui::theme::component_styles(cx, plugin_schema::ThemeComponent::ExplorerMenu);
        Self {
            surface: styles.base.background.unwrap_or(cx.theme().popover),
            foreground: styles
                .base
                .foreground
                .unwrap_or(cx.theme().popover_foreground),
            border: styles.base.border.unwrap_or(cx.theme().border),
            hover: styles.hover.background.unwrap_or(cx.theme().list_hover),
            hover_foreground: styles.hover.foreground.unwrap_or(cx.theme().foreground),
            radius: styles.base.radius_px.unwrap_or(6.),
            font_size: styles.base.font_size_px.unwrap_or(13.),
            padding_x: styles.base.padding_x_px.unwrap_or(6.),
            padding_y: styles.base.padding_y_px.unwrap_or(5.),
            font_family: cx.theme().font_family.clone(),
            bold: false,
        }
    }
    pub fn card(&self, width: f32) -> Div {
        div()
            .flex()
            .flex_col()
            .w(px(width))
            .p(px(self.padding_y))
            .rounded(px(self.radius))
            .border_1()
            .border_color(self.border)
            .bg(self.surface)
            .text_color(self.foreground)
            .font_family(self.font_family.clone())
            .text_size(px(self.font_size))
            .font_weight(if self.bold {
                FontWeight::BOLD
            } else {
                FontWeight::NORMAL
            })
            .shadow_md()
    }
    pub fn row(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        selected: bool,
    ) -> gpui_base::Button {
        // Buttons own their text style so submenu triggers match ordinary menu rows.
        gpui_base::Button::new(id)
            .role(Role::MenuItem)
            .accessibility_label(label)
            .focusable(false)
            .flex()
            .h(px(29.))
            .w_full()
            .items_center()
            .font_family(self.font_family.clone())
            .text_size(px(self.font_size))
            .font_weight(if self.bold {
                FontWeight::BOLD
            } else {
                FontWeight::NORMAL
            })
            .px(px(self.padding_x + 6.))
            .rounded(px((self.radius - 2.).max(2.)))
            .bg(if selected { self.hover } else { self.surface })
            .text_color(if selected {
                self.hover_foreground
            } else {
                self.foreground
            })
    }
}

/// The popup owns focus/navigation/scrolling; callers receive stable item identities.
pub(crate) struct PopupMenu {
    pub items: Vec<MenuItem>,
    pub style: MenuStyle,
    pub position: Point<Pixels>,
    focus: FocusHandle,
    previous: Option<FocusHandle>,
    selected: Option<usize>,
    closed: bool,
    scroll: ScrollHandle,
    /// The caller chooses a local layout width; actual rendering still clamps it to the viewport.
    width: f32,
    /// Generic row diagnostics supplied by a caller; keyboard selection remains available for repair.
    notices: std::collections::BTreeMap<String, String>,
    sink: Rc<dyn Fn(Action, &mut Window, &mut App)>,
}
impl PopupMenu {
    pub fn new(
        items: Vec<MenuItem>,
        style: MenuStyle,
        position: Point<Pixels>,
        sink: impl Fn(Action, &mut Window, &mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let previous = window.focused(cx);
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let selected = items.iter().position(|item| !item.disabled);
        Self {
            items,
            style,
            position,
            focus,
            previous,
            selected,
            closed: false,
            scroll: ScrollHandle::new(),
            width: 230.,
            notices: Default::default(),
            sink: Rc::new(sink),
        }
    }
    fn finish(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        self.closed = true;
        if let Some(focus) = self.previous.take() {
            focus.focus(window, cx);
        }
        (self.sink)(action, window, cx);
        cx.emit(gpui_kit::DismissEvent);
        cx.notify();
    }
    /// Configure width without duplicating popup focus, scroll or button behavior in callers.
    pub fn width(mut self, width: f32) -> Self {
        if width.is_finite() && width > 0. {
            self.width = width;
        }
        self
    }
    fn key(&mut self, key: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        match key.keystroke.key.as_str() {
            "escape" => self.finish(Action::Dismiss, window, cx),
            "enter" | "space" => {
                if let Some(item) = self
                    .selected
                    .and_then(|i| self.items.get(i))
                    .filter(|i| !i.disabled)
                {
                    self.finish(Action::Select(item.id.clone()), window, cx);
                }
            }
            "up" | "down" | "home" | "end" => {
                let enabled: Vec<_> = self
                    .items
                    .iter()
                    .enumerate()
                    .filter_map(|(i, item)| (!item.disabled).then_some(i))
                    .collect();
                if enabled.is_empty() {
                    return;
                }
                let old = enabled
                    .iter()
                    .position(|i| Some(*i) == self.selected)
                    .unwrap_or(0);
                let index = match key.keystroke.key.as_str() {
                    "home" => 0,
                    "end" => enabled.len() - 1,
                    "up" => (old + enabled.len() - 1) % enabled.len(),
                    _ => (old + 1) % enabled.len(),
                };
                self.selected = Some(enabled[index]);
                self.scroll.scroll_to_item(enabled[index]);
                cx.notify();
            }
            _ => {}
        }
    }
    /// Color diagnostic rows with the local danger token and expose their full reason on hover.
    pub fn notices(mut self, notices: std::collections::BTreeMap<String, String>) -> Self {
        self.notices = notices;
        self
    }
}
impl Render for PopupMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.closed {
            return div().into_any_element();
        }
        let viewport = window.viewport_size();
        let width = self.width.min((viewport.width / px(1.) - 16.).max(0.));
        let height = (self.items.len() as f32 * 29. + 2. * self.style.padding_y)
            .min((viewport.height / px(1.) - 16.).max(0.));
        let left = self
            .position
            .x
            .clamp(px(8.), (viewport.width - px(width + 8.)).max(px(8.)));
        let top = self
            .position
            .y
            .clamp(px(8.), (viewport.height - px(height + 8.)).max(px(8.)));
        let mut rows = div()
            .id("menu-scroll")
            .max_h(px(height))
            .overflow_y_scroll()
            .track_scroll(&self.scroll);
        for (index, item) in self.items.iter().enumerate() {
            let id = item.id.clone();
            let debug = format!("native-menu-{}", id);
            let row = self
                .style
                .row(
                    SharedString::from(debug.clone()),
                    item.label.clone(),
                    self.selected == Some(index),
                )
                .debug_selector(move || debug.clone())
                .disabled(item.disabled)
                // Non-actionable placeholders stay readable in both themes without looking enabled.
                .when(item.disabled, |row| {
                    row.text_color(cx.theme().muted_foreground)
                })
                .when_some(self.notices.get(&item.id).cloned(), |row, reason| {
                    row.text_color(cx.theme().danger)
                        .tooltip(move |window, cx| {
                            super::Tooltip::new(reason.clone()).build(window, cx)
                        })
                })
                .when(item.separator_before, |row| {
                    row.border_t_1().border_color(self.style.border)
                })
                .on_hover(cx.listener(move |this, hovered, _, cx| {
                    if *hovered && !this.items[index].disabled {
                        this.selected = Some(index);
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.finish(Action::Select(id.clone()), window, cx)
                }))
                .child(item.label.clone());
            rows = rows.child(row);
        }
        let card = self
            .style
            .card(width)
            .id("native-popup-menu")
            .debug_selector(|| "native-popup-menu".into())
            .role(Role::Menu)
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
            .child(rows);
        deferred(
            anchored().position(point(px(0.), px(0.))).child(
                div()
                    .absolute()
                    .w(viewport.width)
                    .h(viewport.height)
                    .track_focus(&self.focus)
                    .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_up(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                    .capture_key_down(cx.listener(Self::key))
                    // A popup inside a dialog owns these actions as well as raw key events.
                    // Without this boundary, Enter selects a row and also confirms the parent modal.
                    .on_action(
                        cx.listener(|this, _: &gpui_base::actions::Confirm, window, cx| {
                            if let Some(item) = this
                                .selected
                                .and_then(|index| this.items.get(index))
                                .filter(|item| !item.disabled)
                            {
                                this.finish(Action::Select(item.id.clone()), window, cx);
                            }
                            cx.stop_propagation();
                        }),
                    )
                    .on_action(
                        cx.listener(|this, _: &gpui_base::actions::Cancel, window, cx| {
                            this.finish(Action::Dismiss, window, cx);
                            cx.stop_propagation();
                        }),
                    )
                    .on_any_mouse_down(cx.listener(|this, _, window, cx| {
                        this.finish(Action::Dismiss, window, cx);
                        cx.stop_propagation();
                    }))
                    .child(div().absolute().left(left).top(top).child(card)),
            ),
        )
        .with_priority(20)
        .into_any_element()
    }
}

/// The shell can retain focus and dismiss a superseded popup by its own entity identity.
impl gpui_kit::Focusable for PopupMenu {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl gpui_kit::EventEmitter<gpui_kit::DismissEvent> for PopupMenu {}
