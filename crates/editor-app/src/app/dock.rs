//! Project-owned presentation for the GPUI Base dock layout engine.
//!
//! The layout tree, split resizing, panel identity, drag/drop and persistence
//! remain in `gpui-base`; this module owns the editor-specific panel title
//! strip, including its configurable height.

use std::{rc::Rc, sync::Arc};

use crate::theme::component_styles;
use gpui_base::dock::{DockArea, DockAreaRenderer, DockContext};
use gpui_kit::{
    AnyElement, AnyView, App, AppContext as _, Context, Empty, Entity, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, StatefulInteractiveElement as _, StyleRefinement,
    Styled as _, Window,
    component::{
        ActiveTheme as _,
        dock::{
            DockSkin, DragPanel, DropIndicator, PanelHandle, TabGroupContext, TabGroupRenderer,
        },
        h_flex,
    },
    div,
    prelude::FluentBuilder as _,
    px, size,
};
use plugin_schema::ThemeComponent;

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
        cx.new(|cx| {
            let renderer: Rc<dyn DockAreaRenderer> = Rc::new(LocalDockRenderer {
                title_height: self.title_height,
                chrome: DockSkin::new(cx),
            });
            DockArea::new(id, version, window, cx).with_renderer(renderer)
        })
    }
}

struct LocalDockRenderer {
    title_height: f32,
    /// Retains the standard dock edge handles and their window-wide drag tracking.
    chrome: Rc<DockSkin>,
}

impl DockAreaRenderer for LocalDockRenderer {
    /// Reuse native dock resizing while retaining the editor's custom title/tab strips.
    fn render_dock(
        &self,
        dock: &DockContext,
        content: AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        self.chrome.render_dock(dock, content, window, cx)
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
        let title_style = component_styles(cx, ThemeComponent::DockTitleBar).base;
        let tab_styles = component_styles(cx, ThemeComponent::DockTab);
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
                .px(px(title_style.padding_x_px.unwrap_or(8.)))
                .text_size(px(title_style.font_size_px.unwrap_or(14.)))
                .border_b_1()
                .border_color(title_style.border.unwrap_or(cx.theme().border))
                .bg(title_style.background.unwrap_or(cx.theme().tab_bar))
                .text_color(title_style.foreground.unwrap_or(cx.theme().foreground))
                .child(title);
            if let Some(drag) = drag {
                let panel = panel.clone();
                title_bar = title_bar.on_drag(drag, move |drag, offset, _, cx| {
                    cx.stop_propagation();
                    drag.set_drag_offset(offset);
                    drag.set_preview_size(size(px(120.), height));
                    cx.new(|_| LocalDragPreview {
                        panel: panel.clone(),
                    })
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
                    .px(px(tab_styles.base.padding_x_px.unwrap_or(8.)))
                    .text_size(px(tab_styles.base.font_size_px.unwrap_or(14.)))
                    .border_r_1()
                    .border_color(tab_styles.base.border.unwrap_or(cx.theme().border))
                    .hover(|style| {
                        style
                            .bg(tab_styles.hover.background.unwrap_or(
                                tab_styles.base.background.unwrap_or(cx.theme().tab_bar),
                            ))
                            .text_color(tab_styles.hover.foreground.unwrap_or(
                                tab_styles.base.foreground.unwrap_or(cx.theme().foreground),
                            ))
                    })
                    .text_color(if selected {
                        tab_styles
                            .selected
                            .foreground
                            .unwrap_or(cx.theme().foreground)
                    } else {
                        tab_styles
                            .base
                            .foreground
                            .unwrap_or(cx.theme().tab_foreground)
                    })
                    .when(selected, |this| {
                        this.bg(tab_styles
                            .selected
                            .background
                            .unwrap_or(cx.theme().background))
                    })
                    .when(!selected, |this| {
                        this.bg(tab_styles.base.background.unwrap_or(cx.theme().tab_bar))
                    })
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
                    let panel = panel.clone();
                    tab = tab.on_drag(drag, move |drag, offset, _, cx| {
                        cx.stop_propagation();
                        drag.set_drag_offset(offset);
                        drag.set_preview_size(size(px(120.), height));
                        cx.new(|_| LocalDragPreview {
                            panel: panel.clone(),
                        })
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
            .border_color(title_style.border.unwrap_or(cx.theme().border))
            .bg(title_style.background.unwrap_or(cx.theme().tab_bar))
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

    fn render_drop_indicator(
        &self,
        indicator: DropIndicator,
        _: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let target = indicator.to();
        Some(
            div()
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

struct LocalDragPreview {
    panel: Arc<dyn gpui_kit::component::dock::BasePanelView>,
}

impl Render for LocalDragPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let style = component_styles(cx, ThemeComponent::DockDragPreview).base;
        div()
            .w(px(120.))
            .h(px(28.))
            .flex()
            .items_center()
            .overflow_hidden()
            .whitespace_nowrap()
            .px(px(style.padding_x_px.unwrap_or(8.)))
            .border_1()
            .border_color(style.border.unwrap_or(cx.theme().border))
            .bg(style.background.unwrap_or(cx.theme().tab_bar))
            .rounded(px(style.radius_px.unwrap_or(3.)))
            .text_size(px(style.font_size_px.unwrap_or(14.)))
            .text_color(style.foreground.unwrap_or(cx.theme().foreground))
            .child(LocalTabGroupRenderer::panel_title(&self.panel, window, cx))
    }
}
