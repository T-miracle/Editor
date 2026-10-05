//! Image declarations retain source authority and share one whole-document resource budget.
use super::*;

/// Binary file previews are versioned without manufacturing a text document revision.
#[test]
fn file_images_roundtrip_without_a_text_source() {
    let mut wire = serde_json::to_value(Document::new(Node::text("image", "pending"))).unwrap();
    wire["file"] = serde_json::json!({"id":"file-1","path":"picture.png","revision":3});
    wire["root"]["kind"] = serde_json::json!({
        "type":"file_image", "alt":"Picture", "sizing":"original_contain"
    });
    let document: Document = serde_json::from_value(wire).unwrap();
    assert!(document.source.is_none());
    document.validate().unwrap();
    let mut unbound = serde_json::to_value(document).unwrap();
    unbound.as_object_mut().unwrap().remove("file");
    let unbound: Document = serde_json::from_value(unbound).unwrap();
    assert!(unbound.validate().is_err());
}
use serde_json::json;

/// JSON enters the public SDK seam, so an absent node kind is a real compatibility failure.
fn image_document() -> Document {
    serde_json::from_value(json!({
        "version":1,"revision":1,
        "source":{"id":"memory","path":"notes/readme.sample","revision":7},
        "root":{"id":"image","kind":{"type":"image","source":"../assets/你好.png","alt":"示例"}}
    }))
    .expect("ui.images must recognize a portable image declaration")
}

/// Images preserve their URI and require a versioned source without upgrading the UI transport.
#[test]
fn ui_images_are_version_bound_portable_nodes() {
    let mut document = image_document();
    document.validate().unwrap();
    assert_eq!(document.version, VERSION);
    assert_eq!(document.root.theme_role(), "image");
    let encoded = serde_json::to_value(&document).unwrap();
    assert_eq!(encoded["root"]["kind"]["source"], "../assets/你好.png");
    document.source = None;
    assert!(document.validate().unwrap_err().contains("Document.source"));
}

/// URI and alternative text quotas are checked before allocating host resources.
#[test]
fn ui_images_bound_uri_and_alternative_text() {
    let mut document = image_document();
    for uri in [String::new(), "a".repeat(4097)] {
        let encoded = serde_json::to_value(&document).unwrap();
        let mut oversized = encoded;
        oversized["root"]["kind"]["source"] = json!(uri);
        let invalid: Document = serde_json::from_value(oversized).unwrap();
        assert!(invalid.validate().is_err());
    }
    let mut encoded = serde_json::to_value(&document).unwrap();
    encoded["root"]["kind"]["alt"] = json!("a".repeat(65537));
    document = serde_json::from_value(encoded).unwrap();
    assert!(document.validate().is_err());
}

/// The 64-image quota includes toolbar and dialog trees, rather than a separate quota per surface.
#[test]
fn ui_images_limit_the_complete_document_to_64_nodes() {
    let mut document = image_document();
    let prototype = document.root.clone();
    let mut children = Vec::new();
    for index in 0..64 {
        let mut image = prototype.clone();
        image.id = format!("image-{index}");
        children.push(image);
    }
    document.root = Node::column("images", children);
    document.validate().unwrap();
    document.editor_toolbar = Some(prototype.clone());
    assert!(
        document
            .validate()
            .unwrap_err()
            .contains("Image node quota")
    );
    document.editor_toolbar = None;
    document.dialog = Some(Dialog::new("dialog", "Images", prototype));
    assert!(
        document
            .validate()
            .unwrap_err()
            .contains("Image node quota")
    );
}
