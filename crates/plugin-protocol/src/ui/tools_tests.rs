//! Toolbar contributions cross the serialized public boundary with explicit target ownership.
use super::*;

/// One file tool describes its artwork, localization and state without host business enums.
fn file_tool_document() -> serde_json::Value {
    serde_json::json!({
        "version":1,"revision":5,
        "file":{"id":"opened-file","path":"photo.png","revision":3},
        "root":{"id":"scene","kind":{"type":"text","text":"Preview"}},
        "tools":[{
            "id":"view-fit","label":{"zh_cn":"完整查看","en":"Fit image"},
            "tooltip":{"zh_cn":"缩小至完整查看","en":"Contain the entire image"},
            "icon":{"light":"icons/fit.svg","dark":"icons/fit-dark.svg"},
            "target":{"kind":"file","version":{"id":"opened-file","path":"photo.png","revision":3}},
            "visible":true,"selected":true,"disabled":false,"order":10
        }]
    })
}

/// Round-trip preserves the guest declaration instead of silently discarding the toolbar metadata.
#[test]
fn file_tool_declarations_survive_the_public_transport() {
    let wire = file_tool_document();
    let document: Document = serde_json::from_value(wire.clone()).unwrap();
    document.validate().unwrap();
    assert_eq!(
        serde_json::to_value(&document).unwrap()["tools"],
        wire["tools"]
    );
}

/// Tool identities share the whole tree namespace and cannot borrow another file or package path.
#[test]
fn file_tools_reject_foreign_targets_and_unsafe_artwork() {
    for (pointer, invalid) in [
        ("/tools/0/target/version/id", "another-file"),
        ("/tools/0/icon/light", "../other-plugin/icon.svg"),
        ("/tools/0/id", "scene"),
        ("/tools/0/label/en", ""),
    ] {
        let mut wire = file_tool_document();
        *wire.pointer_mut(pointer).unwrap() = invalid.into();
        let document: Document = serde_json::from_value(wire).unwrap();
        assert!(document.validate().is_err(), "invalid {pointer}");
    }
}

/// A hidden, disabled, replaced, modal or retargeted tool cannot be activated by a delayed callback.
#[test]
fn tool_activation_revalidates_state_and_target() {
    let document: Document = serde_json::from_value(file_tool_document()).unwrap();
    let event = ToolEvent {
        revision: 5,
        tool: "view-fit".into(),
        target: document.tools[0].target.clone(),
    };
    document.validate_tool_event(&event).unwrap();
    let mut late = event.clone();
    late.revision = 4;
    assert_eq!(
        document.validate_tool_event(&late).unwrap_err().code,
        crate::api::ErrorCode::StaleRevision
    );
    for (visible, disabled) in [(false, false), (true, true)] {
        let mut changed = document.clone();
        changed.tools[0].visible = visible;
        changed.tools[0].disabled = disabled;
        assert!(changed.validate_tool_event(&event).is_err());
    }
    let mut retargeted = event;
    retargeted.target = ToolTarget::Window {
        panel: "another-window".into(),
    };
    assert!(document.validate_tool_event(&retargeted).is_err());
}
