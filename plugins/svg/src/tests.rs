//! Specifications exercise the current SDK dispatch and observable canvas output.
use super::*;

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

/// Opening a small image preserves its intrinsic dimensions instead of enlarging it.
#[test]
fn default_preview_preserves_small_intrinsic_image_dimensions() {
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
    assert_eq!((rect.w, rect.h), (100., 50.));
    assert_eq!((rect.x, rect.y), (150., 141.));
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
    assert!((rect.x - 100.).abs() < 0.01);
    assert!((rect.y - 116.).abs() < 0.01);
    assert!((rect.w - 200.).abs() < 0.01);
    assert!((rect.h - 100.).abs() < 0.01);
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
    assert!((rect.w - 224.).abs() < 0.01);
    assert!((rect.h - 112.).abs() < 0.01);
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
        Paint::Text { x, y, text, .. } if text == "100%" && *x > 300. && *y < 32.
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
    assert!((vector(&unchanged).0.w - 200.).abs() < 0.01);
    for (index, expected) in [(0, 224.), (1, 200.), (2, 200.), (3, 352.)] {
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
