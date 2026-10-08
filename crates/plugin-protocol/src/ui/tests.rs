//! Contract regression cases are also shipped with the standalone SDK.
use super::*;

/// An icon changes only presentation: wire round trips retain its label and versioned Click validation.
#[test]
fn icon_buttons_retain_accessible_labels_and_native_events() {
    let svg = "<svg viewBox=\"0 0 24 24\"><path d=\"M4 4h16v16H4Z\"/></svg>";
    let button = Node::button("tool", "标题").icon(svg).tooltip("一级标题");
    let encoded = serde_json::to_vec(&button).unwrap();
    let decoded: Node = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, button);
    let mut document = Document::new(decoded).revision(5);
    document.validate().unwrap();
    let event = UiEvent {
        revision: 5,
        node: "tool".into(),
        action: Action::Click,
    };
    document.validate_event(&event).unwrap();
    document.root.disabled = true;
    assert!(document.validate_event(&event).is_err());
    // Older plain buttons omit the additive metadata and keep their existing text presentation.
    let plain: Node =
        serde_json::from_str(r#"{"id":"plain","kind":{"type":"button","label":"Plain"}}"#).unwrap();
    assert!(plain.button_icon.is_none());
    assert!(
        !serde_json::to_value(plain)
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("button_icon")
    );
}

/// Artwork shares root/toolbar/dialog budgets and cannot create an unnamed or non-button control.
#[test]
fn icon_buttons_enforce_kind_names_and_shared_quotas() {
    for node in [
        Node::text("bad", "Text").icon("<svg/>"),
        Node::button("bad", " ").icon("<svg/>"),
        Node::button("bad", "Bad").icon(""),
        Node::button("bad", "Bad").icon("x".repeat(4097)),
    ] {
        assert!(Document::new(node).validate().is_err());
    }
    let icons = (0..64)
        .map(|index| Node::button(format!("b{index}"), "Action").icon("<svg/>"))
        .collect();
    let mut document = Document::new(Node::row("buttons", icons));
    document.validate().unwrap();
    document.dialog = Some(Dialog::new(
        "dialog",
        "Title",
        Node::button("extra", "Extra").icon("<svg/>"),
    ));
    assert!(document.validate().unwrap_err().contains("icon quota"));
}

/// Alternative text is a real link only with an explicit target and opt-in; replaced scenes stay inert.
#[test]
fn declared_content_links_match_their_target_and_scene() {
    let mut node = Node::text("alt", "打开图片");
    node.links.push(LinkTarget {
        uri: "next.md".into(),
        label: "打开图片".into(),
    });
    let mut document = Document::new(node).revision(3);
    assert!(document.validate().is_ok());
    let mut event = UiEvent {
        revision: 3,
        node: "alt".into(),
        action: Action::Link {
            uri: "next.md".into(),
        },
    };
    assert!(document.validate_event(&event).is_err());
    document.link_events = true;
    assert!(document.validate_event(&event).is_ok());
    event.action = Action::Link {
        uri: "other.md".into(),
    };
    assert!(document.validate_event(&event).is_err());
    event.action = Action::Link {
        uri: "next.md".into(),
    };
    event.revision = 2;
    assert_eq!(
        document.validate_event(&event).unwrap_err().code,
        crate::api::ErrorCode::StaleRevision
    );
    event.revision = 3;
    document.root.disabled = true;
    assert!(document.validate_event(&event).is_err());
    document.root.disabled = false;
    document.dialog = Some(Dialog::new("modal", "Title", Node::text("body", "body")));
    assert!(document.validate_event(&event).is_err());
}

/// A guest cannot hide unlimited focus controls or labels inside one otherwise small read-only node.
#[test]
fn declared_links_share_ui_budgets_and_read_only_kinds() {
    let target = LinkTarget {
        uri: "#title".into(),
        label: "标题".into(),
    };
    let mut node = Node::button("bad", "Bad");
    node.links.push(target.clone());
    assert!(Document::new(node).validate().is_err());
    let mut node = Node::rich_text("links", "<p>links</p>");
    node.links = vec![target.clone(); 2048];
    assert!(
        Document::new(node.clone())
            .validate()
            .unwrap_err()
            .contains("node quota")
    );
    node.links = vec![LinkTarget {
        label: "中".repeat(86),
        ..target.clone()
    }];
    assert!(Document::new(node.clone()).validate().is_err());
    node.links = vec![LinkTarget {
        uri: "unsafe\n".into(),
        ..target.clone()
    }];
    assert!(Document::new(node).validate().is_err());
    let mut alt = Node::text("alt", "alt");
    alt.links = vec![target; 2];
    assert!(Document::new(alt).validate().is_err());
}

