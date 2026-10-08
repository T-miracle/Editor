//! Readonly preview-tree regressions observe native image declarations and their original Markdown ranges.

use super::blocks;
use plugin_protocol::ui;

/// Limits apply before a rejected event tree can exhaust guest fuel or produce an oversized native scene.
#[test]
fn oversized_derived_work_returns_a_limit_without_weakening_small_previews() {
    assert!(blocks(&"paragraph\n\n".repeat(10_000), "zh-CN").is_err());
    assert!(blocks(&"a".repeat(65_537), "en").is_err());
    assert!(blocks("# 正常\n\n- [ ] Task\n", "zh-CN").is_ok());
}

/// Traverse only the public node tree, keeping assertions independent of the parser's internal representation.
fn descendants(nodes: &[ui::Node]) -> Vec<&ui::Node> {
    let mut result = Vec::new();
    for node in nodes {
        result.push(node);
        match &node.kind {
            ui::Kind::Column { children } | ui::Kind::Row { children } => {
                result.extend(descendants(children));
            }
            ui::Kind::Scroll { content } => {
                result.extend(descendants(std::slice::from_ref(content.as_ref())));
            }
            _ => {}
        }
    }
    result
}

/// Compact ordinary items retain each leaf's mapping; nested lists and loose paragraphs keep their grouping.
#[test]
fn compact_list_items_preserve_content_mapping_and_multi_block_spacing() {
    let source = "- **正文** [链接](next.md)\n- [ ] 待办\n- 父项\n  - 子项\n\n  第二段\n";
    let nodes = blocks(source, "zh-CN").unwrap();
    let all = descendants(&nodes);
    let bodies: Vec<_> = all
        .iter()
        .filter(|node| node.id.ends_with("item-body"))
        .collect();
    assert_eq!(
        bodies.len(),
        1,
        "only the multi-block parent needs a column"
    );
    assert!(matches!(&bodies[0].kind, ui::Kind::Column { children } if children.len() == 3));
    assert_eq!(bodies[0].layout.gap, 6.);
    assert!(
        all.iter()
            .any(|node| matches!(node.kind, ui::Kind::Checkbox { .. }))
    );
    let rich: Vec<_> = all
        .iter()
        .filter_map(|node| match &node.kind {
            ui::Kind::RichText { html } => Some((node, html)),
            _ => None,
        })
        .collect();
    for text in [
        "<strong>正文</strong>",
        "href=\"next.md\"",
        "待办",
        "父项",
        "子项",
        "第二段",
    ] {
        assert!(
            rich.iter()
                .any(|(node, html)| html.contains(text) && node.source_range.is_some())
        );
    }
}

/// The guest declares an image with literal author alt text and a source range; it never reads its resource.
#[test]
fn standalone_image_declares_source_alt_and_original_utf8_range() {
    let nodes = blocks("![中文](img.png)", "zh-CN").unwrap();
    let images: Vec<_> = descendants(&nodes)
        .into_iter()
        .filter(|node| matches!(node.kind, ui::Kind::Image { .. }))
        .collect();
    assert_eq!(
        images.len(),
        1,
        "the preview must declare one native image, not an alt-text placeholder"
    );
    let image = images[0];
    assert!(matches!(&image.kind,
        ui::Kind::Image { source, alt } if source == "img.png" && alt == "中文"));
    let range = image
        .source_range
        .as_ref()
        .expect("images map to their actual Markdown bytes");
    assert_eq!((range.start, range.end), (0, 18));
    assert!(
        descendants(&nodes).into_iter().all(
            |node| !matches!(&node.kind, ui::Kind::RichText { html } if html.contains("<img"))
        )
    );
}

