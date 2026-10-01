//! Specifications use the exported guest interface and its observable drawing output.

use super::*;

/// Send the exact protocol used by the native host, without bypassing plugin dispatch.
fn dispatch(message: Message) -> Scene {
    let payload = SvgPreview::dispatch(serde_json::to_string(&message).unwrap()).unwrap();
    serde_json::from_str::<Reply>(&payload)
        .unwrap()
        .scene
        .unwrap()
}

/// Surface identity travels with resize, document and wheel events.
fn event(event: Event) -> Scene {
    dispatch(Message::Event(Event::Surface {
        panel: "preview".into(),
        event: Box::new(event),
    }))
}

/// Read the vector operation through the public drawing protocol.
fn vector(scene: &Scene) -> (Rect, &str) {
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

/// Transparent vectors sit above the board, and off-center wheel input keeps the image centered.
#[test]
fn document_renders_above_checkerboard_and_stays_centered_during_zoom() {
    dispatch(Message::Prepare {
        environment: Environment::default(),
        snapshot: None,
    });
    event(Event::Resize {
        width: 400.,
        height: 300.,
        cell_width: 8.,
        cell_height: 20.,
    });
    let source = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200\" height=\"100\"><circle cx=\"100\" cy=\"50\" r=\"40\" fill=\"#667180\"/></svg>";
    let scene = event(Event::Document {
        path: Some("sample.svg".into()),
        text: source.into(),
    });
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
    let zoomed = event(Event::Wheel {
        delta: 1.,
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
    let zoomed = event(Event::Wheel {
        delta: -2.,
        shift: false,
        x: 390.,
        y: 290.,
    });
    let rect = vector(&zoomed).0;
    assert!((rect.x + rect.w / 2. - 200.).abs() < 0.01);
    assert!((rect.y + rect.h / 2. - 166.).abs() < 0.01);
    for id in ["zoom-in", "zoom-out", "actual-size", "fit"] {
        let changed = event(Event::Command {
            id: id.into(),
            arguments: None,
            cwd: None,
            text: None,
        });
        let rect = vector(&changed).0;
        assert!((rect.x + rect.w / 2. - 200.).abs() < 0.01);
        assert!((rect.y + rect.h / 2. - 166.).abs() < 0.01);
    }
    // A manual zoom must also recenter when the native split resizes the preview canvas.
    event(Event::Command {
        id: "zoom-in".into(),
        arguments: None,
        cwd: None,
        text: None,
    });
    let resized = event(Event::Resize {
        width: 600.,
        height: 400.,
        cell_width: 8.,
        cell_height: 20.,
    });
    let rect = vector(&resized).0;
    assert!((rect.x + rect.w / 2. - 300.).abs() < 0.01);
    assert!((rect.y + rect.h / 2. - 216.).abs() < 0.01);
}

/// Large but valid vectors must stay renderable instead of exceeding the host's geometry quota.
#[test]
fn huge_svg_zoom_stays_inside_protocol_geometry_limits() {
    dispatch(Message::Prepare {
        environment: Environment::default(),
        snapshot: None,
    });
    let scene = event(Event::Document {
        path: Some("huge.svg".into()),
        text: "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1000000\" height=\"1000000\"/>"
            .into(),
    });
    assert!(vector(&scene).0.w <= 1_000_000.);
    let scene = event(Event::Wheel {
        delta: 100.,
        shift: false,
        x: 200.,
        y: 150.,
    });
    let rect = vector(&scene).0;
    assert!(rect.w <= 1_000_000. && rect.h <= 1_000_000.);
    assert!(rect.x.abs() <= 1_000_000. && rect.y.abs() <= 1_000_000.);
}

/// Default previews keep their longest edge at 240 logical pixels across viewport resizes.
#[test]
fn default_preview_uses_240_pixels_and_preserves_aspect_ratio() {
    dispatch(Message::Prepare {
        environment: Environment::default(),
        snapshot: None,
    });
    let scene = event(Event::Document {
        path: Some("portrait.svg".into()),
        text: "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"300\" height=\"600\"/>".into(),
    });
    let rect = vector(&scene).0;
    assert_eq!((rect.w, rect.h), (120., 240.));
    let resized = event(Event::Resize {
        width: 900.,
        height: 700.,
        cell_width: 8.,
        cell_height: 20.,
    });
    let rect = vector(&resized).0;
    assert_eq!((rect.w, rect.h), (120., 240.));
}

/// The four visible SVG icons behave like clicks, and window fitting follows later resizes.
#[test]
fn svg_toolbar_controls_zoom_and_places_percentage_at_right() {
    dispatch(Message::Prepare {
        environment: Environment::default(),
        snapshot: None,
    });
    let scene = event(Event::Document {
        path: Some("toolbar.svg".into()),
        text: "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200\" height=\"100\"/>".into(),
    });
    assert!(scene.widgets.is_empty(), "replace the old text buttons");
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
    let unchanged = event(Event::Pointer {
        kind: "up".into(),
        x: icons[0].x + 10.,
        y: icons[0].y + 10.,
        button: 0,
        clicks: 1,
        shift: false,
    });
    assert!((vector(&unchanged).0.w - 240.).abs() < 0.01);
    for (index, expected) in [(0, 268.8), (1, 240.), (2, 200.), (3, 352.)] {
        let rect = icons[index];
        event(Event::Pointer {
            kind: "down".into(),
            x: rect.x + 10.,
            y: rect.y + 10.,
            button: 0,
            clicks: 1,
            shift: false,
        });
        let clicked = event(Event::Pointer {
            kind: "up".into(),
            x: rect.x + 10.,
            y: rect.y + 10.,
            button: 0,
            clicks: 1,
            shift: false,
        });
        assert!((vector(&clicked).0.w - expected).abs() < 0.01);
    }
    let resized = event(Event::Resize {
        width: 800.,
        height: 600.,
        cell_width: 8.,
        cell_height: 20.,
    });
    assert_eq!(vector(&resized).0.w, 752.);
}

/// Percentage text follows the editor UI family, size and weight, including live theme updates.
#[test]
fn percentage_text_inherits_editor_default_typography() {
    let environment = Environment {
        ui_font: FontStyle {
            family: Some("Segoe UI".into()),
            size_px: Some(18.),
            bold: Some(true),
        },
        ..Default::default()
    };
    dispatch(Message::Prepare {
        environment,
        snapshot: None,
    });
    let scene = event(Event::Document {
        path: Some("font.svg".into()),
        text: "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"480\" height=\"480\"/>".into(),
    });
    assert!(scene.paint.iter().any(|operation| matches!(operation,
        Paint::Text { text, size, bold, font, .. } if text == "50%" && *size == 18. && *bold
            && font.as_deref().unwrap_or(&scene.font) == "Segoe UI"
    )));
    let updated = event(Event::Theme(Environment {
        ui_font: FontStyle {
            family: Some("Arial".into()),
            size_px: Some(16.),
            bold: Some(false),
        },
        ..Default::default()
    }));
    assert!(updated.paint.iter().any(|operation| matches!(operation,
        Paint::Text { text, size, bold, font, .. } if text == "50%" && *size == 16. && !*bold
            && font.as_deref().unwrap_or(&updated.font) == "Arial"
    )));
}
