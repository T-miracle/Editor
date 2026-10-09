//! A native scrollbar projects a guest-owned range without creating a second content model.
use gpui_base::ScrollbarHandle;
use gpui_kit::{Bounds, Pixels, Point, Size, point, px, size};
use plugin_runtime::plugin_protocol::ui::ScrollRange;
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Default)]
pub(super) struct CanvasScroll(Rc<RefCell<State>>);
#[derive(Default)]
struct State {
    bounds: Bounds<Pixels>,
    range: Option<ScrollRange>,
    offset: f32,
    pending: Option<f32>,
    dragging: bool,
}
impl CanvasScroll {
    /// Drawing-only frames retain the local drag preview until the guest acknowledges a new range.
    pub(super) fn update(&self, bounds: Bounds<Pixels>, range: Option<ScrollRange>) {
        let mut state = self.0.borrow_mut();
        state.bounds = bounds;
        if state.range != range {
            state.offset = range.as_ref().map_or(0., |range| range.offset);
            state.range = range;
        }
    }
    pub(super) fn take_offset(&self) -> Option<f32> {
        self.0.borrow_mut().pending.take()
    }
    /// A visibility wrapper must keep the upstream scrollbar present while its pointer is captured.
    pub(super) fn dragging(&self) -> bool {
        self.0.borrow().dragging
    }
}
impl ScrollbarHandle for CanvasScroll {
    fn viewport_bounds(&self) -> Bounds<Pixels> {
        self.0.borrow().bounds
    }
    fn offset(&self) -> Point<Pixels> {
        point(px(0.), px(-self.0.borrow().offset))
    }
    fn content_size(&self) -> Size<Pixels> {
        let state = self.0.borrow();
        size(
            state.bounds.size.width,
            px(state.range.as_ref().map_or(0., |range| range.content)),
        )
    }
    fn set_offset(&self, offset: Point<Pixels>) {
        let mut state = self.0.borrow_mut();
        if let Some(range) = &state.range {
            let maximum = (range.content - state.bounds.size.height / px(1.)).max(0.);
            state.offset = (-offset.y / px(1.)).clamp(0., maximum);
            state.pending = Some(state.offset);
        }
    }
    fn start_drag(&self) {
        self.0.borrow_mut().dragging = true;
    }
    fn end_drag(&self) {
        self.0.borrow_mut().dragging = false;
    }
}
