//! Completed comparisons retain their initiating instance authority even with two local sources.
use super::*;

/// Local text survives owner retirement, while its owned view and temporary restrictions do not.
#[gpui::test]
#[ignore = "package plugins/generated-preview with the current host first"]
fn local_comparison_observes_owner_replacement_disable_and_trust_loss(cx: &mut TestAppContext) {
    crate::tests::with_shortcut_editor(cx, false, Vec::new(), |visual, app, path| {
        let root = tempfile::tempdir().unwrap();
        let package = Package::read(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/community-api/generated-preview-0.1.1.zip"),
        )
        .unwrap();
        let mut manager = Manager::open(
            root.path().into(),
            Environment {
                workspace: path.parent().unwrap().display().to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
        let right = visual.update(|_, cx| app.read(cx).plugin_document_info(0, cx).unwrap());
        let left_path = path.parent().unwrap().join("other.txt");
        std::fs::write(&left_path, "另一份本地文本😀\r\n").unwrap();
        let api::EditorValue::DocumentOpened(left) = invoke_for(
            visual,
            &app,
            &mut manager,
            "generated-preview",
            json!({"kind":"open_document", "resource":{"kind":"local", "path":"other.txt"}}),
        )
        .unwrap() else {
            panic!("local text must open")
        };
        let left_editor =
            visual.update(|_, cx| app.read(cx).tabs[1].text.as_ref().unwrap().editor.clone());
        let foreign_marks = visual.update(|_, cx| {
            left_editor.update(cx, |editor, cx| {
                editor.create_range_decorations_collection(
                    vec![gpui_base::input::RangeDecoration::new(0..3)],
                    cx,
                )
            })
        });
        let mut files = package.files.clone();
        let mut manifest: Json = serde_json::from_slice(&files["manifest.json"]).unwrap();
        manifest["version"] = json!("0.1.2");
        files.insert(
            "manifest.json".into(),
            serde_json::to_vec(&manifest).unwrap(),
        );
        let replacement = Package::from_files(files.clone()).unwrap();
        files.insert(
            "generated-preview.wasm".into(),
            vec![0, 97, 115, 109, 13, 0, 1, 0],
        );
        let failed = Package::from_files(files).unwrap();

        // One real consumer covers the three lifecycle transitions without duplicating source/input tests.
        for transition in ["replacement", "disable", "trust loss"] {
            let prior_readonly = transition == "disable";
            visual.update(|_, cx| {
                left_editor.update(cx, |editor, cx| editor.set_readonly(prior_readonly, cx))
            });
            assert!(matches!(
                invoke_for(
                    visual,
                    &app,
                    &mut manager,
                    "generated-preview",
                    json!({"kind":"compare_documents", "left":left.document, "right":right.document})
                )
                .unwrap(),
                api::EditorValue::DocumentsCompared { .. }
            ));
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert!(visual.debug_bounds("document-diff-left").is_some());
            if transition == "replacement" {
                assert!(
                    manager
                        .install(&failed, failed.manifest.permissions.clone())
                        .is_err()
                );
                visual.update(|window, cx| window.draw(cx).clear(cx));
                assert!(
                    visual.debug_bounds("document-diff-left").is_some(),
                    "failed preparation keeps the old owner's comparison"
                );
            }
            // Automatic cleanup cannot steal a separate dialog/control's keyboard ownership.
            let unrelated_focus = visual.update(|window, cx| {
                let focus = cx.focus_handle();
                window.focus(&focus, cx);
                focus
            });
            match transition {
                "replacement" => {
                    manager
                        .install(&replacement, replacement.manifest.permissions.clone())
                        .unwrap();
                }
                "disable" => manager.disable("generated-preview").unwrap(),
                _ => manager.set_workspace_trust(false).unwrap(),
            }
            visual.update(|_, cx| app.update(cx, |_, cx| cx.notify()));
            visual.run_until_parked();
            visual.update(|window, cx| {
                window.draw(cx).clear(cx);
                let owner = app.read(cx);
                assert!(
                    owner.document_comparison.is_none(),
                    "{transition} must retire a completed local comparison"
                );
                assert_eq!(owner.tabs.len(), 2, "local text sessions remain open");
                assert_eq!(
                    left_editor.read(cx).presentation().is_readonly(),
                    prior_readonly
                );
                assert_eq!(foreign_marks.get_ranges(cx), vec![0..3]);
                assert!(
                    unrelated_focus.is_focused(window),
                    "automatic {transition} must preserve other focus"
                );
            });
            assert!(visual.debug_bounds("document-diff-left").is_none());
            if transition == "disable" {
                manager.enable("generated-preview").unwrap();
            }
        }
        assert_eq!(std::fs::read_to_string(path).unwrap(), "original on disk");
        assert_eq!(
            std::fs::read_to_string(left_path).unwrap(),
            "另一份本地文本😀\r\n"
        );
    });
}
