//! Project-owned presentation for the GPUI Base dock layout engine.
//!
//! The layout tree, split resizing, panel identity, drag/drop and persistence
//! remain in `gpui-base`; this module disables regrouping and owns the panel title
//! strip, including its configurable height. Each panel occupies its own split slot.

use std::{cell::Cell, rc::Rc, sync::Arc};

use crate::theme::component_styles;
use gpui_base::dock::{
    DockArea, DockAreaRenderer, DockContext, DockEvent, DockPlacement, InsertTarget, PanelView,
};
use gpui_base::{ElementExt as _, HandleEdge, Placement, resize_handle};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AnyView, App, AppContext as _, Bounds, Context, Div, Empty, Entity,
    InteractiveElement as _, IntoElement, MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels,
    Render, Stateful, StatefulInteractiveElement as _, StyleRefinement, Styled as _, WeakEntity,
    Window, canvas,
    component::{
        ActiveTheme as _,
        dock::{DragPanel, DropIndicator, PanelHandle, TabGroupContext, TabGroupRenderer},
        h_flex,
    },
    div, px, size,
};
use plugin_schema::ThemeComponent;

/// A local Dock appearance with independent panels and a shared title height.
#[derive(Clone, Copy)]
pub struct LocalDock {
    title_height: f32,
}

impl LocalDock {
    pub fn new(title_height: f32) -> Self {
        Self { title_height }
    }

    /// Creates a behavior-backed dock area using this project's own renderer.
    pub fn create_area(
        self,
        id: impl Into<gpui_kit::SharedString>,
        version: Option<usize>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<DockArea> {
        cx.new(|cx| {
            let renderer: Rc<dyn DockAreaRenderer> = Rc::new(LocalDockRenderer {
                title_height: self.title_height,
                resizing: Rc::new(Cell::new(None)),
                area: cx.entity().downgrade(),
            });
            DockArea::new(id, version, window, cx).with_renderer(renderer)
        })
    }
}

struct LocalDockRenderer {
    title_height: f32,
    /// The UI tracks the active handle; Base owns all size constraints and layout state.
    resizing: Rc<Cell<Option<DockPlacement>>>,
    area: WeakEntity<DockArea>,
}

impl DockAreaRenderer for LocalDockRenderer {
    /// Compose project-owned dock chrome over Base's public resize behavior.
    fn render_dock(
        &self,
        dock: &DockContext,
        content: AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        resize::render(
            dock,
            content,
            self.resizing.clone(),
            self.area.clone(),
            window,
            cx,
        )
    }

    fn tab_group_renderer(&self) -> Rc<dyn TabGroupRenderer> {
        Rc::new(LocalTabGroupRenderer {
            title_height: self.title_height,
        })
    }
}

struct LocalTabGroupRenderer {
    title_height: f32,
}

impl LocalTabGroupRenderer {
    fn panel_title(
        panel: &Arc<dyn gpui_kit::component::dock::BasePanelView>,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        PanelHandle::of(panel)
            .map(|handle| handle.title(window, cx))
            .unwrap_or_else(|| {
                // Keep dock identities stable while translating their visible fallback titles.
                let title = match panel.panel_name(cx) {
                    "Explorer" => rust_i18n::t!("panel.explorer").to_string(),
                    "Editor" => rust_i18n::t!("panel.editor").to_string(),
                    name => name.to_owned(),
                };
                title.into_any_element()
            })
    }
}

impl TabGroupRenderer for LocalTabGroupRenderer {
    /// Keep native drag events, rejecting only the central tab-merge zone.
    fn content_frame(&self, group: &TabGroupContext, _: &mut Window, _: &mut App) -> Stateful<Div> {
        let node = group.node();
        let bounds = Rc::new(Cell::new(Bounds::default()));
        let measured = bounds.clone();
        div()
            .id("local-dock-content-frame")
            .debug_selector(move || format!("local-dock-content-{}", node.as_u64()))
            .on_prepaint(move |rect, _, _| measured.set(rect))
            .can_drop(move |value, window, _| {
                // GPUI Base 0.7 uses the middle 30% on each axis for tab merging.
                // Filter that zone against the live pointer, not a previous frame's hint.
                // Base still resolves the edge, moves panels and normalizes split trees.
                let rect = bounds.get();
                let pointer = window.mouse_position();
                value.is::<DragPanel>()
                    && rect.contains(&pointer)
                    && (pointer.x < rect.left() + rect.size.width * 0.35
                        || pointer.x > rect.left() + rect.size.width * 0.65
                        || pointer.y < rect.top() + rect.size.height * 0.35
                        || pointer.y > rect.top() + rect.size.height * 0.65)
            })
    }

