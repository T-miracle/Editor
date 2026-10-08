//! Actual parser/native-leaf regressions cover linked images, keyboard captions and shared activation budgets.

use super::{
    Index,
    targets::{Destination, destination},
};
use crate::{Source, State, preview::blocks};
use plugin_protocol::{api, ui};

/// Production attaches metadata after both the parsed index and final native tree have been derived.
fn annotated(source: &str) -> (Index, Vec<ui::Node>) {
    let index = Index::parse(source);
    let mut nodes = blocks(source, "zh-CN").unwrap();
    index.annotate(&mut nodes);
    (index, nodes)
}

/// Inspect public containers and leaves without depending on the parser's private intermediate representation.
fn descendants(nodes: &[ui::Node]) -> Vec<&ui::Node> {
    let mut all = Vec::new();
    for node in nodes {
        all.push(node);
        match &node.kind {
            ui::Kind::Column { children } | ui::Kind::Row { children } => {
                all.extend(descendants(children))
            }
            ui::Kind::Scroll { content } => {
                all.extend(descendants(std::slice::from_ref(content.as_ref())))
            }
            _ => {}
        }
    }
    all
}

/// Bound source versions let ordinary public UI validation observe mappings and activation budgets.
fn version() -> api::DocumentVersion {
    api::DocumentVersion {
        id: "source".into(),
        path: "notes.md".into(),
        revision: 1,
    }
}

fn document(nodes: Vec<ui::Node>) -> ui::Document {
    let mut document = ui::Document::new(ui::Node::column("test-root", nodes));
    document.source = Some(version());
    document.link_events = true;
    document
}

/// The repository's real platform specification must publish its complete preview, including its final block.
#[test]
fn platform_specification_keeps_complete_native_preview() {
    let source = include_str!("../../../../docs/plugins/specs/plugin-api-platform.md");
    let mut state = State::default();
    state.environment.locale = "zh-CN".into();
    state.source = Some(Source {
        version: version(),
        text: source.into(),
    });
    state.refresh();
    assert!(
        !state.preview_limited,
        "actual specification exceeds parser work budget"
    );
    let validation = document(state.blocks.clone()).validate();
    assert!(validation.is_ok(), "actual specification: {validation:?}");
    let view = state.view();
    assert!(view.document.validate().is_ok());
    let all = descendants(std::slice::from_ref(&view.document.root));
    assert!(!all.iter().any(|node| node.id == "preview-limit"));
    assert!(all.iter().any(|node| {
        node.source_range
            .is_some_and(|range| range.end == source.len())
    }));
}

/// An image-only outer link remains a native image and resolves the original relative/anchor/web policy target.
#[test]
fn image_only_links_declare_leaf_targets_for_relative_anchor_and_web_destinations() {
    for (uri, href, expected) in [
        (
            "next.md",
            "next.md",
            Destination::Relative {
                path: "next.md".into(),
                fragment: None,
            },
        ),
        (
            "#标题",
            "#%E6%A0%87%E9%A2%98",
            Destination::Anchor("标题".into()),
        ),
        (
            "https://example.com/?q=1&x=2",
            "https://example.com/?q=1&x=2",
            Destination::External("https://example.com/?q=1&x=2".into()),
        ),
    ] {
        let source = format!("[![打开图片](missing.png)]({uri})");
        let (index, nodes) = annotated(&source);
        let image = descendants(&nodes)
            .into_iter()
            .find(|node| matches!(node.kind, ui::Kind::Image { .. }))
            .unwrap();
        assert_eq!(
            image.links,
            vec![ui::LinkTarget {
                uri: href.into(),
                label: "打开图片".into()
            }]
        );
        let range = image.source_range.unwrap();
        assert_eq!(
            source.get(range.start..range.end),
            Some("![打开图片](missing.png)")
        );
        let original = index.resolve(&nodes, &image.id, href).unwrap();
        assert_eq!(original, uri);
        assert_eq!(destination(original), Ok(expected));
        assert!(document(nodes).validate().is_ok());
    }
}

/// Split rich/image/rich leaves keep local captions and every repeated link occurrence in source order.
#[test]
fn mixed_and_repeated_links_attach_only_to_their_actual_visible_leaves() {
    let source = "[前 ![图](missing.png) 后](next.md) [甲](same.md) [乙](same.md)\n";
    let (index, nodes) = annotated(source);
    let all = descendants(&nodes);
    let targets: Vec<_> = all.iter().filter(|node| !node.links.is_empty()).collect();
    assert_eq!(
        targets.len(),
        3,
        "the containing column is not an activation target"
    );
    assert_eq!(
        targets[0].links,
        vec![ui::LinkTarget {
            uri: "next.md".into(),
            label: "前 ".into()
        }]
    );
    assert!(matches!(targets[1].kind, ui::Kind::Image { .. }));
    assert_eq!(
        targets[1].links,
        vec![ui::LinkTarget {
            uri: "next.md".into(),
            label: "图".into()
        }]
    );
    assert_eq!(
        targets[2].links,
        vec![
            ui::LinkTarget {
                uri: "next.md".into(),
                label: " 后".into()
            },
            ui::LinkTarget {
                uri: "same.md".into(),
                label: "甲".into()
            },
            ui::LinkTarget {
                uri: "same.md".into(),
                label: "乙".into()
            },
        ]
    );
    for node in all {
        if matches!(
            node.kind,
            ui::Kind::Column { .. } | ui::Kind::Row { .. } | ui::Kind::Scroll { .. }
        ) {
            assert!(node.links.is_empty());
        }
        for target in &node.links {
            assert_eq!(
                index.resolve(&nodes, &node.id, &target.uri),
                Some(target.uri.as_str())
            );
        }
    }
    assert!(document(nodes).validate().is_ok());
}

