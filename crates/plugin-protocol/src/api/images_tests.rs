//! Image input metadata uses the public JSON contract while encoded pixels remain host-owned.
use super::*;
use serde_json::json;

/// An ordinary SDK request can name an opaque input without adding image bytes to its transport.
#[test]
fn image_input_save_operation_crosses_json_as_metadata_only() {
    let operation = json!({"kind":"save_image_input","input":{
        "instance":"owner","scope":"workspace","resource":1},"name":"attachment.png"});
    let parsed: EditorOperation = serde_json::from_value(operation.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), operation);
}

/// Encoded formats and canonical suffixes are explicit; source file names cannot choose a different type.
#[test]
fn image_input_formats_and_receipts_roundtrip_with_source_identity() {
    for (format, extension) in [
        (ImageFormat::Png, "png"),
        (ImageFormat::Jpeg, "jpg"),
        (ImageFormat::Gif, "gif"),
        (ImageFormat::Webp, "webp"),
        (ImageFormat::Svg, "svg"),
    ] {
        assert_eq!(format.extension(), extension);
        let handle = ResourceHandle {
            instance: "owner".into(),
            scope: "workspace".into(),
            resource: 1,
        };
        let document = DocumentVersion {
            id: "file".into(),
            path: "notes/a.sample".into(),
            revision: 7,
        };
        let input = Notification::ImageInput {
            document: document.clone(),
            selection: TextRange { start: 0, end: 6 },
            images: vec![ImageInput {
                handle: handle.clone(),
                format,
                byte_len: 8 * 1024 * 1024,
            }],
        };
        let encoded = serde_json::to_vec(&input).unwrap();
        assert!(encoded.len() < 512);
        let decoded: Notification = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(
            serde_json::to_value(decoded).unwrap(),
            serde_json::to_value(input).unwrap()
        );
        let receipt = EditorValue::ImageSaved {
            input: handle,
            document,
            name: format!("img.{extension}"),
        };
        let decoded: EditorValue =
            serde_json::from_value(serde_json::to_value(&receipt).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(decoded).unwrap(),
            serde_json::to_value(receipt).unwrap()
        );
    }
}

/// A declaration needs source authority; older plain UI documents keep their additive defaults.
#[test]
fn image_input_declaration_requires_a_document_source() {
    let value = json!({"version":1,"revision":0,"editor_image_input":true,
        "root":{"id":"body","kind":{"type":"text","text":"preview"}}});
    let document: crate::ui::Document = serde_json::from_value(value).unwrap();
    assert!(
        document.validate().is_err(),
        "image input cannot borrow ambient editor authority"
    );
    assert!(
        crate::ui::Document::new(crate::ui::Node::text("plain", "plain"))
            .validate()
            .is_ok()
    );
}
