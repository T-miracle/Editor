//! Real independent SDK packages exercise opt-in code requests at the ZIP/Manager publication boundary.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{
        Environment,
        api::{DocumentVersion, ErrorCode, Failure, Notification},
        ui::{Document, Kind, Node},
    },
};
use serde_json::{Value, json};
use std::io::{Cursor, Write};

const PLUGIN: &str = "code-highlighting-fixture";
const CODE: &str = "let value = 1;\r\n\t// 中文\n";
const LANGUAGE: &str = "independent-code-language";

/// Keep the actual externally built component while selecting only public declarations and its UI asset.
fn package(edit: impl FnOnce(&mut Value, &mut Value)) -> Package {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/plugin-api-test/capability-example.zip");
    let mut files = Package::read(&path).unwrap().files;
    let mut manifest: Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["id"] = json!(PLUGIN);
    manifest["name"] = json!("Independent native code fixture");
    manifest["scope"] = json!("workspace");
    manifest["api"]["required"] = json!({
        "package.assets":"^1", "ui.native":"^1", "ui.richtext":"^1",
        "ui.code_highlighting":"^1", "editor.documents":"^1", "configuration":"^1"
    });
    manifest["api"]["optional"] = json!({});
    manifest["permissions"] = json!(["assets.read", "editor.read"]);
    manifest["settings_hook"] = json!(false);
    manifest["settings"] = json!({"label":{
        "title":"Fixture UI", "value_type":{"kind":"string","max_length":120},
        "default":"composable-ui", "scope":"user", "apply":"restart_instance"
    }});
    manifest["panels"] = json!([{
        "id":"welcome", "title":"Independent code", "position":"editor",
        "file_extensions":["sample"]
    }]);
    manifest["commands"] = json!([{"id":"preview-probe", "title":"Publish fixture document"}]);
    let mut document = json!({"version":1, "revision":1, "code_highlighting":true,
        "root":{"id":"code", "kind":{"type":"code_block", "text":CODE, "language":LANGUAGE}}
    });
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

/// Install with exactly the package's declarations; no live Store or permission set is patched.
fn install(package: &Package) -> (tempfile::TempDir, Manager) {
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        root.path().join("plugins"),
        Environment {
            workspace: root.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(package, package.manifest.permissions.clone())
        .unwrap();
    (root, manager)
}

/// A source token names an in-memory editor entity, not a document inferred from a code-language label.
fn source() -> DocumentVersion {
    DocumentVersion {
        id: "source-1".into(),
        path: "unsaved.sample".into(),
        revision: 7,
    }
}

/// Preview supplies unsaved text through the public owned surface, never by reading a fixture source file.
fn preview(manager: &mut Manager, source: Option<DocumentVersion>) -> anyhow::Result<()> {
    let text = if source.is_some() {
        CODE.into()
    } else {
        String::new()
    };
    manager.event(
        PLUGIN,
        Some("welcome".into()),
        Notification::Preview {
            document: source,
            text,
        },
    )
}

/// Ordinary guest commands return the requested publication unchanged, including any obsolete source token.
fn publish(manager: &mut Manager, source: Option<DocumentVersion>) -> anyhow::Result<()> {
    let mut document =
        Document::new(Node::code_block("code", CODE, Some(LANGUAGE.into()))).revision(9);
    document.source = source;
    document.code_highlighting = true;
    manager.invoke_command(
        PLUGIN,
        "preview-probe",
        serde_json::to_value(document).unwrap(),
    )
}

/// Typed refusal is observed at the same boundary as publication, rather than from a private validator.
fn refusal(result: anyhow::Result<()>, expected: ErrorCode) {
    let error = result.unwrap_err();
    assert_eq!(
        error.downcast_ref::<Failure>().map(|failure| failure.code),
        Some(expected),
        "{error:#}"
    );
}

/// Legacy code remains inert without opting in or acquiring document/highlighting authority.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn code_highlighting_defaults_to_inert_without_extra_grants() {
    let package = package(|manifest, document| {
        manifest["api"]["required"]
            .as_object_mut()
            .unwrap()
            .remove("ui.code_highlighting");
        manifest["api"]["required"]
            .as_object_mut()
            .unwrap()
            .remove("editor.documents");
        manifest["permissions"] = json!(["assets.read"]);
        manifest["panels"][0]["position"] = json!("right");
        manifest["panels"][0]
            .as_object_mut()
            .unwrap()
            .remove("file_extensions");
        document
            .as_object_mut()
            .unwrap()
            .remove("code_highlighting");
    });
    let (_root, mut manager) = install(&package);
    let document = manager.live[PLUGIN].views["welcome"].as_ref();
    assert!(!document.code_highlighting && document.source.is_none());
    assert!(
        matches!(&document.root.kind, Kind::CodeBlock { text, language }
        if text == CODE && language.as_deref() == Some(LANGUAGE))
    );
    assert!(
        manager
            .live
            .get_mut(PLUGIN)
            .unwrap()
            .take_editor_requests()
            .is_empty()
    );
}

/// A legal owned request preserves readable text even when no package contributes its language highlighter.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn code_highlighting_publishes_without_a_language_provider() {
    let package = package(|_, _| {});
    let (_root, mut manager) = install(&package);
    assert!(!manager.live[PLUGIN].views["welcome"].code_highlighting);
    preview(&mut manager, Some(source())).unwrap();
    let document = manager.live[PLUGIN].views["welcome"].as_ref();
    assert!(document.code_highlighting);
    assert_eq!(document.source, Some(source()));
    assert!(
        matches!(&document.root.kind, Kind::CodeBlock { text, language }
        if text == CODE && language.as_deref() == Some(LANGUAGE))
    );
    assert_eq!(
        manager.installed.len(),
        1,
        "only the independent UI guest is installed"
    );
    assert!(
        manager
            .live
            .get_mut(PLUGIN)
            .unwrap()
            .take_editor_requests()
            .is_empty()
    );
}