/// An invalid image's visible alternative-text node retains the outer link independently of image resource loading.
#[test]
fn image_error_alternative_text_preserves_the_outer_link_binding() {
    let source = "[![替代文字]()](next.md)";
    let (index, nodes) = annotated(source);
    let alternative = descendants(&nodes).into_iter().find(|node| matches!(&node.kind, ui::Kind::Text { text } if text.contains("图片引用没有地址"))).unwrap();
    assert_eq!(
        alternative.links,
        vec![ui::LinkTarget {
            uri: "next.md".into(),
            label: "替代文字".into()
        }]
    );
    assert_eq!(
        index.resolve(&nodes, &alternative.id, "next.md"),
        Some("next.md")
    );
    assert!(index.resolve(&nodes, &alternative.id, "other.md").is_none());
    assert!(document(nodes).validate().is_ok());
}

/// Only real outer links activate images; alt links, raw HTML and code cannot create navigation targets.
#[test]
fn image_alt_html_and_code_cannot_mint_activation_targets() {
    let source = "[![外 内](missing.png)](next.md)\n\n<a href=\"html.md\">伪</a>\n\n```\n[代码](code.md)\n```\n";
    let (index, nodes) = annotated(source);
    let all = descendants(&nodes);
    let linked: Vec<_> = all.iter().filter(|node| !node.links.is_empty()).collect();
    assert_eq!(linked.len(), 1);
    let image = linked[0];
    assert!(matches!(image.kind, ui::Kind::Image { .. }));
    assert_eq!(
        image.links,
        vec![ui::LinkTarget {
            uri: "next.md".into(),
            label: "外 内".into()
        }]
    );
    assert_eq!(index.resolve(&nodes, &image.id, "next.md"), Some("next.md"));
    for node in all {
        for uri in ["inside.md", "html.md", "code.md"] {
            assert!(index.resolve(&nodes, &node.id, uri).is_none());
        }
    }
    // CommonMark disables an enclosing link when it parses a link inside image alt.
    // Observe the actual events so an invalid outer-link fixture cannot grant next.md access.
    for source in [
        "![外 [内](inside.md)](missing.png)",
        "[![外 [内](inside.md)](missing.png)](next.md)",
    ] {
        let mut image_depth = 0usize;
        let mut parsed_links = Vec::new();
        for event in crate::preview::parser(source) {
            match event {
                pulldown_cmark::Event::Start(pulldown_cmark::Tag::Image { .. }) => {
                    image_depth += 1;
                }
                pulldown_cmark::Event::End(pulldown_cmark::TagEnd::Image) => {
                    image_depth -= 1;
                }
                pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link { dest_url, .. }) => {
                    parsed_links.push((dest_url.into_string(), image_depth));
                }
                _ => {}
            }
        }
        assert_eq!(parsed_links, vec![("inside.md".into(), 1)]);
        let (index, nodes) = annotated(source);
        for node in descendants(&nodes) {
            assert!(node.links.is_empty());
            for uri in ["inside.md", "next.md"] {
                assert!(index.resolve(&nodes, &node.id, uri).is_none());
            }
        }
    }
}

/// Captions are visible parser text and valid bounded UTF-8 prefixes; empty image alt stays empty for host locale.
#[test]
fn link_captions_preserve_visible_text_and_truncate_without_splitting_utf8() {
    let (_, nodes) = annotated("[**粗体** `代码` &amp; 后](next.md)");
    assert_eq!(nodes[0].links[0].label, "粗体 代码 & 后");
    let caption = format!("{}🙂后文", "中".repeat(85));
    for source in [
        format!("[{caption}](next.md)"),
        format!("[![{caption}](missing.png)](next.md)"),
    ] {
        let (_, nodes) = annotated(&source);
        let link = descendants(&nodes)
            .into_iter()
            .find_map(|node| node.links.first())
            .unwrap();
        assert_eq!(link.label, "中".repeat(85));
        assert_eq!(link.label.len(), 255);
        assert!(document(nodes).validate().is_ok());
    }
    let (_, empty) = annotated("[![](missing.png)](next.md)");
    assert!(empty[0].links[0].label.is_empty());
}

/// Metadata cannot introduce an invalid 4096-byte URI even when Unicode expansion makes its rendered href larger.
#[test]
fn oversized_rendered_hrefs_are_not_declared_as_native_focus_targets() {
    let source = format!("[链接](<{}.md>)", "中".repeat(500));
    let (_, nodes) = annotated(&source);
    assert!(
        descendants(&nodes)
            .into_iter()
            .all(|node| node.links.is_empty())
    );
    assert!(document(nodes).validate().is_ok());
}

/// Generated focus controls share the public node quota; existing fallback keeps an oversized source preview valid.
#[test]
fn activation_target_budget_uses_the_existing_visible_preview_limit_fallback() {
    let source: String = (0..700)
        .map(|index| format!("[链{index}](n{index}.md) "))
        .collect();
    assert!(source.len() <= 1024 * 1024);
    let (_, nodes) = annotated(&source);
    assert_eq!(
        descendants(&nodes)
            .iter()
            .map(|node| node.links.len())
            .sum::<usize>(),
        700
    );
    assert!(
        document(nodes).validate().is_err(),
        "links consume actual native focus-control budget"
    );
    let mut state = State::default();
    state.environment.locale = "zh-CN".into();
    state.source = Some(Source {
        version: version(),
        text: source,
    });
    state.refresh();
    let view = state.view();
    assert!(view.document.validate().is_ok());
    assert_eq!(view.document.source, Some(version()));
    assert!(descendants(std::slice::from_ref(&view.document.root)).into_iter().any(|node| matches!(&node.kind, ui::Kind::Text { text } if text.contains("原生预览限制"))));
}
