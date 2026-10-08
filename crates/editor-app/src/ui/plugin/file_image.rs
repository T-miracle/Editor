//! Draw file-authorized pixels using the sizing policy supplied by the guest.
use super::*;
use gpui_kit::component::ActiveTheme;
use gpui_kit::{
    AnyElement, Bounds, InteractiveElement, ParentElement, Styled, div, point, px, size,
};
use plugin_runtime::plugin_protocol::{api::ContentVersion, ui::ImageSizing};

impl PluginView {
    /// Native containment handles both axes; intrinsic limits prevent unrequested enlargement.
    pub(super) fn render_file_image(
        &mut self,
        id: &str,
        alt: &str,
        sizing: ImageSizing,
        viewport: Option<ui::VisualViewport>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let photo = self.photos.get(id).filter(|photo| {
            matches!(&photo.resource.source, ContentVersion::File(version) if self.document.file.as_ref() == Some(version))
        });
        if let Some(photo) = photo
            && let Ok(Some(bitmap)) = &photo.decoded
        {
            return self.render_visual_bitmap(id, bitmap.clone(), sizing, viewport, cx);
        }
        let key = super::bitmap::status_key(photo.and_then(|photo| photo.decoded.as_ref().err()));
        let label = rust_i18n::t!(key, locale = self.environment.locale.as_str());
        div()
            .debug_selector(move || format!("plugin-file-image-status-{key}").into())
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_color(cx.theme().muted_foreground)
            .child(if alt.is_empty() {
                label.to_string()
            } else {
                format!("{alt} · {label}")
            })
            .into_any_element()
    }
}

impl PluginView {
    /// Both file and document resources share native projection, clipping and versioned input.
    pub(super) fn render_visual_bitmap(
        &mut self,
        id: &str,
        bitmap: super::bitmap::Bitmap,
        sizing: ImageSizing,
        viewport: Option<ui::VisualViewport>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let measured = Rc::new(Cell::new(Bounds::default()));
        let measure = measured.clone();
        let content = ui::ContentSize {
            width: bitmap.width as f32,
            height: bitmap.height as f32,
        };
        let transform = viewport.as_ref().and_then(|v| v.transform);
        let revision = self.document.revision;
        let authority = (self.document.file.clone(), self.document.source.clone());
        let measured_authority = authority.clone();
        let node = id.to_owned();
        let measurement = self
            .visual_measurements
            .entry(node.clone())
            .or_default()
            .clone();
        let owner = cx.entity().downgrade();
        let target_node = node.clone();
        let input = viewport.is_some();
        let image = gpui_kit::canvas(
            move |bounds, _, cx| {
                measure.set(bounds);
                let area = ui::ContentSize {
                    width: bounds.size.width / px(1.),
                    height: bounds.size.height / px(1.),
                };
                if input
                    && area.width > 0.
                    && area.height > 0.
                    && measurement.get() != Some((area, content))
                {
                    let owner = owner.clone();
                    let node = target_node.clone();
                    let measurement = measurement.clone();
                    let measured_authority = measured_authority.clone();
                    // Prepaint may not update its rendering entity; publish after the frame through the scene gate.
                    cx.defer(move |cx| {
                        let accepted = owner.update(cx, |view, cx| {
                            if (view.document.file.clone(), view.document.source.clone())
                                != measured_authority
                            {
                                return false;
                            }
                            view.emit_version(
                                &node,
                                revision,
                                Action::ViewportInput(ui::ViewportInput {
                                    content,
                                    event: ui::CanvasEvent::Resize {
                                        width: area.width,
                                        height: area.height,
                                        grid: None,
                                    },
                                }),
                                cx,
                            )
                        });
                        if matches!(accepted, Ok(true)) {
                            measurement.set(Some((area, content)));
                        }
                    });
                }
                bounds
            },
            move |bounds, _, window, _| {
                let target =
                    image_target(bounds, (content.width, content.height), sizing, transform);
                // Intrinsic dimensions arrive after protocol validation; enforce the same GPU extent quota here.
                if target.size.width > px(1_000_000.) || target.size.height > px(1_000_000.) {
                    return;
                }
                let _ = window.paint_image(
                    bounds,
                    target,
                    Default::default(),
                    bitmap.image.clone(),
                    0,
                    false,
                );
            },
        )
        .size_full();
        let emit = Rc::new(
            move |view: &mut PluginView, event: ui::CanvasEvent, cx: &mut Context<PluginView>| {
                input
                    && (view.document.file.clone(), view.document.source.clone()) == authority
                    && view.emit_version(
                        &node,
                        revision,
                        Action::ViewportInput(ui::ViewportInput { content, event }),
                        cx,
                    )
            },
        );
        let wheel = emit.clone();
        let wheel_bounds = measured.clone();
        let mut element = div()
            .debug_selector(|| "plugin-file-image".into())
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .child(image)
            .on_scroll_wheel(cx.listener(
                move |view, event: &gpui_kit::ScrollWheelEvent, _, cx| {
                    let bounds = wheel_bounds.get();
                    let local = event.position - bounds.origin;
                    let delta = event.delta.pixel_delta(px(view
                        .environment
                        .ui_font
                        .size_px
                        .unwrap_or(14.)
                        .max(1.)));
                    // Forward raw logical pixels; the guest selects direction, step, limits and anchor.
                    if wheel(
                        view,
                        ui::CanvasEvent::Wheel {
                            x: local.x / px(1.),
                            y: local.y / px(1.),
                            delta_x: delta.x / px(1.),
                            delta_y: delta.y / px(1.),
                            shift: event.modifiers.shift,
                        },
                        cx,
                    ) {
                        cx.stop_propagation();
                    }
                },
            ));
        // Input mechanisms are shared with canvas clients; no native pan or zoom policy consumes them.
        for (button, index) in [
            (gpui_kit::MouseButton::Left, 0),
            (gpui_kit::MouseButton::Middle, 1),
            (gpui_kit::MouseButton::Right, 2),
        ] {
            let down = emit.clone();
            let down_bounds = measured.clone();
            element = element.on_mouse_down(
                button,
                cx.listener(move |view, event: &gpui_kit::MouseDownEvent, _, cx| {
                    let p = event.position - down_bounds.get().origin;
                    if down(
                        view,
                        ui::CanvasEvent::Pointer {
                            phase: ui::PointerPhase::Down,
                            x: p.x / px(1.),
                            y: p.y / px(1.),
                            button: index,
                            clicks: event.click_count as u8,
                            shift: event.modifiers.shift,
                        },
                        cx,
                    ) {
                        cx.stop_propagation();
                    }
                }),
            );
            let up = emit.clone();
            let up_bounds = measured.clone();
            element = element.on_mouse_up(
                button,
                cx.listener(move |view, event: &gpui_kit::MouseUpEvent, _, cx| {
                    let p = event.position - up_bounds.get().origin;
                    if up(
                        view,
                        ui::CanvasEvent::Pointer {
                            phase: ui::PointerPhase::Up,
                            x: p.x / px(1.),
                            y: p.y / px(1.),
                            button: index,
                            clicks: event.click_count as u8,
                            shift: event.modifiers.shift,
                        },
                        cx,
                    ) {
                        cx.stop_propagation();
                    }
                }),
            );
        }
        element
            .on_mouse_move(
                cx.listener(move |view, event: &gpui_kit::MouseMoveEvent, _, cx| {
                    let p = event.position - measured.get().origin;
                    let button = match event.pressed_button {
                        Some(gpui_kit::MouseButton::Middle) => 1,
                        Some(gpui_kit::MouseButton::Right) => 2,
                        _ => 0,
                    };
                    emit(
                        view,
                        ui::CanvasEvent::Pointer {
                            phase: ui::PointerPhase::Move,
                            x: p.x / px(1.),
                            y: p.y / px(1.),
                            button,
                            clicks: 0,
                            shift: event.modifiers.shift,
                        },
                        cx,
                    );
                }),
            )
            .into_any_element()
    }
}

