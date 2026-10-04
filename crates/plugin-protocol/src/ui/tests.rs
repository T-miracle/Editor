//! Contract regression cases are also shipped with the standalone SDK.
use super::*;

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
