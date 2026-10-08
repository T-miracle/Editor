//! Specifications exercise the current SDK dispatch and observable canvas output.
use super::*;

/// Raster zoom policy is supplied through the same exported dispatch as a third-party viewer.
#[test]
fn raster_viewport_input_owns_zoom_and_preserves_center_across_resize() {
    prepare(Environment::default());
    let send = |event| {
        let message = api::Invocation {
            id: 2,
            message: api::Input::Event {
                panel: Some("preview".into()),
                event,
            },
        };
        let payload = ImagePreview::dispatch(serde_json::to_string(&message).unwrap()).unwrap();
        let reply: api::Completion = serde_json::from_str(&payload).unwrap();
        let document = reply.result.unwrap().views.remove(0).document;
        document.validate().unwrap();
        document
    };
    let file = api::FileContext {
        version: api::FileVersion {
            id: "photo".into(),
            path: "photo.png".into(),
            revision: 1,
        },
        file_type: "png".into(),
        text: None,
    };
    let mut document = send(api::Notification::FilePreview {
        file: Some(file.clone()),
    });
    let input = |revision, event| {
        api::Notification::Ui(ui::UiEvent {
            revision,
            node: "image".into(),
            action: ui::Action::ViewportInput(ui::ViewportInput {
                content: ui::ContentSize {
                    width: 800.,
                    height: 400.,
                },
                event,
            }),
        })
    };
    document = send(input(
        document.revision,
        ui::CanvasEvent::Resize {
            width: 400.,
            height: 300.,
            grid: None,
        },
    ));
    assert_eq!(
        document
            .root
            .viewport
            .as_ref()
            .unwrap()
            .transform
            .unwrap()
            .scale,
        0.5
    );
    document = send(input(
        document.revision,
        ui::CanvasEvent::Wheel {
            x: 10.,
            y: 10.,
            delta_x: 0.,
            delta_y: 14.,
            shift: false,
        },
    ));
    let t = document.root.viewport.as_ref().unwrap().transform.unwrap();
    assert!((t.scale - 0.56).abs() < 0.0001);
    assert_eq!((t.anchor_x, t.anchor_y, t.x, t.y), (0.5, 0.5, 0., 0.));
    document = send(input(
        document.revision,
        ui::CanvasEvent::Resize {
            width: 600.,
            height: 500.,
            grid: None,
        },
    ));
    assert_eq!(document.root.viewport.as_ref().unwrap().transform, Some(t));
    // A retained event cannot modify the replacement scene, and a new file restores automatic sizing.
    document = send(input(
        0,
        ui::CanvasEvent::Wheel {
            x: 10.,
            y: 10.,
            delta_x: 0.,
            delta_y: 14.,
            shift: false,
        },
    ));
    assert_eq!(document.root.viewport.as_ref().unwrap().transform, Some(t));
    let mut replacement = file;
    // Wheel deltas already queued for one unchanged file/node identity must all reach the policy.
    let queued_revision = document.revision;
    for _ in 0..3 {
        document = send(input(
            queued_revision,
            ui::CanvasEvent::Wheel {
                x: 10.,
                y: 10.,
                delta_x: 0.,
                delta_y: 14.,
                shift: false,
            },
        ));
    }
    assert!(
        (document
            .root
            .viewport
            .as_ref()
            .unwrap()
            .transform
            .unwrap()
            .scale
            - 0.7867597)
            .abs()
            < 0.0001
    );
    replacement.version.id = "another-photo".into();
    document = send(api::Notification::FilePreview {
        file: Some(replacement),
    });
    assert!(document.root.viewport.unwrap().transform.is_none());
}