/// Paint the exact current native viewport, retaining center and aspect ratio even when zoom clips pixels.
fn image_target(
    bounds: Bounds<gpui_kit::Pixels>,
    intrinsic: (f32, f32),
    sizing: ImageSizing,
    transform: Option<ui::ContentTransform>,
) -> Bounds<gpui_kit::Pixels> {
    if let Some(transform) = transform {
        let rect = transform.project(
            plugin_runtime::plugin_protocol::Rect {
                x: 0.,
                y: 0.,
                w: intrinsic.0,
                h: intrinsic.1,
            },
            ui::ContentSize {
                width: intrinsic.0,
                height: intrinsic.1,
            },
            ui::ContentSize {
                width: bounds.size.width / px(1.),
                height: bounds.size.height / px(1.),
            },
        );
        return Bounds::new(
            bounds.origin + point(px(rect.x), px(rect.y)),
            size(px(rect.w), px(rect.h)),
        );
    }
    let (width, height) = {
        contained_size(
            intrinsic.0,
            intrinsic.1,
            bounds.size.width / px(1.),
            bounds.size.height / px(1.),
            sizing,
        )
    };
    Bounds::new(
        bounds.origin
            + point(
                (bounds.size.width - px(width)) / 2.,
                (bounds.size.height - px(height)) / 2.,
            ),
        size(px(width), px(height)),
    )
}