/// Opt-in links obey the same stale, disabled and modal gates as other native interactions.
#[test]
fn links_require_opt_in_and_current_active_rich_text() {
    let mut document = Document::new(Node::new(
        "link",
        Kind::RichText {
            html: "<a href=\"#x\">x</a>".into(),
        },
    ));
    let mut event = UiEvent {
        revision: 0,
        node: "link".into(),
        action: Action::Link { uri: "#x".into() },
    };
    assert!(document.validate_event(&event).is_err());
    document.link_events = true;
    assert!(document.validate_event(&event).is_ok());
    event.revision = 1;
    assert_eq!(
        document.validate_event(&event).unwrap_err().code,
        crate::api::ErrorCode::StaleRevision
    );
    event.revision = 0;
    document.root.disabled = true;
    assert!(document.validate_event(&event).is_err());
    document.root.disabled = false;
    document.dialog = Some(Dialog::new("modal", "Title", Node::text("body", "body")));
    assert!(document.validate_event(&event).is_err());
    document.dialog = None;
    for uri in ["", "x\n"] {
        event.action = Action::Link { uri: uri.into() };
        assert!(document.validate_event(&event).is_err());
    }
    document.root = Node::text("link", "plain");
    event.action = Action::Link { uri: "#x".into() };
    assert!(document.validate_event(&event).is_err());
}

/// A wrapping row retains this portable layout flag across independent SDK serialization.
#[test]
fn row_wrap_is_a_portable_layout_flag() {
    let document: Document = serde_json::from_value(serde_json::json!({
        "version":1, "revision":0, "root":{
            "id":"toolbar", "layout":{"wrap":true}, "kind":{"type":"row","children":[]}
        }
    }))
    .unwrap();
    assert_eq!(
        serde_json::to_value(&document).unwrap()["root"]["layout"]["wrap"],
        true
    );
    assert!(document.validate().is_ok());
}

/// Source controls share the document's identity, text, node and depth limits rather than separate quotas.
#[test]
fn editor_toolbar_is_version_bound_and_shares_document_quotas() {
    let mut document = Document::new(Node::text("body", "Preview"));
    document.editor_toolbar = Some(
        Node::row(
            "toolbar",
            vec![Node::button("bold", "B").tooltip("粗体 / Bold")],
        )
        .wrap(),
    );
    assert!(
        document.validate().is_err(),
        "unbound editor toolbar was accepted"
    );
    document.source = Some(crate::api::DocumentVersion {
        id: "open".into(),
        path: "source.sample".into(),
        revision: 1,
    });
    assert!(document.validate().is_ok());
    document.editor_toolbar = Some(Node::button("body", "Duplicate"));
    assert!(document.validate().is_err());
    document.editor_toolbar = Some(Node::button("bold", "B").tooltip("x".repeat(65537)));
    assert!(document.validate().is_err());
    document.root = Node::column(
        "body",
        (0..1024)
            .map(|i| Node::button(format!("r{i}"), ""))
            .collect(),
    );
    document.editor_toolbar = Some(Node::row(
        "toolbar",
        (0..1024)
            .map(|i| Node::button(format!("t{i}"), ""))
            .collect(),
    ));
    assert!(
        document.validate().is_err(),
        "toolbar reset the node budget"
    );
    document.root = Node::column(
        "body",
        (0..16)
            .map(|i| Node::text(format!("text{i}"), "x".repeat(65536)))
            .collect(),
    );
    document.editor_toolbar = Some(Node::button("bold", ""));
    assert!(document.validate().is_ok());
    document.editor_toolbar = Some(Node::button("bold", "").tooltip("x"));
    assert!(
        document.validate().is_err(),
        "tooltip reset the text budget"
    );
    document.root = Node::text("body", "");
    let mut toolbar = Node::button("bold", "B");
    for i in 0..26 {
        toolbar = Node::scroll(format!("depth{i}"), toolbar);
    }
    document.editor_toolbar = Some(toolbar);
    assert!(
        document.validate().is_err(),
        "toolbar reset the depth budget"
    );
}

