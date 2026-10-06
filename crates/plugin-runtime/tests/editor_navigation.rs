//! Independently built SDK guests opt into native link events at the real ZIP/Manager boundary.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{
        Environment,
        api::{DocumentVersion, ErrorCode, Failure, Notification},
        ui::{Action, Kind, UiEvent},
    },
};
use serde_json::{Value, json};
use std::io::{Cursor, Write};

const URI: &str = "https://example.com/help";

/// Only declarations and the ordinary UI asset change; the archive retains actual SDK/WASM dispatch.
fn package(edit: impl FnOnce(&mut Value, &mut Value)) -> Package {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/plugin-api-test/capability-example.zip");
    let mut files = Package::read(&path).unwrap().files;
    let mut manifest: Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["id"] = json!("navigation-fixture");
    manifest["name"] = json!("Independent native link event fixture");
    manifest["api"]["required"] = json!({
        "package.assets":"^1", "ui.native":"^1", "ui.richtext":"^1", "configuration":"^1"
    });
    manifest["api"]["optional"] = json!({});
    manifest["permissions"] = json!(["assets.read"]);
    manifest["settings_hook"] = json!(false);
    manifest["settings"] = json!({"label":{
        "title":"Fixture UI", "value_type":{"kind":"string","max_length":120},
        "default":"composable-ui", "scope":"user", "apply":"restart_instance"
    }});
    let mut document = json!({"version":1,"revision":1,"root":{
        "id":"rich", "kind":{"type":"rich_text",
            "html":"<p><a href=\"https://example.com/help\">帮助 / Help</a></p>"}
    }});
    edit(&mut manifest, &mut document);
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    files.insert(
        "composed-ui.json".into(),
        serde_json::to_vec(&document).unwrap(),
    );
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        archive
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&archive.finish().unwrap().into_inner()).unwrap()
}

/// Linked images retain the same metadata when the independent guest emits alternative text first.
fn kinds() -> [Value; 3] {
    [
        json!({"type":"rich_text", "html":"<p><a href=\"https://example.com/help\">帮助 / Help</a></p>"}),
        json!({"type":"image", "source":"missing.png", "alt":"帮助图片 / Help image"}),
        json!({"type":"text", "text":"图片替代文字 / Alternative text"}),
    ]
}

/// Declare only each read-only node's normal capabilities, plus its explicit accessible link metadata.
fn content(manifest: &mut Value, document: &mut Value, kind: &Value) {
    document["root"]["kind"] = kind.clone();
    document["root"]["links"] = json!([{"uri":URI,"label":"帮助 / Help"}]);
    if kind["type"] != "rich_text" {
        manifest["api"]["required"]
            .as_object_mut()
            .unwrap()
            .remove("ui.richtext");
    }
    if kind["type"] == "image" {
        manifest["api"]["required"]["ui.images"] = json!("^1");
        manifest["api"]["required"]["editor.documents"] = json!("^1");
        manifest["permissions"] = json!(["assets.read", "editor.read"]);
        manifest["panels"][0]["position"] = json!("editor");
        manifest["panels"][0]["file_extensions"] = json!(["sample"]);
    }
}

/// A real Preview notification binds the image after startup's ordinary alternative-text publication.
fn bind_image(manager: &mut Manager, kind: &Value) {
    if kind["type"] == "image" {
        let initial = manager.live["navigation-fixture"].views["welcome"].as_ref();
        assert!(matches!(initial.root.kind, Kind::Text { .. }));
        assert_eq!(initial.root.links.len(), 1);
        manager
            .event(
                "navigation-fixture",
                Some("welcome".into()),
                Notification::Preview {
                    document: Some(DocumentVersion {
                        id: "independent-source".into(),
                        path: "source.sample".into(),
                        revision: 7,
                    }),
                    text: "source\n".into(),
                },
            )
            .unwrap();
        assert!(
            manager.live["navigation-fixture"].views["welcome"]
                .source
                .is_some()
        );
    }
    let published = manager.live["navigation-fixture"].views["welcome"].as_ref();
    assert_eq!(serde_json::to_value(&published.root.kind).unwrap(), *kind);
    assert_eq!(published.root.links[0].uri, URI);
    assert_eq!(published.root.links[0].label, "帮助 / Help");
}

