//! Place measured definition content beside its text anchor in window coordinates.

use gpui_kit::{
    AnyElement, App, AvailableSpace, Bounds, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, Size, Window, deferred, div, point, prelude::*, px, size,
};
use std::{cell::RefCell, rc::Rc};

/// Details sit above ordinary dock popovers and below popup menus (priority 20).
const DEFINITION_POPUP_PRIORITY: usize = 10;

/// A popover whose measured height determines which side of the text has room.
pub(crate) struct DefinitionPopup {
    anchor: Bounds<Pixels>,
    content: Option<AnyElement>,
    handles: Option<AnyElement>,
}

/// Place content using the text anchor's window-local coordinate space.
pub(crate) fn definition_popup(anchor: Bounds<Pixels>, content: AnyElement) -> impl IntoElement {
    // Defer both hit testing and painting so sibling panels and dock clipping
    // cannot cover a card positioned in the window's coordinate space.
    deferred(DefinitionPopup {
        anchor,
        content: Some(content),
        handles: None,
    })
    .with_priority(DEFINITION_POPUP_PRIORITY)
}

/// Keep measured placement and retained resize state together for this frame.
pub(crate) struct DefinitionPopupLayout {
    origin: Point<Pixels>,
    size: Size<Pixels>,
    resize: Rc<RefCell<PopupResizeState>>,
}

/// Resize from the outer vertical edge and the right edge of the card.
#[derive(Clone, Copy)]
enum ResizeEdge {
    Right,
    Top,
    Bottom,
}

/// Keep the edge beside the text fixed while resizing away from it.
#[derive(Clone, Copy)]
enum PopupSide {
    Above,
    Below,
}

impl PopupSide {
    /// Return the room on this side without crossing the hovered text.
    fn available_height(self, above: Pixels, below: Pixels) -> Pixels {
        match self {
            Self::Above => above,
            Self::Below => below,
        }
    }
}

/// Resize from the original press position so redraws never compound the delta.
#[derive(Clone, Copy)]
struct ResizeDrag {
    edge: ResizeEdge,
    start: Point<Pixels>,
    size: Size<Pixels>,
    minimum: Size<Pixels>,
}

/// Retain a chosen size across card redraws, with limits from the current viewport.
#[derive(Default)]
struct PopupResizeState {
    bounds: Bounds<Pixels>,
    size: Option<Size<Pixels>>,
    maximum: Size<Pixels>,
    drag: Option<ResizeDrag>,
    side: Option<PopupSide>,
}

impl PopupResizeState {
    /// Start a border drag without making short, naturally sized cards jump larger.
    fn begin_resize(&mut self, edge: ResizeEdge, position: Point<Pixels>) {
        self.drag = Some(ResizeDrag {
            edge,
            start: position,
            size: self.bounds.size,
            minimum: size(
                self.bounds.size.width.min(px(160.)),
                self.bounds.size.height.min(px(80.)),
            ),
        });
    }

    /// Consume captured movement even outside the card, and cancel a lost release.
    fn resize_to(&mut self, position: Point<Pixels>, button: Option<MouseButton>) -> bool {
        let Some(drag) = self.drag else {
            return false;
        };
        if button != Some(MouseButton::Left) {
            self.drag = None;
            return true;
        }
        let delta = position - drag.start;
        let mut requested = drag.size;
        match drag.edge {
            ResizeEdge::Right => requested.width += delta.x,
            // Above the text, pulling the outer edge upward increases height.
            ResizeEdge::Top => requested.height -= delta.y,
            ResizeEdge::Bottom => requested.height += delta.y,
        }
        self.size = Some(size(
            requested.width.clamp(
                drag.minimum.width.min(self.maximum.width),
                self.maximum.width,
            ),
            requested.height.clamp(
                drag.minimum.height.min(self.maximum.height),
                self.maximum.height,
            ),
        ));
        true
    }
}

