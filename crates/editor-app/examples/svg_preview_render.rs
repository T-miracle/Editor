//! Validate the real WASM drawing output with the same native vector renderer as the editor.

#[path = "../src/ui/plugin/images.rs"]
mod images;

use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, Paint, Rect, api, ui},
};
use std::{collections::BTreeMap, path::Path, sync::Arc};

/// Inspect the ordinary native document tree used by the production renderer.
fn drawing(scene: &ui::Document) -> &ui::Canvas {
    let ui::Kind::Canvas(canvas) = &scene.root.kind else {
        panic!("canvas required")
    };
    canvas
}
fn send(manager: &mut Manager, event: api::Notification) -> anyhow::Result<()> {
    manager.event("svg", Some("preview".into()), event)
}

/// Locate document rasters separately from toolbar SVGs through their public viewport clip.
fn document_index(scene: &ui::Document) -> usize {
    drawing(scene)
        .paint
        .iter()
        .position(|operation| matches!(operation, Paint::Svg { clip, .. } if clip.y > 0.))
        .expect("document SVG raster")
}

/// Render the whole published scene for visual QA, using native vector images for icons and text.
fn render_panel(scene: &ui::Document, renderer: &mut images::VectorRenderer) -> image::RgbaImage {
    let mut scene = scene.clone();
    // Text uses the same font database as the SVG renderer for this standalone diagnostic PNG.
    let ui::Kind::Canvas(canvas) = &mut scene.root.kind else {
        panic!("canvas required")
    };
    let family_default = canvas
        .font
        .family
        .clone()
        .unwrap_or_else(|| "Segoe UI".into());
    for operation in &mut canvas.paint {
        if let Paint::Text {
            x,
            y,
            text,
            color,
            size,
            font,
            ..
        } = operation
        {
            let escaped = text
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            let family = font
                .as_deref()
                .unwrap_or(&family_default)
                .replace('"', "&quot;");
            let rect = Rect {
                x: *x,
                y: *y,
                w: (600. - *x).max(1.),
                h: *size * 1.5,
            };
            *operation = Paint::Svg {
                rect,
                clip: Rect {
                    x: 0.,
                    y: 0.,
                    w: 600.,
                    h: 600.,
                },
                source: format!(
                    "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\"><text x=\"0\" y=\"{size}\" font-family=\"{family}\" font-size=\"{size}\" fill=\"#{color:06x}\">{escaped}</text></svg>",
                    rect.w, rect.h
                ),
            };
        }
    }
    let rendered = renderer.prepare(&BTreeMap::from([("panel".into(), Arc::new(scene.clone()))]));
    let mut output = image::RgbaImage::new(1200, 1200);
    for (index, operation) in drawing(&scene).paint.iter().enumerate() {
        match operation {
            Paint::Fill { rect, color, .. } => {
                let color =
                    image::Rgba([(*color >> 16) as u8, (*color >> 8) as u8, *color as u8, 255]);
                for y in ((rect.y * 2.).max(0.) as u32).min(1200)
                    ..(((rect.y + rect.h) * 2.).ceil().max(0.) as u32).min(1200)
                {
                    for x in ((rect.x * 2.).max(0.) as u32).min(1200)
                        ..(((rect.x + rect.w) * 2.).ceil().max(0.) as u32).min(1200)
                    {
                        output.put_pixel(x, y, color);
                    }
                }
            }
            Paint::Svg { .. } => {
                if let Some(vector) = &rendered["panel/canvas/preview-canvas"][index] {
                    let dimensions = vector.image.size(0);
                    let mut pixels = vector.image.as_bytes(0).unwrap().to_vec();
                    // Native rasters contain straight BGRA; image compositing expects RGBA.
                    for pixel in pixels.chunks_exact_mut(4) {
                        pixel.swap(0, 2);
                    }
                    let raster = image::RgbaImage::from_raw(
                        dimensions.width.0 as u32,
                        dimensions.height.0 as u32,
                        pixels,
                    )
                    .unwrap();
                    image::imageops::overlay(
                        &mut output,
                        &raster,
                        (vector.rect.x * 2.).round() as i64,
                        (vector.rect.y * 2.).round() as i64,
                    );
                }
            }
            Paint::Text { .. } => unreachable!("diagnostic text was converted above"),
        }
    }
    output
}

