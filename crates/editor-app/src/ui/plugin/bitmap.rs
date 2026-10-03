//! Decode bounded, authorized bytes on the plugin worker, never through GPUI's ambient URL loader.

use gpui_kit::RenderImage;
use plugin_runtime::plugin_protocol::api::{ErrorCode, Failure};
use std::io::Cursor;
use std::sync::Arc;

/// A decoded image carries its intrinsic layout size alongside the retained native pixels.
#[derive(Clone)]
pub(crate) struct Bitmap {
    pub image: Arc<RenderImage>,
    pub width: u32,
    pub height: u32,
}

/// Content detection accepts the documented formats; invalid or oversized bytes fail one image only.
#[cfg(test)]
pub(super) fn decode(bytes: &[u8]) -> Result<Bitmap, Failure> {
    decode_with_budget(bytes, 64 * 1024 * 1024)
}

/// Remaining resident pixel capacity is checked by the decoder before allocating its output buffer.
pub(super) fn decode_with_budget(bytes: &[u8], remaining: u64) -> Result<Bitmap, Failure> {
    let mut buffer = if let Ok(format) = image::guess_format(bytes) {
        if !matches!(
            format,
            image::ImageFormat::Png
                | image::ImageFormat::Jpeg
                | image::ImageFormat::Gif
                | image::ImageFormat::WebP
        ) {
            return Err(Failure::new(
                ErrorCode::UnsupportedOperation,
                "Unsupported image encoding",
            ));
        }
        let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(4096);
        limits.max_image_height = Some(4096);
        limits.max_alloc = Some(remaining.min(64 * 1024 * 1024));
        reader.limits(limits);
        // GIF/WebP use their first frame. This bounded decode does not allocate an animation frame stack.
        let decoded = reader.decode().map_err(|error| match error {
            image::ImageError::Limits(_) => limited(),
            _ => invalid(),
        })?;
        // The output is always RGBA, even when a JPEG's decoder accounts only for three channels.
        if u64::from(decoded.width()) * u64::from(decoded.height()) * 4 > remaining {
            return Err(limited());
        }
        decoded.into_rgba8()
    } else {
        svg(bytes, remaining)?
    };
    let (width, height) = buffer.dimensions();
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return Err(Failure::new(
            ErrorCode::LimitExceeded,
            "Image dimensions exceed 4096 pixels",
        ));
    }
    // GPUI's direct RenderImage source expects straight BGRA, as in its public image decoder.
    for pixel in buffer.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Ok(Bitmap {
        image: Arc::new(RenderImage::new([image::Frame::new(buffer)])),
        width,
        height,
    })
}