/// Put a narrow drag target over the border while leaving the text area selectable.
fn resize_handle(edge: ResizeEdge, resize: Rc<RefCell<PopupResizeState>>) -> AnyElement {
    let id = match edge {
        ResizeEdge::Right => "editor-definition-resize-right",
        ResizeEdge::Top => "editor-definition-resize-top",
        ResizeEdge::Bottom => "editor-definition-resize-bottom",
    };
    let handle = div()
        .id(id)
        .debug_selector(move || id.into())
        .absolute()
        .occlude()
        .on_mouse_down(MouseButton::Left, move |event, _, cx| {
            resize.borrow_mut().begin_resize(edge, event.position);
            cx.stop_propagation();
        });
    match edge {
        ResizeEdge::Right => handle
            .right_0()
            .top_0()
            .w(px(6.))
            .h_full()
            .cursor_col_resize(),
        ResizeEdge::Top => handle
            .top_0()
            .left_0()
            .h(px(6.))
            .w_full()
            .cursor_row_resize(),
        ResizeEdge::Bottom => handle
            .bottom_0()
            .left_0()
            .h(px(6.))
            .w_full()
            .cursor_row_resize(),
    }
    .into_any_element()
}

impl IntoElement for DefinitionPopup {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for DefinitionPopup {
    type RequestLayoutState = DefinitionPopupLayout;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some("definition-popup".into())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let resize = window.with_element_state(
            global_id.expect("DefinitionPopup has a stable element id"),
            |retained: Option<Rc<RefCell<PopupResizeState>>>, _| {
                let retained = retained.unwrap_or_default();
                (retained.clone(), retained)
            },
        );
        // Text geometry is local to the content area. Desktop window bounds
        // include its screen origin, which changes when the left edge is resized.
        let viewport = Bounds::new(point(px(0.), px(0.)), window.viewport_size());
        let margin = px(8.);
        let above = (self.anchor.top() - viewport.top() - margin).max(px(0.));
        let below = (viewport.bottom() - self.anchor.bottom() - margin).max(px(0.));
        let (maximum, requested, preferred_side) = {
            let mut state = resize.borrow_mut();
            // During a drag, only use room on the current side so growing
            // the card cannot flip its position or change the grabbed edge.
            let max_height = state
                .side
                .filter(|_| state.drag.is_some())
                .map(|side| side.available_height(above, below))
                .unwrap_or(above.max(below));
            let maximum = size((viewport.size.width - margin * 2.).max(px(0.)), max_height);
            // A window shrink must also constrain a previously chosen card size.
            state.size = state.size.map(|size| {
                gpui_kit::size(
                    size.width.min(maximum.width),
                    size.height.min(maximum.height),
                )
            });
            (
                maximum,
                state.size,
                state.side.filter(|_| state.size.is_some()),
            )
        };
        let content = self
            .content
            .take()
            .expect("definition content is laid out once");
        let mut frame = div()
            .id("definition-popup-frame")
            .relative()
            .flex()
            .flex_col()
            .max_w(if requested.is_some() {
                maximum.width
            } else {
                maximum.width.min(px(500.))
            })
            .max_h(if requested.is_some() {
                maximum.height
            } else {
                maximum.height.min(px(320.))
            })
            .child(content)
            .when_some(requested, |frame, size| frame.w(size.width).h(size.height))
            .into_any_element();
        // Measure preferred content within the frame's limits; min-content
        // would collapse a shrinkable Markdown child to its padding alone.
        let size = frame.layout_as_root(
            size(AvailableSpace::MaxContent, AvailableSpace::MaxContent),
            window,
            cx,
        );
        self.content = Some(frame);
        // Retain a resized card's side while it fits, including after release.
        // New cards prefer below; lack of space moves the card above the text.
        let side = preferred_side
            .filter(|side| side.available_height(above, below) >= size.height)
            .unwrap_or_else(|| {
                if below >= size.height || below >= above && above < size.height {
                    PopupSide::Below
                } else {
                    PopupSide::Above
                }
            });
        {
            let mut state = resize.borrow_mut();
            state.side = Some(side);
            state.maximum = gpui_kit::size(maximum.width, side.available_height(above, below));
        }
        // Placement is known only after measuring content. Lay out handles
        // separately so the correct outer edge is active in this same frame.
        let vertical_edge = match side {
            PopupSide::Above => ResizeEdge::Top,
            PopupSide::Below => ResizeEdge::Bottom,
        };
        let mut handles = div()
            .id("definition-popup-resize-handles")
            .relative()
            .w(size.width)
            .h(size.height)
            .child(resize_handle(ResizeEdge::Right, resize.clone()))
            .child(resize_handle(vertical_edge, resize.clone()))
            .into_any_element();
        handles.layout_as_root(AvailableSpace::min_size(), window, cx);
        self.handles = Some(handles);
        let y = match side {
            PopupSide::Below => self.anchor.bottom(),
            PopupSide::Above => self.anchor.top() - size.height,
        };
        let x = self
            .anchor
            .left()
            .max(viewport.left() + margin)
            .min((viewport.right() - size.width - margin).max(viewport.left() + margin));
        let layout = div().into_any_element().request_layout(window, cx);
        (
            layout,
            DefinitionPopupLayout {
                origin: point(x, y),
                size,
                resize,
            },
        )
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        // The content uses window coordinates, independent of the editor's scroll frame.
        layout.resize.borrow_mut().bounds = Bounds::new(layout.origin, layout.size);
        self.content
            .as_mut()
            .unwrap()
            .prepaint_at(layout.origin, window, cx);
        self.handles
            .as_mut()
            .unwrap()
            .prepaint_at(layout.origin, window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.content.as_mut().unwrap().paint(window, cx);
        self.handles.as_mut().unwrap().paint(window, cx);
        // Capture after a border press so dragging beyond the card cannot
        // become editor hover, text selection, or a lost mouse release.
        let moved = layout.resize.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase.capture()
                && moved
                    .borrow_mut()
                    .resize_to(event.position, event.pressed_button)
            {
                window.refresh();
                cx.stop_propagation();
            }
        });
        let released = layout.resize.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase.capture() && event.button == MouseButton::Left {
                let mut state = released.borrow_mut();
                if state.resize_to(event.position, Some(MouseButton::Left)) {
                    state.drag = None;
                    window.refresh();
                    cx.stop_propagation();
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Context, MouseButton, Render, TestAppContext, deferred, gpui, rgb, size};
    use std::{cell::Cell, rc::Rc};

    /// Exercise an upper card that could fit below after the user shrinks it.
    struct UpperResizablePopup;

    impl Render for UpperResizablePopup {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            definition_popup(
                Bounds::new(point(px(40.), px(340.)), size(px(80.), px(20.))),
                div()
                    .id("upper-resizable-card")
                    .debug_selector(|| "upper-resizable-card".into())
                    .w(px(300.))
                    .h(px(320.))
                    .min_h_0()
                    .flex_grow_1()
                    .flex_shrink_1()
                    .occlude()
                    .into_any_element(),
            )
        }
    }