/// Toolbar visibility is plugin-owned; an unknown legacy field cannot re-enter the public tree.
#[test]
fn host_toolbar_toggle_is_absent_from_the_current_tree() {
    let document = Document::new(Node::text("body", "Preview"));
    let mut wire = serde_json::to_value(&document).unwrap();
    wire["editor_toolbar_toggle"] = serde_json::json!("显示/隐藏工具栏");
    let decoded: Document = serde_json::from_value(wire).unwrap();
    assert!(
        serde_json::to_value(decoded)
            .unwrap()
            .get("editor_toolbar_toggle")
            .is_none()
    );
}

/// Toolbar callbacks retain stale/disabled/modal rejection even when the source is the only visible surface.
#[test]
fn editor_toolbar_events_share_revision_and_modal_priority() {
    use crate::api::ErrorCode;
    let mut document = Document::new(Node::text("body", "Preview")).revision(3);
    document.source = Some(crate::api::DocumentVersion {
        id: "open".into(),
        path: "source.sample".into(),
        revision: 1,
    });
    document.editor_toolbar = Some(Node::button("bold", "B"));
    let mut event = UiEvent {
        revision: 3,
        node: "bold".into(),
        action: Action::Click,
    };
    assert!(document.validate_event(&event).is_ok());
    event.revision = 2;
    assert_eq!(
        document.validate_event(&event).unwrap_err().code,
        ErrorCode::StaleRevision
    );
    event.revision = 3;
    document.editor_toolbar.as_mut().unwrap().disabled = true;
    assert_eq!(
        document.validate_event(&event).unwrap_err().code,
        ErrorCode::InvalidHandle
    );
    document.editor_toolbar.as_mut().unwrap().disabled = false;
    document.dialog = Some(Dialog::new(
        "modal",
        "Modal",
        Node::button("close", "Close"),
    ));
    assert_eq!(
        document.validate_event(&event).unwrap_err().code,
        ErrorCode::InvalidHandle
    );
    assert!(document.active_node("close").is_some());
    document.dialog = None;
    document.menu = Some(PopupMenu {
        id: "popup".into(),
        x: 0.,
        y: 0.,
        items: vec![MenuItem {
            id: "choice".into(),
            label: "Choice".into(),
            disabled: false,
            separator_before: false,
        }],
    });
    assert!(document.active_node("bold").is_none());
    assert_eq!(
        document.validate_event(&event).unwrap_err().code,
        ErrorCode::InvalidState
    );
    event.node = "popup".into();
    event.action = Action::Select("choice".into());
    assert!(document.validate_event(&event).is_ok());
}

#[test]
fn side_tabs_rejects_ambiguous_items_and_unbounded_geometry() {
    // Collection validation is independent of its location in a document.
    let mut controls = SideTabs {
        id: "tabs".into(),
        position: SideTabsPosition::Right,
        items: vec![SideTab {
            id: "one".into(),
            label: "One".into(),
            status: None,
            disabled: false,
            closable: true,
        }],
        selected: Some("one".into()),
        rename: None,
        width: 180.,
        min_width: 80.,
        max_width: 480.,
    };
    assert!(controls.validate().is_ok());
    controls.width = f32::NAN;
    assert!(controls.validate().is_err());
    controls.width = 180.;
    controls.selected = Some("removed".into());
    assert!(controls.validate().is_err());
    controls.selected = None;
    let duplicate = controls.items[0].clone();
    controls.items.push(duplicate);
    assert!(controls.validate().is_err());
}

