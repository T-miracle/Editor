//! The shipped XML component supplies structure through public Manager, without a native LSP.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, api::DocumentVersion, language::SourceSnapshot},
};
use serde_json::json;
use std::{
    io::{Cursor, Write},
    path::PathBuf,
    sync::Arc,
};

/// Keep the actual WASM/resources but remove process declarations; structure must be an independent capability.
fn structure_package() -> Package {
    let original = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/xml.zip"),
    )
    .unwrap();
    let mut manifest = serde_json::to_value(&original.manifest).unwrap();
    manifest["language_servers"] = json!([]);
    manifest["services"] = json!({});
    manifest["structure_providers"] = json!([{"id":"structure", "language":"xml"}]);
    manifest["permissions"] = json!(["editor.read"]);
    manifest["api"]["required"] = json!({"configuration":"^1", "language.structure":"^1"});
    let mut files = original.files;
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
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

/// Element coverage, exact definition bytes and separate comment folds are observable package behavior.
#[test]
#[ignore = "build actual XML ZIP with the current public SDK first"]
fn xml_structure_is_independent_and_preserves_definition_ranges() {
    let workspace = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    let package = structure_package();
    let mut manager = Manager::open(
        store.path().into(),
        Environment {
            workspace: workspace.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    assert!(
        manager.language_services().is_empty(),
        "structure alone must not manufacture an LSP"
    );
    let provider = manager.structure_providers()["xml/structure"]
        .as_ref()
        .unwrap()
        .clone();
    let source = "<root>\n  <!-- 注释🙂\n       第二行 -->\n  <item id=\"中文🙂\"><item name=\"child\"/></item>\n</root>";
    let snapshot = SourceSnapshot {
        document: DocumentVersion {
            id: "structure-test".into(),
            path: "tree.xml".into(),
            revision: 7,
        },
        text: source.into(),
    };
    let result = provider.describe(snapshot.clone()).unwrap();
    assert_eq!(result.proposal.nodes.len(), 1);
    let root = &result.proposal.nodes[0];
    assert_eq!(root.name, "root");
    assert_eq!(root.range.end, source.len());
    let item = &root.children[0];
    assert!(item.name.contains("中文🙂"));
    assert_eq!(&source[item.definition.start..item.definition.end], "item");
    assert_eq!(
        &source[item.range.start..item.range.end],
        "<item id=\"中文🙂\"><item name=\"child\"/></item>"
    );
    assert!(item.children[0].name.contains("child"));
    assert!(
        result
            .proposal
            .folds
            .iter()
            .any(|range| source[range.start..range.end].starts_with("<!--"))
    );
    assert!(result.icons.contains_key("icons/element.svg"));
    manager.disable("xml").unwrap();
    assert!(!provider.is_active());
    assert!(provider.describe(snapshot).is_err());
}

/// The shipped guest's deepest legal output must execute, validate, and drop on an ordinary production-sized stack.
#[test]
#[ignore = "build actual XML ZIP with the current public SDK first"]
fn xml_structure_deep_documents_stay_bounded_on_two_mib_stack() {
    std::thread::Builder::new()
        .name("actual-deep-xml-structure".into())
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let workspace = tempfile::tempdir().unwrap();
            let store = tempfile::tempdir().unwrap();
            let package = structure_package();
            let mut manager = Manager::open(
                store.path().into(),
                Environment {
                    workspace: workspace.path().display().to_string(),
                    ..Default::default()
                },
            )
            .unwrap();
            manager
                .install(&package, package.manifest.permissions.clone())
                .unwrap();
            assert!(manager.language_services().is_empty());
            let provider = manager.structure_providers()["xml/structure"]
                .as_ref()
                .unwrap()
                .clone();
            for depth in [60, 127, 128] {
                let text = "<层>\n".repeat(depth) + &"</层>\n".repeat(depth);
                let source = SourceSnapshot {
                    document: DocumentVersion {
                        id: "deep-xml".into(),
                        path: "deep.xml".into(),
                        revision: depth as u64,
                    },
                    text,
                };
                let result = provider.describe(source.clone()).unwrap();
                let mut nodes = result.proposal.nodes.as_slice();
                let mut count = 0;
                while let Some(node) = nodes.first() {
                    assert_eq!(nodes.len(), 1);
                    assert_eq!(
                        &source.text[node.definition.start..node.definition.end],
                        "层"
                    );
                    count += 1;
                    nodes = &node.children;
                }
                assert_eq!(count, depth.min(127));
                if depth <= 127 {
                    assert_eq!(result.proposal.folds.len(), depth);
                } else {
                    // A source beyond the parser budget remains a bounded incomplete tree, without speculative folds.
                    assert!(result.proposal.folds.is_empty());
                }
                assert!(provider.is_active());
                drop(result);
            }
            // Oversize input fails before WASM dispatch and must not poison or silently replace its healthy lease.
            let oversized = SourceSnapshot {
                document: DocumentVersion {
                    id: "deep-xml".into(),
                    path: "deep.xml".into(),
                    revision: 200,
                },
                text: " ".repeat(1024 * 1024 + 1),
            };
            assert!(provider.describe(oversized).is_err());
            assert!(provider.is_active());
            let unchanged = manager.structure_providers()["xml/structure"]
                .as_ref()
                .unwrap()
                .clone();
            assert!(Arc::ptr_eq(&provider, &unchanged));
        })
        .unwrap()
        .join()
        .unwrap();
}
