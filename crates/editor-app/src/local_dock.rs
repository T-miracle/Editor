//! Project-owned presentation for the GPUI Base dock layout engine.
//!
//! The layout tree, split resizing, panel identity, drag/drop and persistence
//! remain in `gpui-base`; this module owns the editor-specific panel title
//! strip, including its configurable height.

use std::{rc::Rc, sync::Arc};

use gpui_base::dock::{DockArea, DockAreaRenderer};
use gpui_kit::{
    AnyElement, AnyView, App, AppContext as _, Context, Empty, Entity, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, StatefulInteractiveElement as _, StyleRefinement,
    Styled as _, Window,
    component::{
        ActiveTheme as _,
        dock::{DragPanel, PanelHandle, TabGroupContext, TabGroupRenderer},
        h_flex,
    },
    div,
    prelude::FluentBuilder as _,
    px, size,
};

/// A local Dock appearance with a single shared title/tab-strip height.
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
        let renderer: Rc<dyn DockAreaRenderer> = Rc::new(LocalDockRenderer {
            title_height: self.title_height,
        });
        cx.new(|cx| DockArea::new(id, version, window, cx).with_renderer(renderer))
    }
}

struct LocalDockRenderer {
    title_height: f32,
}

impl DockAreaRenderer for LocalDockRenderer {
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
            .unwrap_or_else(|| panel.panel_name(cx).into_any_element())
    }
}

impl TabGroupRenderer for LocalTabGroupRenderer {
    fn render_tab_bar(
        &self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let visible = group
            .panels()
            .iter()
            .enumerate()
            .filter(|(_, panel)| panel.visible(cx))
            .map(|(index, panel)| (index, panel.clone()))
            .collect::<Vec<_>>();

        if visible.is_empty() {
            return Empty.into_any_element();
        }

        let height = px(self.title_height);
        if visible.len() == 1 {
            let (index, panel) = &visible[0];
            if PanelHandle::of(panel).is_some_and(|handle| !handle.title_bar(cx)) {
                return Empty.into_any_element();
            }
            let drag = group.drag_panel(*index, cx);
            let title = Self::panel_title(panel, window, cx);
            let mut title_bar = h_flex()
                .id("local-dock-title-bar")
                .w_full()
                .h(height)
                .flex_shrink_0()
                .items_center()
                .px_2()
                .text_sm()
                .border_b_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().tab_bar)
                .child(title);
            if let Some(drag) = drag {
                title_bar = title_bar.on_drag(drag, move |drag, offset, _, cx| {
                    cx.stop_propagation();
                    drag.set_drag_offset(offset);
                    drag.set_preview_size(size(px(120.), height));
                    cx.new(|_| LocalDragPreview)
                });
            }
            return title_bar.into_any_element();
        }

        let active_ix = group.active_ix();
        let tabs = visible
            .into_iter()
            .map(|(index, panel)| {
                let selected = index == active_ix;
                let title = Self::panel_title(&panel, window, cx);
                let drag = group.drag_panel(index, cx);
                let mut tab = h_flex()
                    .id(("local-dock-tab", index))
                    .h_full()
                    .min_w(px(48.))
                    .max_w(px(220.))
                    .flex_shrink_0()
                    .items_center()
                    .px_2()
                    .border_r_1()
                    .border_color(cx.theme().border)
                    .when(selected, |this| this.bg(cx.theme().background))
                    .when(!selected, |this| this.bg(cx.theme().tab_bar))
                    .child(div().flex_1().min_w(px(0.)).truncate().child(title))
                    .on_click({
                        let group = group.clone();
                        move |_, window, cx| group.select_tab(index, window, cx)
                    })
                    .drag_over::<DragPanel>(|this, _, _, cx| {
                        this.border_l_2().border_color(cx.theme().drag_border)
                    })
                    .on_drop({
                        let group = group.clone();
                        move |drag: &DragPanel, window, cx| {
                            group.drop_panel(drag.clone(), Some(index), true, window, cx);
                        }
                    });
                if let Some(drag) = drag {
                    tab = tab.on_drag(drag, move |drag, offset, _, cx| {
                        cx.stop_propagation();
                        drag.set_drag_offset(offset);
                        drag.set_preview_size(size(px(120.), height));
                        cx.new(|_| LocalDragPreview)
                    });
                }
                tab
            })
            .collect::<Vec<_>>();

        h_flex()
            .id("local-dock-tab-strip")
            .w_full()
            .h(height)
            .flex_shrink_0()
            .overflow_x_scroll()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().tab_bar)
            .children(tabs)
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
}

struct LocalDragPreview;

impl Render for LocalDragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(120.))
            .h(px(28.))
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().tab_bar)
            .rounded(px(3.))
    }
}