/// An omitted edge uses the current default; either explicit edge round-trips.
#[test]
fn sidebar_position_defaults_right_and_round_trips_left() {
    let legacy = serde_json::json!({
        "id": "tabs", "items": [], "selected": null, "rename": null,
        "width": 180., "min_width": 80., "max_width": 480.
    });
    let mut tabs: SideTabs = serde_json::from_value(legacy).unwrap();
    assert_eq!(tabs.position, SideTabsPosition::Right);
    tabs.position = SideTabsPosition::Left;
    let value = serde_json::to_value(&tabs).unwrap();
    assert_eq!(value["position"], "left");
    assert_eq!(serde_json::from_value::<SideTabs>(value).unwrap(), tabs);
}

#[test]
fn rejects_unsupported_and_ambiguous_documents() {
    let mut doc = Document::new(Node::column("root", vec![Node::button("save", "保存")]));
    assert!(doc.validate().is_ok());
    doc.version += 1;
    assert!(doc.validate().is_err());
    doc.version = VERSION;
    doc.dialog = Some(Dialog::new(
        "dialog",
        "标题",
        Node::text("save", "重复标识"),
    ));
    assert!(doc.validate().unwrap_err().contains("Duplicate"));
}

/// Public SDK mappings are version-bound and use inclusive quota limits with half-open byte ranges.
#[test]
fn richtext_source_ranges_require_a_version_and_bounded_offsets() {
    let mut document = Document::new(Node::text("block", "你好").source_range(0..6));
    assert!(document.validate().is_err());
    document.source = Some(crate::api::DocumentVersion {
        id: "open-document".into(),
        path: "source.sample".into(),
        revision: 7,
    });
    for (start, end) in [(0, 0), (0, 6), (1024 * 1024, 1024 * 1024)] {
        document.root.source_range = Some(SourceRange { start, end });
        assert!(document.validate().is_ok(), "valid range {start}..{end}");
    }
    for (start, end) in [(6, 0), (0, 1024 * 1024 + 1)] {
        document.root.source_range = Some(SourceRange { start, end });
        assert!(document.validate().is_err(), "invalid range {start}..{end}");
    }
}

#[test]
fn bounds_native_work_including_table_cells_and_depth() {
    let mut node = Node::text("leaf", "文字");
    for i in 0..26 {
        node = Node::column(format!("n{i}"), vec![node]);
    }
    assert!(Document::new(node).validate().is_err());
    let table = Node::new(
        "table",
        Kind::Table {
            headers: vec!["A".into(), "B".into()],
            rows: vec![vec!["x".into(); 2]; 1100],
        },
    );
    assert!(Document::new(table).validate().is_err());
    assert!(
        Document::new(Node::text("x", "x").width(f32::NAN))
            .validate()
            .is_err()
    );
}

#[test]
fn rejects_invalid_selection_and_ragged_tables() {
    let node = Node::new(
        "choice",
        Kind::Choice {
            options: vec![OptionItem::new("a", "A")],
            selected: Some("missing".into()),
        },
    );
    assert!(Document::new(node).validate().is_err());
    let node = Node::new(
        "table",
        Kind::Table {
            headers: vec!["A".into()],
            rows: vec![vec![]],
        },
    );
    assert!(Document::new(node).validate().is_err());
}

#[test]
fn modal_and_inactive_tabs_do_not_receive_background_actions() {
    let tabs = Node::new(
        "tabs",
        Kind::Tabs {
            selected: "one".into(),
            tabs: vec![
                Tab::new("one", "一", Node::button("first", "第一")),
                Tab::new("two", "二", Node::button("second", "第二")),
            ],
        },
    );
    let mut doc = Document::new(tabs);
    assert!(doc.active_node("first").is_some());
    assert!(doc.active_node("second").is_none());
    doc.dialog = Some(Dialog::new("modal", "标题", Node::button("close", "关闭")));
    assert!(doc.active_node("first").is_none());
    assert!(doc.active_node("close").is_some());
}

