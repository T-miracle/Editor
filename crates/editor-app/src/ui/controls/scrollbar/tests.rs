//! Compare the embedded editor's theme projection with the explorer/terminal scrollbar.

use super::*;
use gpui_kit::{
    Bounds, Context, IntoElement, MouseButton, Pixels, Point, Render, Size, TestAppContext, Window,
    div, gpui, point, prelude::*, size,
};
use std::{cell::Cell, rc::Rc, time::Duration};

/// Fixed content metrics isolate scrollbar paint and hit testing from text/tree layout.
#[derive(Clone)]
struct Handle {
    offset: Rc<Cell<Point<Pixels>>>,
    viewport: Bounds<Pixels>,
}

impl ScrollbarHandle for Handle {
    fn viewport_bounds(&self) -> Bounds<Pixels> {
        self.viewport
    }

    fn offset(&self) -> Point<Pixels> {
        self.offset.get()
    }

    fn content_size(&self) -> Size<Pixels> {
        size(px(1000.), px(1000.))
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        self.offset.set(offset);
    }
}

/// Embedded Base editors resolve global styles; explorer and terminal use the local control.
struct Harness {
    handle: Handle,
    local: bool,
    horizontal: bool,
    padded: bool,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bar = if self.local {
            if self.padded {
                vertical_scrollbar(&self.handle, cx)
            } else {
                vertical_viewport_scrollbar(&self.handle, cx)
            }
        } else {
            Scrollbar::vertical(&self.handle)
        };
        let bar = if self.horizontal {
            bar.axis(gpui_base::ScrollbarAxis::Horizontal)
        } else {
            bar
        };
        div().w(px(100.)).h(px(200.)).child(bar)
    }
}

/// Painted geometry, theme alpha and native dragging must agree in all three states.
#[gpui::test]
fn embedded_and_local_scrollbars_share_paint_and_drag_behavior(cx: &mut TestAppContext) {
    for dark in [false, true] {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::typography::init(cx);
            let mut theme = crate::theme::builtin_theme(dark).clone();
            let styles = theme
                .components
                .get_mut(&ThemeComponent::Scrollbar)
                .unwrap();
            styles.base.background = Some("#12345680".into());
            styles.hover.as_mut().unwrap().background = Some("#23456780".into());
            styles.active.as_mut().unwrap().background = Some("#34567880".into());
            crate::theme::apply_theme(&theme, cx);
            let scrollbar = gpui_base::Theme::global(cx).scrollbar.clone();
            assert_eq!(scrollbar.motion().idle(), Duration::from_secs(2));
        });
        for (local, padded, horizontal) in [
            (false, false, false),
            (true, false, false),
            (true, true, false),
            (false, false, true),
            (true, false, true),
            (true, true, true),
        ] {
            let along_axis = |value| {
                if horizontal {
                    point(px(value), px(0.))
                } else {
                    point(px(0.), px(value))
                }
            };
            // Explorer's tree has four pixels of padding; editor/terminal viewports reach the edge.
            let padding = if padded { 4. } else { 0. };
            let handle = Handle {
                offset: Rc::new(Cell::new(along_axis(-100.))),
                viewport: Bounds::new(
                    point(px(padding), px(padding)),
                    size(px(100. - padding * 2.), px(200. - padding * 2.)),
                ),
            };
            let input = handle.clone();
            let (_, cx) = cx.add_window_view(move |_, _| Harness {
                handle: input,
                local,
                horizontal,
                padded,
            });
            cx.update(|window, cx| {
                // Window creation drains startup theme updates. Freeze transitions afterwards,
                // so the paint assertions also verify that those updates kept our projection.
                let scrollbar = gpui_base::Theme::global(cx).scrollbar.clone();
                let motion = scrollbar
                    .motion()
                    .with_enter(Duration::ZERO)
                    .with_exit(Duration::ZERO)
                    .with_expand(Duration::ZERO);
                gpui_base::Theme::global_mut(cx).scrollbar = scrollbar.with_motion(motion);
                window.refresh();
                window.draw(cx).clear(cx);
            });
            // Movement away from the initial offset reveals the default scrolling-mode thumb.
            for (state, width, radius, rgb, opacity) in [
                (0, 6., 3., 0x123456, 0.55),
                (1, 8., 4., 0x234567, 0.7),
                (2, 8., 4., 0x345678, 0.8),
            ] {
                let center = cx.update(|window, _| {
                    let thumb = window
                        .painted_quads()
                        .into_iter()
                        .find(|quad| quad.background.as_solid().is_some_and(|color| color.a > 0.))
                        .expect("the scrolled thumb must be visible");
                    let scale = window.scale_factor();
                    let thickness = if horizontal {
                        thumb.bounds.size.height
                    } else {
                        thumb.bounds.size.width
                    };
                    assert_eq!(thickness.0 / scale, width);
                    let edge_gap = if horizontal {
                        200. - thumb.bounds.bottom().0 / scale
                    } else {
                        100. - thumb.bounds.right().0 / scale
                    };
                    assert_eq!(
                        edge_gap, 4.,
                        "all states must keep the explorer's edge clearance"
                    );
                    assert_eq!(thumb.corner_radii.top_left.0 / scale, radius);
                    let color: Hsla = gpui_kit::rgba((rgb << 8) | 0x80).into();
                    assert_eq!(thumb.background, color.opacity(opacity).into());
                    point(
                        px(thumb.bounds.center().x.0 / scale),
                        px(thumb.bounds.center().y.0 / scale),
                    )
                });
                match state {
                    0 => cx.simulate_mouse_move(center, None, Default::default()),
                    1 => cx.simulate_mouse_down(center, MouseButton::Left, Default::default()),
                    _ => {
                        let destination = center + along_axis(30.);
                        cx.simulate_mouse_move(
                            destination,
                            Some(MouseButton::Left),
                            Default::default(),
                        );
                        cx.simulate_mouse_up(destination, MouseButton::Left, Default::default());
                        assert!(
                            if horizontal {
                                handle.offset().x
                            } else {
                                handle.offset().y
                            } < px(-100.),
                            "native dragging must move content"
                        );
                    }
                }
                cx.update(|window, cx| window.draw(cx).clear(cx));
            }
        }
    }
}
