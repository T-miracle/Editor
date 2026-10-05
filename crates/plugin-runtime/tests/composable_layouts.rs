//! Actual independently built guest validates layout negotiation, source epochs and auxiliary limits.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, api, ui},
};
use std::io::{Cursor, Write};

/// Manifest variants reuse the real public-SDK component rather than bypassing package admission.
fn package(variant: &str) -> Package {
    let mut files = Package::read(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-layout-test/layout-example.zip"),
    )
    .expect("build-layout-example.ps1 first")
    .files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    if variant == "unnegotiated" {
        manifest["api"]["required"]
            .as_object_mut()
            .unwrap()
            .remove("editor.layout");
    }
    if variant == "auxiliary" {
        manifest["panels"][0]["auxiliary"] = true.into();
    }
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&zip.finish().unwrap().into_inner()).unwrap()
}

/// Only a negotiated provider publishes exact editor references; stale ingress never rewinds its authority.
#[test]
#[ignore = "build actual independent fixture with scripts/build-layout-example.ps1 first"]
fn independent_layout_guest_obeys_negotiation_and_source_ownership() {
    let workspace = tempfile::tempdir().unwrap();
    for (variant, expected) in [
        ("normal", None),
        ("unnegotiated", Some(api::ErrorCode::CapabilityUnavailable)),
        ("auxiliary", Some(api::ErrorCode::PermissionDenied)),
    ] {
        let data = tempfile::tempdir().unwrap();
        let pkg = package(variant);
        let mut manager = Manager::open(
            data.path().to_path_buf(),
            Environment {
                workspace: workspace.path().display().to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        manager
            .install(&pkg, pkg.manifest.permissions.clone())
            .unwrap();
        let source = api::DocumentVersion {
            id: "native-text".into(),
            path: "notes.layout".into(),
            revision: 2,
        };
        let result = manager.event(
            "layout-example",
            Some("layout".into()),
            api::Notification::Preview {
                document: Some(source.clone()),
                text: "draft".into(),
            },
        );
        if let Some(code) = expected {
            assert_eq!(
                result
                    .unwrap_err()
                    .downcast_ref::<api::Failure>()
                    .unwrap()
                    .code,
                code
            );
            continue;
        }
        result.unwrap();
        let doc = &manager.live["layout-example"].views["layout"];
        assert!(doc.editor_layout);
        let mut reference = None;
        doc.root.visit(&mut |node| {
            if let ui::Kind::NativeEditor { document } = &node.kind {
                reference = Some(document.clone());
            }
        });
        assert_eq!(reference, Some(source.clone()));
        let mut old = source;
        old.revision = 1;
        let error = manager
            .event(
                "layout-example",
                Some("layout".into()),
                api::Notification::Preview {
                    document: Some(old),
                    text: "old".into(),
                },
            )
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<api::Failure>().unwrap().code,
            api::ErrorCode::StaleRevision
        );
        assert!(manager.live.contains_key("layout-example"));
        let stale = api::Notification::Ui(ui::UiEvent {
            revision: 0,
            node: "layout-content".into(),
            action: ui::Action::Click,
        });
        assert_eq!(
            manager
                .event("layout-example", Some("layout".into()), stale)
                .unwrap_err()
                .downcast_ref::<api::Failure>()
                .unwrap()
                .code,
            api::ErrorCode::StaleRevision
        );
        manager
            .event(
                "layout-example",
                Some("layout".into()),
                api::Notification::Preview {
                    document: None,
                    text: String::new(),
                },
            )
            .unwrap();
        assert!(!manager.live["layout-example"].views["layout"].editor_layout);
    }
}
