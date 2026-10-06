//! Tool artwork is a bounded geometry-only SVG, never an ambient resource loader.

/// Recheck installed artwork before handing bytes to native rendering; XML resolves no entities.
pub(crate) fn svg(bytes: &[u8]) -> anyhow::Result<()> {
    anyhow::ensure!(bytes.len() <= 64 * 1024, "Tool SVG exceeds 64 KiB");
    let source = std::str::from_utf8(bytes)?;
    anyhow::ensure!(
        !source.to_ascii_lowercase().contains("<!doctype"),
        "Tool SVG cannot declare a DTD"
    );
    let document = roxmltree::Document::parse_with_options(
        source,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: 512,
            entity_resolver: None,
        },
    )?;
    anyhow::ensure!(
        document.root_element().tag_name().name() == "svg",
        "Invalid SVG root"
    );
    for node in document.descendants() {
        anyhow::ensure!(!node.is_pi(), "Tool SVG cannot load XML stylesheets");
        if node.is_text() {
            anyhow::ensure!(
                node.text().is_some_and(|text| text.trim().is_empty()),
                "Tool SVG cannot render text"
            );
        }
        if !node.is_element() {
            continue;
        }
        let tag = node.tag_name();
        anyhow::ensure!(
            matches!(tag.namespace(), None | Some("http://www.w3.org/2000/svg"))
                && [
                    "svg", "g", "path", "rect", "circle", "ellipse", "line", "polyline", "polygon"
                ]
                .contains(&tag.name())
                && node.ancestors().count() <= 18,
            "Tool SVG permits only bounded geometric elements"
        );
        for attribute in node.attributes() {
            // A whitelist rejects href regardless of prefix/whitespace, plus CSS, event handlers and filters.
            anyhow::ensure!(
                attribute.namespace().is_none()
                    && [
                        "viewBox",
                        "width",
                        "height",
                        "x",
                        "y",
                        "x1",
                        "y1",
                        "x2",
                        "y2",
                        "cx",
                        "cy",
                        "r",
                        "rx",
                        "ry",
                        "d",
                        "points",
                        "fill",
                        "stroke",
                        "stroke-width",
                        "stroke-linecap",
                        "stroke-linejoin",
                        "stroke-miterlimit",
                        "stroke-dasharray",
                        "stroke-dashoffset",
                        "fill-rule",
                        "clip-rule",
                        "opacity",
                        "fill-opacity",
                        "stroke-opacity",
                        "transform",
                        "vector-effect"
                    ]
                    .contains(&attribute.name()),
                "Unsupported tool SVG attribute: {}",
                attribute.name()
            );
            let compact: String = attribute
                .value()
                .chars()
                .filter(|value| !value.is_whitespace())
                .collect();
            anyhow::ensure!(
                !compact.to_ascii_lowercase().contains("url("),
                "Tool SVG cannot reference URL paints"
            );
            if matches!(attribute.name(), "fill" | "stroke") {
                // Theme tinting needs currentColor; literal color names and hex need no paint servers.
                let color = attribute.value().trim();
                anyhow::ensure!(
                    (color.len() <= 32
                        && !color.is_empty()
                        && color.bytes().all(|byte| byte.is_ascii_alphabetic()))
                        || color
                            .strip_prefix('#')
                            .is_some_and(|hex| [3, 4, 6, 8].contains(&hex.len())
                                && hex.bytes().all(|byte| byte.is_ascii_hexdigit())),
                    "Tool SVG requires a literal or currentColor paint"
                );
            }
        }
    }
    Ok(())
}