/// A real bound opt-in publication cannot borrow the older rich-text capability as highlighting authority.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn code_highlighting_opt_in_requires_its_negotiated_capability() {
    let package = package(|manifest, _| {
        manifest["api"]["required"]
            .as_object_mut()
            .unwrap()
            .remove("ui.code_highlighting");
    });
    let (_root, mut manager) = install(&package);
    refusal(
        preview(&mut manager, Some(source())),
        ErrorCode::CapabilityUnavailable,
    );
    assert!(
        manager.live[PLUGIN].views.is_empty(),
        "an unauthorized output is never published"
    );
}

/// The public input gate protects unsaved code before it can reach an ungranted guest.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn code_highlighting_source_requires_editor_read() {
    let package = package(|manifest, _| {
        manifest["permissions"] = json!(["assets.read"]);
        // The package inspector already rejects ungranted editor surfaces; use a legal panel
        // to observe the public source-notification permission gate independently.
        manifest["panels"][0]["position"] = json!("right");
        manifest["panels"][0]
            .as_object_mut()
            .unwrap()
            .remove("file_extensions");
    });
    let (_root, mut manager) = install(&package);
    let before = manager.live[PLUGIN].views["welcome"].clone();
    refusal(
        preview(&mut manager, Some(source())),
        ErrorCode::PermissionDenied,
    );
    assert_eq!(manager.live[PLUGIN].views["welcome"], before);
    assert!(
        manager
            .live
            .get_mut(PLUGIN)
            .unwrap()
            .take_editor_requests()
            .is_empty()
    );
}

/// Read grants and the capability do not make a right-hand panel or an application instance a workspace preview.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn code_highlighting_source_requires_an_owned_workspace_editor_panel() {
    for application in [false, true] {
        let package = package(|manifest, _| {
            manifest["panels"][0]["position"] = json!("right");
            manifest["panels"][0]
                .as_object_mut()
                .unwrap()
                .remove("file_extensions");
            if application {
                manifest["scope"] = json!("application");
            }
        });
        let (_root, mut manager) = install(&package);
        let before = manager.live[PLUGIN].views["welcome"].clone();
        refusal(
            preview(&mut manager, Some(source())),
            ErrorCode::PermissionDenied,
        );
        assert_eq!(manager.live[PLUGIN].views["welcome"], before);
    }
}

/// A guest's ordinary command cannot manufacture code authority by opting in without any source.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn code_highlighting_opt_in_without_source_is_not_published() {
    let package = package(|_, _| {});
    let (_root, mut manager) = install(&package);
    refusal(publish(&mut manager, None), ErrorCode::InvalidRequest);
    assert!(manager.live[PLUGIN].views.is_empty());
}

/// Opt-in is additive: the original CodeBlock rendering contract must still be negotiated independently.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn code_highlighting_does_not_replace_the_richtext_capability() {
    let package = package(|manifest, _| {
        manifest["api"]["required"]
            .as_object_mut()
            .unwrap()
            .remove("ui.richtext");
    });
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(root.path().join("plugins"), Environment::default()).unwrap();
    refusal(
        manager.install(&package, package.manifest.permissions.clone()),
        ErrorCode::CapabilityUnavailable,
    );
    assert!(manager.installed.is_empty() && manager.live.is_empty());
}

/// Even a currently empty code scene must negotiate both contracts before opting in.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn code_highlighting_text_scene_still_requires_richtext() {
    let package = package(|manifest, document| {
        manifest["api"]["required"]
            .as_object_mut()
            .unwrap()
            .remove("ui.richtext");
        document["root"] =
            json!({"id":"waiting", "kind":{"type":"text", "text":"Waiting for code"}});
    });
    let (_root, mut manager) = install(&package);
    refusal(
        preview(&mut manager, Some(source())),
        ErrorCode::CapabilityUnavailable,
    );
    assert!(manager.live[PLUGIN].views.is_empty());
}

/// Real WASM output cannot revive an edited, closed or reopened source merely because a path/revision matches.
#[test]
#[ignore = "build current capability-example through the host SDK first"]
fn code_highlighting_late_publication_cannot_reclaim_changed_source() {
    let original = source();
    for next in [
        Some(DocumentVersion {
            revision: 8,
            ..original.clone()
        }),
        Some(DocumentVersion {
            id: "reopened-source".into(),
            ..original.clone()
        }),
        None,
    ] {
        let package = package(|_, _| {});
        let (_root, mut manager) = install(&package);
        preview(&mut manager, Some(original.clone())).unwrap();
        preview(&mut manager, next.clone()).unwrap();
        assert_eq!(manager.live[PLUGIN].views["welcome"].source, next);
        refusal(
            publish(&mut manager, Some(original.clone())),
            ErrorCode::StaleRevision,
        );
        assert!(
            manager.live[PLUGIN].views.is_empty(),
            "old code output cannot become the current scene"
        );
    }
}
