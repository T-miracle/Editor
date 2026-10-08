//! Icon admission uses a neutral independent SDK package and the public Manager shared by all UI.
use super::*;
use plugin_runtime::plugin_protocol::{
    api::{ErrorCode, Failure},
    ui,
};

const SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\"><path d=\"M4 4h16v16H4Z\"/></svg>";

/// Declare only the native icon seam, without any language, source, image, canvas or editor capability.
fn icon_package(svg: &str, negotiated: bool) -> Package {
    package_with_ui(
        |manifest| {
            manifest["id"] = json!("icon-fixture");
            manifest["api"]["required"] =
                json!({"package.assets":"^1", "ui.native":"^1", "configuration":"^1"});
            manifest["api"]["optional"] = json!({});
            if negotiated {
                manifest["api"]["required"]["ui.icons"] = json!("^1");
            }
            manifest["permissions"] = json!(["assets.read"]);
            manifest["settings_hook"] = json!(false);
            manifest["settings"]["label"]["default"] = json!("composable-ui");
        },
        |tree| {
            tree["root"] =
                serde_json::to_value(ui::Node::button("icon", "图标操作").icon(svg)).unwrap();
        },
    )
}

/// An accepted icon is preserved as geometric data; omitted negotiation rejects publication atomically.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn native_icons_require_independent_negotiation_and_preserve_artwork() {
    for negotiated in [false, true] {
        let package = icon_package(SVG, negotiated);
        let root = tempfile::tempdir().unwrap();
        let mut manager =
            Manager::open(root.path().join("plugins"), Environment::default()).unwrap();
        let result = manager.install(&package, package.manifest.permissions.clone());
        if negotiated {
            result.unwrap();
            let document = &manager.live["icon-fixture"].views["welcome"];
            assert_eq!(document.root.button_icon.as_deref(), Some(SVG));
            document
                .validate_event(&ui::UiEvent {
                    revision: document.revision,
                    node: "icon".into(),
                    action: ui::Action::Click,
                })
                .unwrap();
        } else {
            assert_eq!(
                result.unwrap_err().downcast_ref::<Failure>().unwrap().code,
                ErrorCode::CapabilityUnavailable
            );
            assert!(manager.installed.is_empty() && manager.live.is_empty());
        }
    }
}

/// Scripts, href namespaces, paint URLs and malformed XML never reach a native renderer.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn native_icons_reject_ambient_resources_and_active_svg() {
    for svg in [
        "<svg><script>alert(1)</script></svg>",
        "<svg><image href=\"https://example.com/image.png\"/></svg>",
        "<svg xmlns:x=\"urn:x\"><path x:href=\"local.svg\"/></svg>",
        "<svg><path fill=\"url(https://example.com/paint)\"/></svg>",
        "<svg><path style=\"fill:red\"/></svg>",
        "<!DOCTYPE svg [<!ENTITY x 'text'>]><svg/>",
        "<svg><text>Text</text></svg>",
        "<svg",
    ] {
        let package = icon_package(svg, true);
        let root = tempfile::tempdir().unwrap();
        let mut manager =
            Manager::open(root.path().join("plugins"), Environment::default()).unwrap();
        let error = manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<Failure>().unwrap().code,
            ErrorCode::InvalidRequest,
            "{svg}"
        );
        assert!(manager.installed.is_empty() && manager.live.is_empty());
    }
}