/// Inline image declarations retain their order, surrounding emphasis, code and independent source ranges.
#[test]
fn inline_images_preserve_text_emphasis_and_distinct_ranges() {
    let source = "**前 ![甲](a.png) 中 ![乙](b.png) 后** 和 \u{0060}代码\u{0060}";
    let nodes = blocks(source, "zh-CN").unwrap();
    let all = descendants(&nodes);
    let images: Vec<_> = all
        .iter()
        .filter(|node| matches!(node.kind, ui::Kind::Image { .. }))
        .collect();
    assert_eq!(images.len(), 2);
    for (image, expected_source, expected_alt, expected_range) in [
        (images[0], "a.png", "甲", (6, 19)),
        (images[1], "b.png", "乙", (24, 37)),
    ] {
        assert!(matches!(&image.kind, ui::Kind::Image { source, alt }
            if source == expected_source && alt == expected_alt));
        let range = image.source_range.as_ref().unwrap();
        assert_eq!((range.start, range.end), expected_range);
    }
    let rich: String = all
        .iter()
        .filter_map(|node| match &node.kind {
            ui::Kind::RichText { html } => Some(html.as_str()),
            _ => None,
        })
        .collect();
    assert!(rich.contains("<strong>前 "));
    assert!(rich.contains("<strong> 中 "));
    assert!(rich.contains("<strong> 后"));
    assert!(rich.contains("<code>代码</code>"));
    assert!(!rich.contains("<img"));
    let ids: std::collections::HashSet<_> = all.iter().map(|node| &node.id).collect();
    assert_eq!(
        ids.len(),
        all.len(),
        "native image and rich fragments must have unique identities"
    );
}

/// Quote and tight-list nesting retain native image declarations without flattening neighboring styled text.
#[test]
fn nested_quote_lists_preserve_images_and_styled_neighboring_text() {
    let source = "> - **前** ![甲](a.png) *后*\n>   - ![乙](b.png)\n";
    let nodes = blocks(source, "en-US").unwrap();
    let all = descendants(&nodes);
    let images: Vec<_> = all
        .iter()
        .filter(|node| matches!(node.kind, ui::Kind::Image { .. }))
        .collect();
    assert_eq!(images.len(), 2);
    for image in images {
        let range = image.source_range.as_ref().unwrap();
        assert!(matches!(
            &source[range.start..range.end],
            "![甲](a.png)" | "![乙](b.png)"
        ));
    }
    let rich: String = all
        .iter()
        .filter_map(|node| match &node.kind {
            ui::Kind::RichText { html } => Some(html.as_str()),
            _ => None,
        })
        .collect();
    assert!(rich.contains("<strong>前</strong>"));
    assert!(rich.contains("<em>后</em>"));
    assert!(all.iter().any(|node| node.id.ends_with("-quote")));
    assert!(all.iter().filter(|node| node.id.ends_with("-list")).count() >= 2);
}

/// Tables with images retain separate header/body rows and equal cell counts instead of becoming a flat flow.
#[test]
fn table_images_preserve_header_cell_layout_and_rich_text() {
    let source =
        "| **标题** ![头](h.png) | 描述 |\n| --- | --- |\n| 左 ![图](a.png) 右 | *后文* |\n";
    let nodes = blocks(source, "zh-CN").unwrap();
    assert_eq!(nodes.len(), 1);
    let ui::Kind::Column { children: rows } = &nodes[0].kind else {
        panic!("an image table must keep its native row and cell structure");
    };
    // Rows keep their own nodes; a rule after each one carries no content of its own.
    let rows: Vec<_> = rows
        .iter()
        .filter(|node| matches!(node.kind, ui::Kind::Row { .. }))
        .collect();
    assert_eq!(rows.len(), 2);
    for row in rows {
        assert!(matches!(&row.kind, ui::Kind::Row { children } if children.len() == 2));
    }
    let all = descendants(&nodes);
    assert_eq!(
        all.iter()
            .filter(|node| matches!(node.kind, ui::Kind::Image { .. }))
            .count(),
        2
    );
    let rich: String = all
        .iter()
        .filter_map(|node| match &node.kind {
            ui::Kind::RichText { html } => Some(html.as_str()),
            _ => None,
        })
        .collect();
    for text in ["标题", "描述", "左 ", " 右", "<em>后文</em>"] {
        assert!(
            rich.contains(text),
            "table image conversion must preserve every cell's text and style"
        );
    }
}