#[test]
fn serde_preserves_edit_revisions_and_event_payloads() {
    let doc = Document::new(Node::input(
        "name",
        Input {
            value: "中文".into(),
            value_revision: 9,
            placeholder: "名字".into(),
        },
    ))
    .revision(7);
    assert_eq!(
        serde_json::from_str::<Document>(&serde_json::to_string(&doc).unwrap()).unwrap(),
        doc
    );
    let event = UiEvent {
        revision: 7,
        node: "name".into(),
        action: Action::Submit("中文".into()),
    };
    assert_eq!(
        serde_json::from_str::<UiEvent>(&serde_json::to_string(&event).unwrap()).unwrap(),
        event
    );
}

/// Per-document vector quotas cannot be bypassed by spreading drawings across multiple canvases.
#[test]
fn canvas_geometry_and_aggregate_resource_limits_are_validated() {
    let vector = crate::Paint::Svg {
        rect: crate::Rect {
            x: 0.,
            y: 0.,
            w: 100.,
            h: 100.,
        },
        clip: crate::Rect {
            x: 0.,
            y: 0.,
            w: 100.,
            h: 100.,
        },
        source: "<svg/>".into(),
    };
    let nodes = (0..17)
        .map(|index| {
            Node::new(
                format!("canvas{index}"),
                Kind::Canvas(Canvas {
                    paint: vec![vector.clone()],
                    ..Default::default()
                }),
            )
        })
        .collect();
    assert!(
        Document::new(Node::column("root", nodes))
            .validate()
            .unwrap_err()
            .contains("vector quota")
    );
    let invalid = Document::new(Node::new(
        "invalid",
        Kind::Canvas(Canvas {
            caret: Some(crate::Rect {
                x: f32::NAN,
                ..Default::default()
            }),
            ..Default::default()
        }),
    ));
    assert!(invalid.validate().is_err());
    let drawing = Document::new(Node::new("drawing", Kind::Canvas(Canvas::default())));
    let event = UiEvent {
        revision: 0,
        node: "drawing".into(),
        action: Action::Canvas(CanvasEvent::Text {
            text: "no focus".into(),
        }),
    };
    assert_eq!(
        drawing.validate_event(&event).unwrap_err().code,
        crate::api::ErrorCode::InvalidRequest
    );
}

/// Highlighting is an opt-in readonly request that cannot outlive its owning source version.
#[test]
fn code_highlighting_requires_a_source_version_and_defaults_inert() {
    let document = Document::new(Node::code_block(
        "sample",
        "answer = 42",
        Some("novel".into()),
    ));
    let mut encoded = serde_json::to_value(&document).unwrap();
    assert!(
        !encoded
            .get("code_highlighting")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    );
    encoded["code_highlighting"] = serde_json::json!(true);
    let requested: Document = serde_json::from_value(encoded.clone()).unwrap();
    assert!(requested.validate().unwrap_err().contains("source"));
    encoded["source"] = serde_json::json!({"id":"source", "path":"notes.md", "revision":1});
    let requested: Document = serde_json::from_value(encoded).unwrap();
    requested.validate().unwrap();
}

/// A viewport opt-in cannot derive document authority from an ordinary unbound scroll tree.
#[test]
fn editor_viewport_binding_requires_a_source_document() {
    let document = Document::new(Node::scroll("viewport", Node::text("paragraph", "source")));
    let mut encoded = serde_json::to_value(document).unwrap();
    encoded["editor_viewport"] = serde_json::json!("viewport");
    let bound: Document = serde_json::from_value(encoded).unwrap();
    assert!(
        bound.validate().is_err(),
        "viewport binding requires exact source identity"
    );
}