/// SVG shares resvg's native renderer while blocking every ambient file or network resolver.
fn svg(bytes: &[u8], remaining: u64) -> Result<image::RgbaImage, Failure> {
    let options = resvg::usvg::Options {
        fontdb: super::svg::fonts(),
        image_href_resolver: resvg::usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    // UTF-8 XML only: from_data would implicitly inflate SVGZ before any allocation quota.
    let source = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let tree = super::svg::parse(source, &options)?;
    let size = tree.size().to_int_size();
    let (width, height) = (size.width(), size.height());
    if width > 4096 || height > 4096 || u64::from(width) * u64::from(height) * 4 > remaining {
        return Err(limited());
    }
    super::svg::render_budget(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        width,
        height,
        remaining,
    )?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).ok_or_else(invalid)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    let mut pixels = pixmap.take();
    // tiny-skia emits premultiplied RGBA; the shared conversion below then changes it to straight BGRA.
    for pixel in pixels.chunks_exact_mut(4) {
        let alpha = u32::from(pixel[3]);
        if alpha > 0 {
            for channel in &mut pixel[..3] {
                *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    image::RgbaImage::from_raw(width, height, pixels).ok_or_else(invalid)
}

/// Decoder internals and local paths are not user-facing error text or authority.
fn invalid() -> Failure {
    Failure::new(
        ErrorCode::OperationFailed,
        "Image cannot be decoded or exceeds the allocation limit",
    )
}

/// This typed failure can be retried after other image resources leave the resident cache.
fn limited() -> Failure {
    Failure::new(
        ErrorCode::LimitExceeded,
        "Image dimensions or native image cache quota exceeded",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
    use std::io::Cursor;

    /// The declared set is checked by encoded content, including formats without an alpha channel.
    #[test]
    fn documented_images_decode_with_intrinsic_dimensions() {
        for format in [
            ImageFormat::Png,
            ImageFormat::Jpeg,
            ImageFormat::Gif,
            ImageFormat::WebP,
        ] {
            let image =
                DynamicImage::ImageRgba8(RgbaImage::from_pixel(30, 20, Rgba([23, 100, 200, 255])));
            let image = if format == ImageFormat::Jpeg {
                DynamicImage::ImageRgb8(image.to_rgb8())
            } else {
                image
            };
            let mut bytes = Cursor::new(Vec::new());
            image.write_to(&mut bytes, format).unwrap();
            let bitmap = decode(bytes.get_ref()).expect("documented encoding");
            assert_eq!((bitmap.width, bitmap.height), (30, 20));
        }
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="31" height="21"><rect width="31" height="21" fill="red"/></svg>"#;
        assert_eq!(
            (decode(svg).unwrap().width, decode(svg).unwrap().height),
            (31, 21)
        );
    }

    /// Arbitrary bytes and decompression dimensions cannot become native allocations.
    #[test]
    fn corrupt_unsupported_and_huge_images_fail_individually() {
        assert!(decode(b"broken image").is_err());
        assert!(
            decode(br#"<svg xmlns="http://www.w3.org/2000/svg" width="100000" height="100000"/>"#)
                .is_err()
        );
        let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(4097, 1, Rgba([0, 0, 0, 255])));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, ImageFormat::Png).unwrap();
        assert!(decode(bytes.get_ref()).is_err());
    }

    /// SVGZ is deliberately outside the supported set: usvg's implicit gzip inflate has no output quota.
    #[test]
    fn compressed_svg_is_rejected_before_implicit_decompression() {
        // Gzip of a valid 1x1 SVG: the small fixture proves the encoding is denied before size inspection.
        let gzip = &[
            31, 139, 8, 0, 0, 0, 0, 0, 0, 10, 179, 41, 46, 75, 87, 168, 200, 205, 201, 43, 182, 85,
            202, 40, 41, 41, 176, 210, 215, 47, 47, 47, 215, 43, 55, 214, 203, 47, 74, 215, 55, 50,
            48, 48, 208, 47, 46, 75, 87, 82, 40, 207, 76, 41, 201, 176, 85, 50, 84, 82, 200, 72,
            205, 76, 207, 40, 1, 49, 245, 237, 0, 90, 22, 239, 220, 62, 0, 0, 0,
        ];
        assert!(
            decode(gzip).is_err(),
            "compressed SVG must not enter an unbounded inflater"
        );
    }

    /// Small encoded entity declarations must not expand before the native pixel budget is checked.
    #[test]
    fn svg_entity_declarations_are_rejected_before_expansion() {
        let source = br#"<!DOCTYPE svg [<!ENTITY body "expanded text">]><svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><text>&body;</text></svg>"#;
        assert!(decode(source).is_err(), "image XML cannot declare entities");
    }

    /// A bounded XML tree also caps recursive SVG conversion, before native rasterization begins.
    #[test]
    fn excessively_nested_and_dense_svg_trees_are_rejected() {
        let nested = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\">{}<rect width=\"1\" height=\"1\"/>{}</svg>",
            "<g>".repeat(256),
            "</g>".repeat(256)
        );
        assert!(decode(nested.as_bytes()).is_err());
        let dense = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\">{}</svg>",
            "<rect width=\"1\" height=\"1\"/>".repeat(10_001)
        );
        assert!(decode(dense.as_bytes()).is_err());
    }

    /// Intermediate filter and pattern surfaces cannot bypass a tiny root image's pixel budget.
    #[test]
    fn svg_effect_surfaces_use_the_same_allocation_budget() {
        let filter = br#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><defs><filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="128" height="128"><feFlood flood-color="red"/></filter></defs><rect width="1" height="1" filter="url(#f)"/></svg>"#;
        assert!(decode_with_budget(filter, 1024).is_err());
        let pattern = br#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><defs><pattern id="p" patternUnits="userSpaceOnUse" width="128" height="128"><rect width="128" height="128" fill="red"/></pattern></defs><rect width="1" height="1" fill="url(#p)"/></svg>"#;
        assert!(decode_with_budget(pattern, 1024).is_err());
    }

    /// Reused SVG definitions are charged by expanded reference cost before usvg clones them.
    #[test]
    fn svg_reference_expansion_is_bounded_before_conversion() {
        let mut definitions = "<g id=\"g0\"><rect width=\"1\" height=\"1\"/></g>".to_string();
        for index in 1..15 {
            definitions.push_str(&format!(
                "<g id=\"g{index}\"><use href=\"#g{}\"/><use href=\"#g{}\"/></g>",
                index - 1,
                index - 1
            ));
        }
        let source = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\"><defs>{definitions}</defs><use href=\"#g14\"/></svg>"
        );
        assert!(decode(source.as_bytes()).is_err());
    }

    /// The preflight must resolve the exact same reference as usvg, including namespace priority.
    #[test]
    fn svg_expansion_guard_handles_whitespace_and_xlink_priority() {
        let mut definitions = "<g id=\"g0\"><rect width=\"1\" height=\"1\"/></g>".to_string();
        for index in 1..15 {
            definitions.push_str(&format!(
                "<g id=\"g{index}\"><use href=\" #g{} \"/><use href=\" #g{} \"/></g>",
                index - 1,
                index - 1
            ));
        }
        let spaced = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\"><defs>{definitions}</defs><use href=\" #g14 \"/></svg>"
        );
        let dual = spaced
            .replace(
                "width=\"1\" height=\"1\"><defs>",
                "xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"1\" height=\"1\"><defs>",
            )
            .replace("href=\" #g", "href=\"#g0\" xlink:href=\" #g");
        let duplicates = spaced.replace(" #g", "#g").replace(" \"", "\"").replace(
            "</defs>",
            &format!(
                "{}</defs>",
                (0..15)
                    .map(|index| format!("<g id=\"g{index}\"/>"))
                    .collect::<String>()
            ),
        );
        // The seam is parsing: a later rendering rejection would still allow expensive expansion first.
        let denied = [&spaced, &dual, &duplicates].map(|source| {
            super::super::svg::parse(source, &resvg::usvg::Options::default()).is_err()
        });
        assert_eq!(
            denied, [true; 3],
            "whitespace, xlink priority and first-wins duplicate IDs must match usvg"
        );
    }

    /// Text SVGs need the same system-font database as the established native vector preview.
    #[test]
    fn svg_text_keeps_visible_glyphs() {
        let source = br#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="24"><text x="2" y="18" font-family="Segoe UI" font-size="16">Hello</text></svg>"#;
        let bitmap = decode(source).unwrap();
        assert!(
            bitmap
                .image
                .as_bytes(0)
                .unwrap()
                .chunks_exact(4)
                .any(|pixel| pixel[3] != 0),
            "SVG text must produce visible pixels"
        );
    }
}