/// Decode exactly the result envelope exported by the SDK.
fn dispatch(message: api::Input) -> ui::Canvas {
    let payload =
        ImagePreview::dispatch(serde_json::to_string(&api::Invocation { id: 1, message }).unwrap())
            .unwrap();
    let result: api::Completion = serde_json::from_str(&payload).unwrap();
    assert_eq!(result.id, 1);
    let output = result.result.unwrap();
    output.views[0].document.validate().unwrap();
    let ui::Kind::Canvas(canvas) = output.views.into_iter().next().unwrap().document.root.kind
    else {
        panic!("canvas required")
    };
    canvas
}
fn prepare(environment: Environment) {
    let api = serde_json::from_value(serde_json::json!({"base":"1.0.0","capabilities":{"ui.native":"1.0.0","ui.canvas":"1.1.0","editor.documents":"1.0.0"}})).unwrap();
    dispatch(api::Input::Prepare {
        environment,
        api,
        snapshot: None,
    });
}
fn event(event: api::Notification) -> ui::Canvas {
    dispatch(api::Input::Event {
        panel: Some("preview".into()),
        event,
    })
}
fn preview(path: &str, text: String) -> ui::Canvas {
    event(api::Notification::Preview {
        document: Some(api::DocumentVersion {
            id: path.into(),
            path: path.into(),
            revision: 1,
        }),
        text,
    })
}
fn canvas(action: ui::CanvasEvent) -> ui::Canvas {
    let revision = STATE.with(|state| state.borrow().revision);
    event(api::Notification::Ui(ui::UiEvent {
        revision,
        node: "preview-canvas".into(),
        action: ui::Action::Canvas(action),
    }))
}
/// Toolbar commands are driven by real pointer pairs, not an undeclared command backdoor.
fn command(id: String) -> ui::Canvas {
    let index = TOOLBAR_ICONS
        .iter()
        .position(|(name, _)| *name == id)
        .unwrap();
    let rect = STATE.with(|state| state.borrow().toolbar_button_rect(index));
    let mut result = ui::Canvas::default();
    for phase in [ui::PointerPhase::Down, ui::PointerPhase::Up] {
        result = canvas(ui::CanvasEvent::Pointer {
            phase,
            x: rect.x + rect.w / 2.,
            y: rect.y + rect.h / 2.,
            button: 0,
            clicks: 1,
            shift: false,
        });
    }
    result
}

/// Read the vector operation through the public drawing protocol.
fn vector(scene: &ui::Canvas) -> (Rect, &str) {
    scene
        .paint
        .iter()
        // Toolbar vectors precede the document; the final vector is the preview itself.
        .rev()
        .find_map(|paint| {
            if let Paint::Svg { rect, source, .. } = paint {
                Some((*rect, source.as_str()))
            } else {
                None
            }
        })
        .expect("a valid document must produce a vector image")
}

/// A small SVG opens at a 240px longest edge while preserving its aspect ratio and center.
#[test]
fn default_preview_enlarges_small_svg_to_a_240_pixel_longest_edge() {
    prepare(Environment::default());
    canvas(ui::CanvasEvent::Resize {
        width: 400.,
        height: 300.,
        grid: None,
    });
    let scene = preview(
        "small.svg",
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"50\"/>".into(),
    );
    let (rect, _) = vector(&scene);
    assert!((rect.w - 240.).abs() < 0.01 && (rect.h - 120.).abs() < 0.01);
    assert!((rect.x - 80.).abs() < 0.01 && (rect.y - 106.).abs() < 0.01);
    // A viewport smaller than the preferred minimum still contains the complete SVG.
    let narrow = canvas(ui::CanvasEvent::Resize {
        width: 120.,
        height: 100.,
        grid: None,
    });
    let rect = vector(&narrow).0;
    assert!((rect.w - 88.).abs() < 0.01 && (rect.h - 44.).abs() < 0.01);
    let restored = canvas(ui::CanvasEvent::Resize {
        width: 400.,
        height: 300.,
        grid: None,
    });
    assert!((vector(&restored).0.w - 240.).abs() < 0.01);
    command("actual-size".into());
    let manual = canvas(ui::CanvasEvent::Resize {
        width: 600.,
        height: 400.,
        grid: None,
    });
    assert!(
        (vector(&manual).0.w - 100.).abs() < 0.01,
        "manual 1:1 ignores the initial minimum"
    );
}

