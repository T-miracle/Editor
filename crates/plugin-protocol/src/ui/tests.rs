//! Contract regression cases are also shipped with the standalone SDK.
use super::*;

#[test]
fn canvas_chrome_rejects_ambiguous_items_and_unbounded_geometry() {
    let mut chrome = CanvasChrome {
        sidebar: Some(SideTabs {
            id: "tabs".into(),
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
        }),
        ..Default::default()
    };
    assert!(chrome.validate().is_ok());
    chrome.sidebar.as_mut().unwrap().width = f32::NAN;
    assert!(chrome.validate().is_err());
    chrome.sidebar.as_mut().unwrap().width = 180.;
    chrome.sidebar.as_mut().unwrap().selected = Some("removed".into());
    assert!(chrome.validate().is_err());
    chrome.sidebar.as_mut().unwrap().selected = None;
    let duplicate = chrome.sidebar.as_ref().unwrap().items[0].clone();
    chrome.sidebar.as_mut().unwrap().items.push(duplicate);
    assert!(chrome.validate().is_err());
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