/// Binding, active block identity and normalized finite geometry are checked before a guest receives input.
#[test]
fn editor_viewport_events_require_exact_current_active_block_and_bounded_geometry() {
    use crate::api::{DocumentVersion, PreviewViewport, ViewportTarget};
    let mut document = Document::new(Node::scroll(
        "viewport",
        Node::column(
            "content",
            vec![
                Node::text("paragraph", "中文").source_range(0..6),
                Node::text("disabled", "隐藏")
                    .source_range(7..13)
                    .disabled(true),
                Node::scroll(
                    "inner",
                    Node::text("inner-block", "内层").source_range(14..20),
                )
                .source_range(14..20),
            ],
        ),
    ))
    .revision(9);
    document.source = Some(DocumentVersion {
        id: "source".into(),
        path: "notes.md".into(),
        revision: 7,
    });
    document.editor_viewport = Some("viewport".into());
    document.validate().unwrap();
    let position = PreviewViewport {
        block: "paragraph".into(),
        source_range: SourceRange { start: 0, end: 6 },
        fraction: 0.5,
        origin: None,
        layout: false,
    };
    let event = UiEvent {
        revision: 9,
        node: "viewport".into(),
        action: Action::Viewport(position.clone()),
    };
    document.validate_event(&event).unwrap();
    for changed in [
        PreviewViewport {
            source_range: SourceRange { start: 0, end: 5 },
            ..position.clone()
        },
        PreviewViewport {
            block: "disabled".into(),
            source_range: SourceRange { start: 7, end: 13 },
            ..position.clone()
        },
        PreviewViewport {
            block: "inner-block".into(),
            source_range: SourceRange { start: 14, end: 20 },
            ..position.clone()
        },
        PreviewViewport {
            block: "inner".into(),
            source_range: SourceRange { start: 14, end: 20 },
            ..position.clone()
        },
        PreviewViewport {
            fraction: f32::NAN,
            ..position.clone()
        },
        PreviewViewport {
            origin: Some(0),
            ..position.clone()
        },
    ] {
        assert!(
            document
                .validate_event(&UiEvent {
                    action: Action::Viewport(changed),
                    ..event.clone()
                })
                .is_err()
        );
    }
    assert!(
        document
            .validate_event(&UiEvent {
                revision: 8,
                ..event.clone()
            })
            .is_err()
    );
    document.editor_viewport = None;
    assert!(
        document.validate_event(&event).is_err(),
        "ordinary scroll defaults inert"
    );
    for target in [
        ViewportTarget::Source {
            offset: 1024 * 1024 + 1,
            line_fraction: 0.0,
        },
        ViewportTarget::Source {
            offset: 0,
            line_fraction: f32::INFINITY,
        },
        ViewportTarget::Preview {
            node: "".into(),
            fraction: 0.0,
        },
        ViewportTarget::Preview {
            node: "paragraph".into(),
            fraction: -0.1,
        },
    ] {
        assert!(target.validate().is_err());
    }
}
/// Visual viewports survive the public JSON contract instead of being silently discarded.
#[test]
fn visual_viewport_roundtrips_on_a_canvas() {
    let mut value = serde_json::to_value(Document::new(Node::new(
        "map",
        Kind::Canvas(Canvas::default()),
    )))
    .unwrap();
    value["root"]["viewport"] = serde_json::json!({
        "content": {"width": 100.0, "height": 50.0},
        "transform": {"scale": 2.0, "x": 0.0, "y": 0.0, "anchor_x": 0.5, "anchor_y": 0.5}
    });
    let document: Document = serde_json::from_value(value.clone()).unwrap();
    document.validate().unwrap();
    assert_eq!(
        serde_json::to_value(document).unwrap()["root"]["viewport"],
        value["root"]["viewport"]
    );
}

/// The visual contract must reject mixing content projection with character-grid scrolling.
#[test]
fn visual_viewport_rejects_grid_and_preserves_declared_geometry() {
    let mut canvas = Canvas::default();
    canvas.grid = true;
    let mut node = Node::new("map", Kind::Canvas(canvas));
    node.viewport = Some(VisualViewport {
        content: Some(ContentSize {
            width: 100.,
            height: 50.,
        }),
        transform: Some(ContentTransform {
            scale: 2.4,
            ..Default::default()
        }),
    });
    assert!(Document::new(node).validate().is_err());
    let t = ContentTransform {
        scale: 2.4,
        ..Default::default()
    };
    let rect = t.project(
        crate::Rect {
            x: 0.,
            y: 0.,
            w: 100.,
            h: 50.,
        },
        ContentSize {
            width: 100.,
            height: 50.,
        },
        ContentSize {
            width: 400.,
            height: 300.,
        },
    );
    assert!((rect.x - 80.).abs() < 0.001 && (rect.y - 90.).abs() < 0.001);
    assert!((rect.w - 240.).abs() < 0.001 && (rect.h - 120.).abs() < 0.001);
}