/// Produce a standalone board-and-SVG image while checking native BGRA color and transparency.
fn main() -> anyhow::Result<()> {
    let arguments = std::env::args().collect::<Vec<_>>();
    let package = Package::read(Path::new(arguments.get(1).expect("svg.zip")))?;
    let source = std::fs::read_to_string(arguments.get(2).expect("gear.svg fixture"))?;
    let output = Path::new(arguments.get(3).expect("output.png"));
    let directory = tempfile::tempdir()?;
    // Supply a realistic dark panel theme; zero-valued protocol defaults would hide black icons.
    let mut manager = Manager::open(
        directory.path().into(),
        Environment {
            background: 0x202026,
            foreground: 0xc9cbd0,
            border: 0x35353b,
            muted: 0x2f2f35,
            muted_foreground: 0x8d9098,
            ..Default::default()
        },
    )?;
    manager.install(&package, package.manifest.permissions.clone())?;
    send(
        &mut manager,
        api::Notification::Ui(ui::UiEvent {
            revision: 0,
            node: "preview-canvas".into(),
            action: ui::Action::Canvas(ui::CanvasEvent::Resize {
                width: 600.,
                height: 600.,
                grid: None,
            }),
        }),
    )?;
    send(
        &mut manager,
        api::Notification::Preview {
            document: Some(api::DocumentVersion {
                id: "gear".into(),
                path: "gear.svg".into(),
                revision: 1,
            }),
            text: source,
        },
    )?;
    let scene = manager.live["svg"].views["preview"].clone();
    let scenes = BTreeMap::from([("preview".into(), scene.clone())]);
    let mut renderer = images::VectorRenderer::default();
    let rendered = renderer.prepare(&scenes);
    // Each asset must produce actual nontransparent pixels through the native SVG rasterizer.
    for (index, operation) in drawing(&scene).paint.iter().enumerate() {
        if matches!(operation, Paint::Svg { clip, .. } if clip.y == 0.) {
            let icon = rendered["preview/canvas/preview-canvas"][index]
                .as_ref()
                .expect("toolbar icon raster");
            assert!(
                icon.image
                    .as_bytes(0)
                    .unwrap()
                    .chunks_exact(4)
                    .any(|pixel| pixel[3] > 0)
            );
        }
    }
    let vector = rendered["preview/canvas/preview-canvas"][document_index(&scene)]
        .as_ref()
        .expect("native SVG raster");
    let dimensions = vector.image.size(0);
    let width = dimensions.width.0 as u32;
    let height = dimensions.height.0 as u32;
    let pixels = vector.image.as_bytes(0).unwrap();
    let density = width as f32 / vector.rect.w;
    // This fixture has a transparent outer margin and center hole, plus an opaque colored tooth.
    let pixel = |intrinsic_x: f32, intrinsic_y: f32| {
        // Probe known points of the 480px fixture after its new 240px default scaling.
        let x = intrinsic_x / 480. * vector.rect.w;
        let y = intrinsic_y / 480. * vector.rect.h;
        let offset = ((y * density) as usize * width as usize + (x * density) as usize) * 4;
        &pixels[offset..offset + 4]
    };
    assert_eq!(pixel(10., 10.)[3], 0);
    assert_eq!(pixel(240., 245.)[3], 0);
    assert_eq!(pixel(240., 100.), &[0x80, 0x71, 0x66, 255]);
    let mut composite = image::RgbaImage::new(width, height);
    for operation in &drawing(&scene).paint {
        if let Paint::Fill { rect, color, .. } = operation {
            // Project the guest's actual board operations into the cropped SVG raster bounds.
            let left = ((rect.x - vector.rect.x) * density).floor().max(0.) as u32;
            let top = ((rect.y - vector.rect.y) * density).floor().max(0.) as u32;
            let right = ((rect.x + rect.w - vector.rect.x) * density).ceil().max(0.) as u32;
            let bottom = ((rect.y + rect.h - vector.rect.y) * density).ceil().max(0.) as u32;
            let color = image::Rgba([(*color >> 16) as u8, (*color >> 8) as u8, *color as u8, 255]);
            for y in top.min(height)..bottom.min(height) {
                for x in left.min(width)..right.min(width) {
                    composite.put_pixel(x, y, color);
                }
            }
        }
    }
    for (target, source) in composite.pixels_mut().zip(pixels.chunks_exact(4)) {
        let alpha = u32::from(source[3]);
        for channel in 0..3 {
            target[channel] = ((u32::from(source[2 - channel]) * alpha
                + u32::from(target[channel]) * (255 - alpha)
                + 127)
                / 255) as u8;
        }
    }
    composite.save(output)?;
    let panel_output = output.with_file_name(format!(
        "{}-panel.png",
        output.file_stem().unwrap().to_string_lossy()
    ));
    render_panel(&scene, &mut renderer).save(&panel_output)?;
    // A second real document checks partial alpha, which requires unpremultiplication for GPUI.
    send(&mut manager, api::Notification::Preview {
        document: Some(api::DocumentVersion { id: "alpha".into(), path: "alpha.svg".into(), revision: 1 }),
        text: "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"><rect width=\"10\" height=\"10\" fill=\"red\" fill-opacity=\"0.5\"/></svg>".into(),
    })?;
    let alpha_scene = manager.live["svg"].views["preview"].clone();
    let rendered = renderer.prepare(&BTreeMap::from([("preview".into(), alpha_scene.clone())]));
    let vector = rendered["preview/canvas/preview-canvas"][document_index(&alpha_scene)]
        .as_ref()
        .unwrap();
    assert_eq!(&vector.image.as_bytes(0).unwrap()[..4], &[0, 0, 255, 128]);
    println!(
        "PASS: real WASM + native raster color, holes, partial alpha, checkerboard layers; {}",
        output.display()
    );
    Ok(())
}