/// Transparent vectors sit above the board, and off-center wheel input keeps the image centered.
#[test]
fn document_renders_above_checkerboard_and_stays_centered_during_zoom() {
    prepare(Environment::default());
    canvas(ui::CanvasEvent::Resize {
        width: 400.,
        height: 300.,
        grid: None,
    });
    let source = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200\" height=\"100\"><circle cx=\"100\" cy=\"50\" r=\"40\" fill=\"#667180\"/></svg>";
    let scene = preview("sample.svg", source.into());
    let (rect, rendered) = vector(&scene);
    assert_eq!(rendered, source);
    // Intrinsic-to-logical scaling uses f32, so compare the visible geometry within a subpixel.
    assert!((rect.x - 80.).abs() < 0.01);
    assert!((rect.y - 106.).abs() < 0.01);
    assert!((rect.w - 240.).abs() < 0.01);
    assert!((rect.h - 120.).abs() < 0.01);
    assert!(matches!(scene.paint.last(), Some(Paint::Svg { .. })));
    assert!(scene.paint.iter().any(|paint| matches!(
        paint,
        Paint::Fill {
            color: 0xbfbfbf,
            ..
        }
    )));
    let zoomed = canvas(ui::CanvasEvent::Wheel {
        delta_x: 0.,
        delta_y: 1. * 14.,
        shift: false,
        x: 120.,
        y: 126.,
    });
    let (rect, _) = vector(&zoomed);
    assert!((rect.w - 268.8).abs() < 0.01);
    assert!((rect.h - 134.4).abs() < 0.01);
    assert!((rect.x + rect.w / 2. - 200.).abs() < 0.01);
    assert!((rect.y + rect.h / 2. - 166.).abs() < 0.01);
    // An opposite-corner wheel event and every toolbar command must retain the same center.
    let zoomed = canvas(ui::CanvasEvent::Wheel {
        delta_x: 0.,
        delta_y: -2. * 14.,
        shift: false,
        x: 390.,
        y: 290.,
    });
    let rect = vector(&zoomed).0;
    assert!((rect.x + rect.w / 2. - 200.).abs() < 0.01);
    assert!((rect.y + rect.h / 2. - 166.).abs() < 0.01);
    for id in ["zoom-in", "zoom-out", "actual-size", "fit"] {
        let changed = command(id.into());
        let rect = vector(&changed).0;
        assert!((rect.x + rect.w / 2. - 200.).abs() < 0.01);
        assert!((rect.y + rect.h / 2. - 166.).abs() < 0.01);
    }
    // A manual zoom must also recenter when the native split resizes the preview canvas.
    command("zoom-in".into());
    let resized = canvas(ui::CanvasEvent::Resize {
        width: 600.,
        height: 400.,
        grid: None,
    });
    let rect = vector(&resized).0;
    assert!((rect.x + rect.w / 2. - 300.).abs() < 0.01);
    assert!((rect.y + rect.h / 2. - 216.).abs() < 0.01);
}

/// Large but valid vectors must stay renderable instead of exceeding the host's geometry quota.
#[test]
fn huge_svg_zoom_stays_inside_protocol_geometry_limits() {
    prepare(Environment::default());
    let scene = preview(
        "huge.svg",
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1000000\" height=\"1000000\"/>".into(),
    );
    assert!(vector(&scene).0.w <= 1_000_000.);
    let scene = canvas(ui::CanvasEvent::Wheel {
        delta_x: 0.,
        delta_y: 100. * 14.,
        shift: false,
        x: 200.,
        y: 150.,
    });
    let rect = vector(&scene).0;
    assert!(rect.w <= 1_000_000. && rect.h <= 1_000_000.);
    assert!(rect.x.abs() <= 1_000_000. && rect.y.abs() <= 1_000_000.);
}