/// Target the actually published revision, so refusal cannot be mistaken for a stale view rejection.
fn link(manager: &mut Manager, uri: &str) -> anyhow::Result<()> {
    let revision = manager.live["navigation-fixture"].views["welcome"].revision;
    manager.event(
        "navigation-fixture",
        Some("welcome".into()),
        Notification::Ui(UiEvent {
            revision,
            node: "rich".into(),
            action: Action::Link { uri: uri.into() },
        }),
    )
}

/// Each read-only node can publish explicit links without acquiring editor.navigation/browser authority.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn richtext_link_events_publish_with_their_independent_capability() {
    for kind in kinds() {
        let package = package(|manifest, document| {
            content(manifest, document, &kind);
            manifest["api"]["required"]["ui.links"] = json!("^1");
            document["link_events"] = json!(true);
        });
        let root = tempfile::tempdir().unwrap();
        let mut manager =
            Manager::open(root.path().join("plugins"), Environment::default()).unwrap();
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
        bind_image(&mut manager, &kind);
        assert!(manager.live["navigation-fixture"].views["welcome"].link_events);
        if kind["type"] != "rich_text" {
            // Image/text routing accepts only the guest's declared destination, not arbitrary URIs.
            let error = link(&mut manager, "https://example.com/undeclared").unwrap_err();
            assert_eq!(
                error.downcast_ref::<Failure>().unwrap().code,
                ErrorCode::InvalidRequest
            );
        }
        link(&mut manager, URI).unwrap();
        assert!(
            manager
                .live
                .get_mut("navigation-fixture")
                .unwrap()
                .take_editor_requests()
                .is_empty()
        );
    }
}

/// Both click routing and inert metadata need ui.links; rich-text/image capabilities cannot supply it.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn richtext_cannot_publish_link_events_without_ui_links() {
    // Keep the original flag-only case alongside all three metadata cases with the flag still false.
    for kind in std::iter::once(None).chain(kinds().into_iter().map(Some)) {
        let package = package(|manifest, document| {
            if let Some(kind) = &kind {
                content(manifest, document, kind);
            } else {
                document["link_events"] = json!(true);
            }
        });
        let root = tempfile::tempdir().unwrap();
        let mut manager =
            Manager::open(root.path().join("plugins"), Environment::default()).unwrap();
        let error = manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap_err();
        let failure = error.downcast_ref::<Failure>().unwrap();
        assert_eq!(failure.code, ErrorCode::CapabilityUnavailable);
        assert!(failure.message.contains("ui.links"));
        assert!(manager.installed.is_empty() && manager.live.is_empty());
    }
}

/// Omitted flags preserve inert legacy markup and all explicit metadata, even with ui.links negotiated.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn richtext_links_remain_inert_with_the_default_flag() {
    for kind in std::iter::once(None).chain(kinds().into_iter().map(Some)) {
        let package = package(|manifest, document| {
            if let Some(kind) = &kind {
                content(manifest, document, kind);
                manifest["api"]["required"]["ui.links"] = json!("^1");
            }
        });
        let root = tempfile::tempdir().unwrap();
        let mut manager =
            Manager::open(root.path().join("plugins"), Environment::default()).unwrap();
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
        if let Some(kind) = &kind {
            bind_image(&mut manager, kind);
        }
        let before = manager.live["navigation-fixture"].views["welcome"].clone();
        assert!(!before.link_events);
        let error = link(&mut manager, URI).unwrap_err();
        assert_eq!(
            error.downcast_ref::<Failure>().unwrap().code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(manager.live["navigation-fixture"].views["welcome"], before);
        assert!(
            manager
                .live
                .get_mut("navigation-fixture")
                .unwrap()
                .take_editor_requests()
                .is_empty()
        );
    }
}
