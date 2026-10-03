//! Contract regression cases are also shipped with the standalone SDK.
use super::*;

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
