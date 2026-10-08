//! Delivered incremental scenes use the public installed-package and revision admission boundary.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, api},
};

/// Removing negotiation from the same independent guest rejects its first incremental activation.
#[test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn incremental_scene_requires_explicit_capability() {
    use std::io::{Cursor, Write};
    let root = tempfile::tempdir().unwrap();
    let package = Package::read(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/markdown.zip"),
    )
    .unwrap();
    let mut files = package.files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["api"]["required"]
        .as_object_mut()
        .unwrap()
        .remove("ui.incremental");
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
    let package = Package::from_bytes(&zip.finish().unwrap().into_inner()).unwrap();
    let mut manager = Manager::open(root.path().join("plugins"), Environment::default()).unwrap();
    let error = manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap_err();
    assert!(
        matches!(error.downcast_ref::<api::Failure>(), Some(failure) if failure.code == api::ErrorCode::CapabilityUnavailable),
        "{error:#}"
    );
    assert!(manager.live.is_empty());
}

/// One paragraph replacement preserves the rendered tail, then a hidden preview pauses derived content.
#[test]
#[ignore = "build markdown through scripts/build-plugins.ps1 first"]
fn delivered_incremental_scene_restores_content_and_suspends_hidden_derivation() {
    let root = tempfile::tempdir().unwrap();
    let package = Package::read(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/markdown.zip"),
    )
    .unwrap();
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let original = (0..100)
        .map(|index| format!("段落 {index} 正文。\n\n"))
        .collect::<String>();
    let edited = original.replacen("段落 50", "修改段落 50", 1);
    for (revision, text) in [(1, original), (2, edited.clone())] {
        file(&mut manager, revision);
        manager
            .event(
                "markdown",
                Some("preview".into()),
                api::Notification::Preview {
                    document: Some(api::DocumentVersion {
                        id: "open-note".into(),
                        path: "note.md".into(),
                        revision,
                    }),
                    text,
                },
            )
            .unwrap();
    }
    let scene = manager.live["markdown"].views["preview"].clone();
    let rendered = serde_json::to_string(scene.as_ref()).unwrap();
    assert!(rendered.contains("修改段落 50"));
    assert!(rendered.contains("段落 99 正文"));
    assert_eq!(scene.source.as_ref().unwrap().revision, 2);
    manager
        .event(
            "markdown",
            Some("preview".into()),
            api::Notification::Tool(plugin_runtime::plugin_protocol::ui::ToolEvent {
                tool: "display-source".into(),
                target: plugin_runtime::plugin_protocol::ui::ToolTarget::File {
                    version: scene.file.clone().unwrap(),
                },
                revision: scene.revision,
            }),
        )
        .unwrap();
    file(&mut manager, 3);
    manager
        .event(
            "markdown",
            Some("preview".into()),
            api::Notification::Preview {
                document: Some(api::DocumentVersion {
                    id: "open-note".into(),
                    path: "note.md".into(),
                    revision: 3,
                }),
                text: edited.replace("段落 99", "隐藏期间修改段落 99"),
            },
        )
        .unwrap();
    let hidden = &manager.live["markdown"].views["preview"];
    // A hidden pane cannot attach stale byte ranges to its current toolbar/document authority.
    hidden
        .root
        .visit(&mut |node| assert!(node.source_range.is_none()));
    assert_eq!(hidden.source.as_ref().unwrap().revision, 3);
    manager
        .event(
            "markdown",
            Some("preview".into()),
            api::Notification::Tool(plugin_runtime::plugin_protocol::ui::ToolEvent {
                tool: "display-split".into(),
                target: plugin_runtime::plugin_protocol::ui::ToolTarget::File {
                    version: hidden.file.clone().unwrap(),
                },
                revision: hidden.revision,
            }),
        )
        .unwrap();
    assert!(
        serde_json::to_string(manager.live["markdown"].views["preview"].as_ref())
            .unwrap()
            .contains("隐藏期间修改段落 99")
    );
}

/// Pair each memory revision with its real file contribution before the plugin binds display intent.
fn file(manager: &mut Manager, revision: u64) {
    manager
        .event(
            "markdown",
            Some("preview".into()),
            api::Notification::FilePreview {
                file: Some(api::FileContext {
                    version: api::FileVersion {
                        id: "file-note".into(),
                        path: "note.md".into(),
                        revision,
                    },
                    file_type: "md".into(),
                    text: Some(api::DocumentVersion {
                        id: "open-note".into(),
                        path: "note.md".into(),
                        revision,
                    }),
                }),
            },
        )
        .unwrap();
}
