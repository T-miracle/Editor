//! Exercise complete file layouts at the public serialized guest boundary.
use super::*;

/// Pane resizing is explicit portable geometry; wrapping or scalar controls cannot claim it.
#[test]
fn serialized_resizable_containers_keep_their_geometry_contract() {
    let wire = serde_json::json!({"version":1,"revision":1,"root":{"id":"panes","layout":{"resizable":true},"kind":{"type":"row","children":[{"id":"left","kind":{"type":"text","text":"A"}},{"id":"right","kind":{"type":"text","text":"B"}}]}}});
    let document: Document = serde_json::from_value(wire.clone()).unwrap();
    document.validate().unwrap();
    assert_eq!(
        serde_json::to_value(&document).unwrap()["root"]["layout"]["resizable"],
        true
    );
    let mut invalid = wire;
    invalid["root"]["layout"]["wrap"] = true.into();
    assert!(
        serde_json::from_value::<Document>(invalid)
            .unwrap()
            .validate()
            .is_err()
    );
}

/// Content defaults accept portable RGB tokens only, never unbounded or malformed theme data.
#[test]
fn content_defaults_are_bounded_rgb_roles() {
    let mut document = Document::new(Node::text("text", "content"));
    document
        .content_colors
        .insert("rich_text.foreground".into(), 0xabcdef);
    document.validate().unwrap();
    document
        .content_colors
        .insert("rich_text.foreground".into(), 0x1ffffff);
    assert!(document.validate().is_err());
    document.content_colors.clear();
    document.content_colors.insert("invalid role".into(), 0);
    assert!(document.validate().is_err());
    document.content_colors = (0..65)
        .map(|index| (format!("role{index}.foreground"), 0))
        .collect();
    assert!(document.validate().is_err());
}

/// Layout references reuse one exact document; duplicate/cross-file references cannot create sessions.
#[test]
fn native_editor_layout_references_are_exact_and_unique() {
    let wire = serde_json::json!({
        "version":1,"revision":2,"editor_layout":true,
        "source":{"id":"open-text","path":"notes.txt","revision":3},
        "root":{"id":"layout","kind":{"type":"row","children":[
            {"id":"editor","kind":{"type":"native_editor","document":{"id":"open-text","path":"notes.txt","revision":3}}},
            {"id":"detail","kind":{"type":"text","text":"Details"}}
        ]}}
    });
    let document: Document = serde_json::from_value(wire.clone()).unwrap();
    document.validate().unwrap();
    assert!(document.active_native_editor().is_some());
    let mut disabled = document.clone();
    disabled.root.disabled = true;
    assert!(disabled.active_native_editor().is_none());
    let mut invalid = wire.clone();
    invalid["root"]["kind"]["children"][0]["kind"]["document"]["id"] = "another-file".into();
    assert!(
        serde_json::from_value::<Document>(invalid)
            .unwrap()
            .validate()
            .is_err()
    );
    let mut duplicate = wire.clone();
    let mut second = duplicate["root"]["kind"]["children"][0].clone();
    second["id"] = "second-editor".into();
    duplicate["root"]["kind"]["children"]
        .as_array_mut()
        .unwrap()
        .push(second);
    assert!(
        serde_json::from_value::<Document>(duplicate)
            .unwrap()
            .validate()
            .is_err()
    );
    let mut unowned = wire;
    unowned["editor_layout"] = false.into();
    assert!(
        serde_json::from_value::<Document>(unowned)
            .unwrap()
            .validate()
            .is_err()
    );
    let mut dialog = document.clone();
    dialog.dialog = Some(Dialog::new(
        "modal",
        "Modal",
        document.root.find("editor").unwrap().clone(),
    ));
    dialog.dialog.as_mut().unwrap().content.id = "dialog-editor".into();
    assert!(dialog.validate().is_err());
}