/// Automatic fitting follows viewport changes and stops growing at the original image size.
#[test]
fn automatic_preview_preserves_ratio_and_restores_intrinsic_size_after_resize() {
    prepare(Environment::default());
    let scene = preview(
        "portrait.svg",
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"300\" height=\"600\"/>".into(),
    );
    let rect = vector(&scene).0;
    assert_eq!((rect.w, rect.h), (134., 268.));
    let resized = canvas(ui::CanvasEvent::Resize {
        width: 900.,
        height: 700.,
        grid: None,
    });
    let rect = vector(&resized).0;
    assert_eq!((rect.w, rect.h), (300., 600.));
}

/// The four visible SVG icons behave like clicks, and window fitting follows later resizes.
#[test]
fn svg_toolbar_controls_zoom_and_places_percentage_at_right() {
    prepare(Environment::default());
    let scene = preview(
        "toolbar.svg",
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200\" height=\"100\"/>".into(),
    );
    let icons = scene
        .paint
        .iter()
        .filter_map(|operation| {
            if let Paint::Svg { rect, source, .. } = operation {
                if source.contains("<title>") {
                    usvg::Tree::from_str(source, &usvg::Options::default()).unwrap();
                    return Some(*rect);
                }
            }
            None
        })
        .collect::<Vec<_>>();
    assert_eq!(
        icons.len(),
        4,
        "all four toolbar controls use real SVG assets"
    );
    assert!(scene.paint.iter().any(|operation| matches!(operation,
        Paint::Text { x, y, text, .. } if text == "120%" && *x > 300. && *y < 32.
    )));
    // An unmatched release must not activate a button or change the current image size.
    let unchanged = canvas(ui::CanvasEvent::Pointer {
        phase: ui::PointerPhase::Up,
        x: icons[0].x + 10.,
        y: icons[0].y + 10.,
        button: 0,
        clicks: 1,
        shift: false,
    });
    assert!((vector(&unchanged).0.w - 240.).abs() < 0.01);
    for (index, expected) in [(0, 268.8), (1, 240.), (2, 200.), (3, 352.)] {
        let rect = icons[index];
        canvas(ui::CanvasEvent::Pointer {
            phase: ui::PointerPhase::Down,
            x: rect.x + 10.,
            y: rect.y + 10.,
            button: 0,
            clicks: 1,
            shift: false,
        });
        let clicked = canvas(ui::CanvasEvent::Pointer {
            phase: ui::PointerPhase::Up,
            x: rect.x + 10.,
            y: rect.y + 10.,
            button: 0,
            clicks: 1,
            shift: false,
        });
        assert!((vector(&clicked).0.w - expected).abs() < 0.01);
    }
    let resized = canvas(ui::CanvasEvent::Resize {
        width: 800.,
        height: 600.,
        grid: None,
    });
    assert_eq!(vector(&resized).0.w, 752.);
}

/// Percentage text follows the editor UI family, size and weight, including live theme updates.
#[test]
fn percentage_text_inherits_editor_default_typography() {
    let environment = Environment {
        ui_font: plugin_protocol::FontStyle {
            family: Some("Segoe UI".into()),
            size_px: Some(18.),
            bold: Some(true),
        },
        ..Default::default()
    };
    prepare(environment);
    let scene = preview(
        "font.svg",
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"480\" height=\"480\"/>".into(),
    );
    assert!(scene.paint.iter().any(|operation| matches!(operation,
        Paint::Text { text, size, bold, font, .. } if text == "56%" && *size == 18. && *bold
            && font.as_deref().or(scene.font.family.as_deref()).unwrap() == "Segoe UI"
    )));
    let updated = event(api::Notification::Theme(Environment {
        ui_font: plugin_protocol::FontStyle {
            family: Some("Arial".into()),
            size_px: Some(16.),
            bold: Some(false),
        },
        ..Default::default()
    }));
    assert!(updated.paint.iter().any(|operation| matches!(operation,
        Paint::Text { text, size, bold, font, .. } if text == "56%" && *size == 16. && !*bold
            && font.as_deref().or(updated.font.family.as_deref()).unwrap() == "Arial"
    )));
}