    /// Resizing must keep the edge beside the text fixed, even after release.
    #[gpui::test]
    fn upper_resize_retains_placement_and_stays_in_viewport(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| UpperResizablePopup);
        cx.simulate_resize(size(px(500.), px(600.)));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let initial = cx.debug_bounds("upper-resizable-card").unwrap();
        assert_eq!(initial.bottom(), px(340.));
        assert!(cx.debug_bounds("editor-definition-resize-bottom").is_none());
        let start = cx
            .debug_bounds("editor-definition-resize-top")
            .unwrap()
            .center();
        let end = start + point(px(0.), px(120.));
        cx.simulate_mouse_down(start, MouseButton::Left, Default::default());
        cx.simulate_mouse_move(end, MouseButton::Left, Default::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.simulate_mouse_up(end, MouseButton::Left, Default::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let shorter = cx.debug_bounds("upper-resizable-card").unwrap();
        assert_eq!(shorter.size.height, px(200.));
        assert_eq!(shorter.bottom(), initial.bottom());
        assert!(cx.debug_bounds("editor-definition-resize-top").is_some());
        assert!(cx.debug_bounds("editor-definition-resize-bottom").is_none());
        // Pull beyond the viewport; the card must stop at its upper margin.
        let start = cx
            .debug_bounds("editor-definition-resize-top")
            .unwrap()
            .center();
        let end = start - point(px(0.), px(3000.));
        cx.simulate_mouse_down(start, MouseButton::Left, Default::default());
        cx.simulate_mouse_move(end, MouseButton::Left, Default::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.simulate_mouse_up(end, MouseButton::Left, Default::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let taller = cx.debug_bounds("upper-resizable-card").unwrap();
        assert_eq!(taller.top(), px(8.));
        assert_eq!(taller.bottom(), initial.bottom());
    }

    /// Stand in for a clipped dock panel beside another ordinary floating panel.
    struct CoveredPopup {
        detail_clicks: Rc<Cell<usize>>,
        panel_clicks: Rc<Cell<usize>>,
    }

    impl Render for CoveredPopup {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let detail_clicks = self.detail_clicks.clone();
            let panel_clicks = self.panel_clicks.clone();
            div()
                .relative()
                .size_full()
                .child(
                    div()
                        .w(px(100.))
                        .h(px(100.))
                        .overflow_hidden()
                        .child(definition_popup(
                            Bounds::new(point(px(40.), px(80.)), size(px(80.), px(20.))),
                            div()
                                .id("raised-details")
                                .debug_selector(|| "raised-details".into())
                                .w(px(160.))
                                .h(px(70.))
                                .bg(rgb(0xffffff))
                                .occlude()
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    detail_clicks.set(detail_clicks.get() + 1);
                                    cx.stop_propagation();
                                })
                                .into_any_element(),
                        )),
                )
                .child(deferred(
                    div()
                        .id("ordinary-floating-panel")
                        .absolute()
                        .left(px(40.))
                        .top(px(100.))
                        .w(px(160.))
                        .h(px(70.))
                        .bg(rgb(0x808080))
                        .occlude()
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            panel_clicks.set(panel_clicks.get() + 1);
                            cx.stop_propagation();
                        }),
                ))
        }
    }

    /// Details must receive clicks above sibling panels and outside dock clipping.
    #[gpui::test]
    fn details_are_above_floating_panels(cx: &mut TestAppContext) {
        let detail_clicks = Rc::new(Cell::new(0));
        let panel_clicks = Rc::new(Cell::new(0));
        let details = detail_clicks.clone();
        let panels = panel_clicks.clone();
        let (_, cx) = cx.add_window_view(move |_, _| CoveredPopup {
            detail_clicks: details,
            panel_clicks: panels,
        });
        cx.simulate_resize(size(px(400.), px(300.)));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let card = cx.debug_bounds("raised-details").unwrap();
        assert!(
            card.top() >= px(100.),
            "details must extend outside the panel"
        );
        // Inspect the actual scene as well as hit testing: details must be
        // painted after the overlapping panel, not just receive its clicks.
        let (detail_order, panel_order) = cx.update(|window, _| {
            let quads = window.painted_quads();
            let details = quads
                .iter()
                .find(|quad| quad.background == rgb(0xffffff).into())
                .expect("details must be painted");
            let panel = quads
                .iter()
                .find(|quad| quad.background == rgb(0x808080).into())
                .expect("the overlapping panel must be painted");
            (details.order, panel.order)
        });
        assert!(detail_order > panel_order, "details must be visibly on top");
        cx.simulate_click(card.center(), Default::default());
        assert_eq!(
            detail_clicks.get(),
            1,
            "details must own the topmost hitbox"
        );
        assert_eq!(
            panel_clicks.get(),
            0,
            "the covered panel must not receive clicks"
        );
    }
}