/// Containment preserves aspect ratio on both axes; only an explicit Contain policy may enlarge.
fn contained_size(
    width: f32,
    height: f32,
    available_width: f32,
    available_height: f32,
    sizing: ImageSizing,
) -> (f32, f32) {
    let scale = (available_width.max(0.) / width).min(available_height.max(0.) / height);
    let scale = if sizing == ImageSizing::OriginalContain {
        scale.min(1.)
    } else {
        scale
    };
    (width * scale, height * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Native input publishes geometry and wheel deltas; only a guest declaration changes the scale.
    #[gpui_kit::gpui::test]
    fn file_image_viewport_forwards_versioned_wheel_and_resize(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::{AppContext as _, component::Root};
        use plugin_runtime::plugin_protocol::{
            api::FileVersion,
            ui::{Document, Kind, Node},
        };
        use std::{cell::RefCell, sync::Arc};
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::ui::typography::init(cx);
            crate::ui::theme::apply_theme(crate::ui::theme::builtin_theme(false), cx);
        });
        let version = FileVersion {
            id: "picture".into(),
            path: "picture.png".into(),
            revision: 1,
        };
        let mut document = Document::new(
            Node::new(
                "image",
                Kind::FileImage {
                    alt: String::new(),
                    sizing: ImageSizing::OriginalContain,
                },
            )
            .grow(),
        );
        document.file = Some(version.clone());
        document.root.viewport = Some(ui::VisualViewport::default());
        let replacement = document.clone();
        let events = Rc::new(RefCell::new(Vec::new()));
        let inputs = events.clone();
        let slot = Rc::new(RefCell::new(None));
        let capture = slot.clone();
        let (_, visual) = cx.add_window_view(move |window, cx| {
            let view = cx.new(|cx| {
                PluginView::new(
                    "other-image-provider".into(),
                    document,
                    Environment::default(),
                    move |event, _| inputs.borrow_mut().push(event),
                    window,
                    cx,
                )
            });
            // Feed the same immutable, version-authorized decoded publication used by production.
            view.update(cx, |view, _| {
                view.photos.insert(
                    "image".into(),
                    Arc::new(super::super::images::Photo {
                        resource: Arc::new(plugin_runtime::ImageResource {
                            source: ContentVersion::File(version),
                            uri: "picture.png".into(),
                            state: plugin_runtime::ImageState::Ready(Arc::new(Vec::new())),
                        }),
                        decoded: Ok(Some(super::super::bitmap::Bitmap {
                            width: 100,
                            height: 50,
                            image: Arc::new(gpui_kit::RenderImage::new([image::Frame::new(
                                image::RgbaImage::from_pixel(
                                    100,
                                    50,
                                    image::Rgba([255, 0, 0, 255]),
                                ),
                            )])),
                        })),
                    }),
                );
            });
            *capture.borrow_mut() = Some(view.clone());
            Root::new(view, window, cx)
        });
        let view = slot.borrow_mut().take().unwrap();
        visual.simulate_resize(size(px(400.), px(300.)));
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let bounds = visual.debug_bounds("plugin-file-image").unwrap();
        assert!(events.borrow().iter().any(|event| matches!(&event.action, Action::ViewportInput(input)
            if input.content == (ui::ContentSize {width: 100., height: 50.}) && matches!(input.event, ui::CanvasEvent::Resize {width: 400., height: 300., ..}))));
        events.borrow_mut().clear();
        for delta in [14., -14., 14.] {
            visual.simulate_event(gpui_kit::ScrollWheelEvent {
                // Off-center input must not use the pointer as its zoom anchor.
                position: bounds.origin + point(px(10.), px(10.)),
                delta: gpui_kit::ScrollDelta::Pixels(point(px(0.), px(delta))),
                ..Default::default()
            });
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert!(events.borrow().iter().any(|event| matches!(&event.action,
                Action::ViewportInput(input) if matches!(input.event, ui::CanvasEvent::Wheel {delta_y, ..} if delta_y == delta))));
            events.borrow_mut().clear();
        }
        visual.simulate_resize(size(px(260.), px(220.)));
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let bounds = visual.debug_bounds("plugin-file-image").unwrap();
        assert!(events.borrow().iter().any(|event| matches!(&event.action,
            Action::ViewportInput(input) if matches!(input.event, ui::CanvasEvent::Resize {width: 260., height: 220., ..}))));
        events.borrow_mut().clear();
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                let mut next = replacement;
                next.file.as_mut().unwrap().id = "another-picture".into();
                view.update_document(next, Environment::default(), window, cx);
            })
        });
        // File authority changed: retained callbacks cannot publish measurements for the old resource.
        visual.simulate_event(gpui_kit::ScrollWheelEvent {
            position: bounds.center(),
            delta: gpui_kit::ScrollDelta::Pixels(point(px(0.), px(14.))),
            ..Default::default()
        });
        assert!(events.borrow().is_empty());
    }

    /// Boundary fixtures cover no enlargement, exact fit and containment on each independent axis.
    #[test]
    fn file_image_paint_geometry_preserves_intrinsic_ratio_and_fits_both_axes() {
        for (image, area, expected) in [
            ((100., 50.), (400., 300.), (100., 50.)),
            ((100., 50.), (100., 50.), (100., 50.)),
            ((800., 200.), (400., 300.), (400., 100.)),
            ((200., 800.), (400., 300.), (75., 300.)),
            ((800., 600.), (400., 300.), (400., 300.)),
            ((100., 50.), (40., 300.), (40., 20.)),
            ((100., 50.), (400., 300.), (100., 50.)),
        ] {
            assert_eq!(
                contained_size(
                    image.0,
                    image.1,
                    area.0,
                    area.1,
                    ImageSizing::OriginalContain
                ),
                expected
            );
        }
    }
}