/// A table whose cells carry inline code renders every cell as its own rich block.
/// One generated `<table>` would let neighbouring cells share native inline state and lose
/// their glyphs, so each cell owns an identity and its header cell keeps the muted surface.
#[test]
fn table_cells_keep_inline_code_in_independent_rich_blocks() {
    let source = "| 验收 | 结果 |\n| --- | --- |\n| T01 | `language_tests`: 高亮出现 |\n| T02 | `lsp_tests` 与 `dependency_tests` |\n";
    let nodes = blocks(source, "zh-CN").unwrap();
    assert_eq!(nodes.len(), 1);
    let all = descendants(&nodes);
    assert!(
        all.iter().all(
            |node| !matches!(&node.kind, ui::Kind::RichText { html } if html.contains("<table"))
        ),
        "a native table must not fall back to one generated HTML table"
    );
    let cells: Vec<_> = all
        .iter()
        .filter(|node| node.id.contains("table-cell-"))
        .collect();
    assert_eq!(
        cells.len(),
        6,
        "every header and body cell keeps its own block"
    );
    let header: Vec<_> = cells
        .iter()
        .filter(|cell| cell.role == "github-muted")
        .collect();
    assert_eq!(
        header.len(),
        2,
        "only the header cells keep the muted table surface"
    );
    assert!(header.iter().all(|cell| matches!(
        &cell.kind,
        ui::Kind::RichText { html } if html.contains("<strong>")
    )));
    assert_eq!(
        cells
            .iter()
            .filter(|cell| matches!(
                &cell.kind,
                ui::Kind::RichText { html } if html.contains("<code>language_tests</code>")
            ))
            .count(),
        1
    );
    let ids: std::collections::HashSet<_> = all.iter().map(|node| &node.id).collect();
    assert_eq!(
        ids.len(),
        all.len(),
        "native table cells must have unique identities"
    );
    let rich: String = all
        .iter()
        .filter_map(|node| match &node.kind {
            ui::Kind::RichText { html } => Some(html.as_str()),
            _ => None,
        })
        .collect();
    for text in [
        "<code>language_tests</code>",
        "<code>lsp_tests</code>",
        "<code>dependency_tests</code>",
        "高亮出现",
    ] {
        assert!(rich.contains(text), "missing rendered cell content: {text}");
    }
}

/// Raw HTML remains literal text; only parsed Markdown images may declare controlled image resources.
#[test]
fn raw_html_and_code_never_declare_implicit_images() {
    let source = "<img src=\"evil.png\" alt=\"字\">\n\n文字 <img src=\"inline.png\"> \u{0060}![伪](code.png)\u{0060}\n\n![真](img.png)";
    let nodes = blocks(source, "zh-CN").unwrap();
    let all = descendants(&nodes);
    let images: Vec<_> = all
        .iter()
        .filter(|node| matches!(node.kind, ui::Kind::Image { .. }))
        .collect();
    assert_eq!(images.len(), 1);
    assert!(
        matches!(&images[0].kind, ui::Kind::Image { source, alt } if source == "img.png" && alt == "真")
    );
    assert!(all.iter().any(|node| matches!(&node.kind,
        ui::Kind::Text { text } if text.contains("<img src=\"evil.png\""))));
    let rich: String = all
        .iter()
        .filter_map(|node| match &node.kind {
            ui::Kind::RichText { html } => Some(html.as_str()),
            _ => None,
        })
        .collect();
    assert!(rich.contains("&lt;img src="));
    assert!(rich.contains("<code>![伪](code.png)</code>"));
    assert!(!rich.contains("<img"));
}

/// Structurally invalid image URIs fail locally, leaving surrounding Markdown and alternative text visible.
#[test]
fn empty_and_oversized_image_sources_leave_localized_alt_and_surrounding_text() {
    for (locale, empty_reason, oversized_reason) in [
        ("zh-CN", "没有地址", "超过"),
        ("en-US", "no source", "exceeds"),
    ] {
        for (source, reason) in [
            ("前\n\n![替代]()\n\n后".to_owned(), empty_reason),
            (
                format!("前\n\n![替代]({})\n\n后", "a".repeat(4097)),
                oversized_reason,
            ),
        ] {
            let nodes = blocks(&source, locale).unwrap();
            let all = descendants(&nodes);
            assert!(
                all.iter()
                    .all(|node| !matches!(node.kind, ui::Kind::Image { .. }))
            );
            assert!(
                all.iter()
                    .any(|node| matches!(&node.kind, ui::Kind::Text { text }
                if text.contains("替代") && text.contains(reason)))
            );
            let rich: String = all
                .iter()
                .filter_map(|node| match &node.kind {
                    ui::Kind::RichText { html } => Some(html.as_str()),
                    _ => None,
                })
                .collect();
            assert!(rich.contains("前") && rich.contains("后"));
            assert!(!rich.contains("<img"));
        }
    }
}