    fn render_tab_bar(
        &self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        // Every local layout slot contains one panel, so only its title is drawn.
        let Some(panel) = group.active_panel() else {
            return Empty.into_any_element();
        };
        if PanelHandle::of(panel).is_some_and(|handle| !handle.title_bar(cx)) {
            return Empty.into_any_element();
        }
        let style = component_styles(cx, ThemeComponent::DockTitleBar).base;
        let panel = panel.clone();
        let node = group.node();
        let drag = group.drag_panel(group.active_ix(), cx);
        h_flex()
            .id("local-dock-title-bar")
            .debug_selector(move || format!("local-dock-title-{}", node.as_u64()))
            .w_full()
            .h(px(self.title_height))
            .flex_shrink_0()
            .items_center()
            .px(px(style.padding_x_px.unwrap_or(8.)))
            .text_size(px(style.font_size_px.unwrap_or(14.)))
            .border_b_1()
            .border_color(style.border.unwrap_or(cx.theme().border))
            .bg(style.background.unwrap_or(cx.theme().tab_bar))
            .text_color(style.foreground.unwrap_or(cx.theme().foreground))
            .child(Self::panel_title(&panel, window, cx))
            .when_some(drag.filter(|_| !group.is_locked()), |bar, drag| {
                // Base owns drag identity, hit testing, split edits, and activation.
                bar.on_drag(drag, move |drag, offset, _, cx| {
                    drag.set_drag_offset(offset);
                    drag.set_preview_size(size(px(120.), px(28.)));
                    cx.stop_propagation();
                    cx.new(|_| LocalDragPreview {
                        panel: panel.clone(),
                    })
                })
            })
            .into_any_element()
    }
    fn render_active_panel(
        &self,
        panel: AnyView,
        group: &TabGroupContext,
        _: &mut Window,
        _: &mut App,
    ) -> AnyElement {
        if group.is_collapsed() {
            return Empty.into_any_element();
        }
        div()
            .id("local-dock-panel-content")
            .overflow_y_scroll()
            .overflow_x_hidden()
            .flex_1()
            .child(panel.cached(StyleRefinement::default().absolute().size_full()))
            .into_any_element()
    }
    /// Draw the half-pane resolved by Base; the center tab-merge target is omitted.
    fn render_drop_indicator(
        &self,
        indicator: DropIndicator,
        _: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        indicator.placement()?;
        let target = indicator.to();
        Some(
            div()
                .debug_selector(|| "local-dock-drop-indicator".into())
                .absolute()
                .left(target.origin().x)
                .top(target.origin().y)
                .w(target.size().width)
                .h(target.size().height)
                .bg(cx.theme().tokens.drop_target)
                .into_any_element(),
        )
    }
}

/// The title follows the pointer while the original panel retains its identity.
struct LocalDragPreview {
    panel: Arc<dyn gpui_kit::component::dock::BasePanelView>,
}

impl Render for LocalDragPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let style = component_styles(cx, ThemeComponent::DockDragPreview).base;
        h_flex()
            .w(px(120.))
            .h(px(28.))
            .px(px(style.padding_x_px.unwrap_or(8.)))
            .bg(style.background.unwrap_or(cx.theme().tab_bar))
            .text_color(style.foreground.unwrap_or(cx.theme().foreground))
            .border_1()
            .border_color(style.border.unwrap_or(cx.theme().border))
            .rounded(px(style.radius_px.unwrap_or(3.)))
            .text_size(px(style.font_size_px.unwrap_or(14.)))
            .overflow_hidden()
            .child(LocalTabGroupRenderer::panel_title(&self.panel, window, cx))
    }
}

/// Insert new panels as independent split leaves using the public DockArea API.
pub(crate) fn add_panel_view(
    area: &mut DockArea,
    panel: Arc<dyn PanelView>,
    placement: DockPlacement,
    size: Option<Pixels>,
    window: &mut Window,
    cx: &mut Context<DockArea>,
) {
    let id = panel.panel_id(cx);
    if area.panel(id).is_some() {
        return;
    }
    let neighbor = area.layout(placement).and_then(|tree| {
        tree.panels()
            .last()
            .and_then(|panel| tree.find_panel_node(panel))
    });
    // Base registers the view before it can be moved; finish both edits in one UI turn.
    area.add_panel_view(panel, placement, size, window, cx);
    if let Some(node) = neighbor {
        let placement = match placement {
            DockPlacement::Left | DockPlacement::Right => Placement::Bottom,
            _ => Placement::Right,
        };
        area.move_panel(
            id,
            InsertTarget::Split {
                node,
                placement,
                size: None,
            },
            window,
            cx,
        );
    }
}

mod resize;

#[cfg(test)]
mod tests;
