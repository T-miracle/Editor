//! Project-owned dock handles; gpui-base remains responsible for resize constraints.

use super::*;

/// A resize gesture needs a payload but no floating preview.
#[derive(Clone)]
struct ResizeDock;

impl Render for ResizeDock {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// Track moves across the whole window, as a pointer can leave the narrow handle.
pub(super) fn render(
    dock: &DockContext,
    content: AnyElement,
    resizing: Rc<Cell<Option<DockPlacement>>>,
    area: WeakEntity<DockArea>,
    _: &mut Window,
    _: &mut App,
) -> AnyElement {
    let placement = dock.placement();
    let id = match placement {
        DockPlacement::Left => "local-resize-left",
        DockPlacement::Right => "local-resize-right",
        DockPlacement::Bottom => "local-resize-bottom",
        DockPlacement::Center => "local-resize-center",
    };
    let edge = if placement == DockPlacement::Left {
        HandleEdge::Trailing
    } else {
        HandleEdge::Leading
    };
    let started = resizing.clone();
    let handle = resize_handle(id, placement.axis())
        .inside(edge)
        .with_appearance(Rc::new(|handle, _, cx| {
            // The project supplies paint; Base supplies hit area, cursor and dragging.
            let style = component_styles(cx, ThemeComponent::DockTitleBar).base;
            let line = div().bg(style.border.unwrap_or(cx.theme().border));
            Some(if handle.axis() == gpui_kit::Axis::Horizontal {
                line.w(px(1.)).h_full().into_any_element()
            } else {
                line.h(px(1.)).w_full().into_any_element()
            })
        }))
        .on_drag(ResizeDock, move |_, _, _, cx| {
            cx.stop_propagation();
            started.set(Some(placement));
            cx.new(|_| ResizeDock)
        });
    let dock = dock.clone();
    let tracker = canvas(
        |_, _, _| (),
        move |_, _, window, _| {
            let moving = resizing.clone();
            let moved_dock = dock.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if phase.bubble() && moving.get() == Some(placement) {
                    moved_dock.resize_to(event.position, window, cx);
                }
            });
            let finished = resizing.clone();
            let finished_dock = dock.clone();
            let area = area.clone();
            window.on_mouse_event(move |_: &MouseUpEvent, phase, window, cx| {
                if phase.bubble() && finished.get() == Some(placement) {
                    finished.set(None);
                    finished_dock.end_resize(window, cx);
                    // Persist the final extent through the existing host subscription.
                    let _ = area.update(cx, |_, cx| cx.emit(DockEvent::LayoutChanged));
                }
            });
        },
    )
    .absolute()
    .size_full();
    div()
        .flex()
        .size_full()
        .relative()
        .child(content)
        .child(handle)
        .child(tracker)
        .into_any_element()
}
