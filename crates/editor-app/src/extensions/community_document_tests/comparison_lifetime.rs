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
            // Cover both focus owners in one lifecycle fixture: unrelated controls keep their
            // keys, while an actually clicked pane must not retain focus after being unmounted.
            let unrelated_focus = if transition == "trust loss" {
                let bounds = visual.debug_bounds("document-diff-left").unwrap();
                let click = bounds.origin + gpui_kit::point(gpui_kit::px(20.), gpui_kit::px(60.));
                visual.simulate_mouse_down(click, gpui_kit::MouseButton::Left, Default::default());
                visual.simulate_mouse_up(click, gpui_kit::MouseButton::Left, Default::default());
                visual.update(|window, cx| {
                    assert!(left_editor.read(cx).focus_handle(cx).is_focused(window));
                });
                // A new public comparison replaces the focused old pane through normal
                // right-document activation; it must not leave the old left handle owning keys.
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
                visual.update(|window, cx| {
                    window.draw(cx).clear(cx);
                    assert!(
                        app.read(cx)
                            .editor
                            .read(cx)
                            .focus_handle(cx)
                            .is_focused(window)
                    );
                });
                visual.simulate_mouse_down(click, gpui_kit::MouseButton::Left, Default::default());
                visual.simulate_mouse_up(click, gpui_kit::MouseButton::Left, Default::default());
                visual.update(|window, cx| {
                    assert!(left_editor.read(cx).focus_handle(cx).is_focused(window));
                });
                None
            } else {
                Some(visual.update(|window, cx| {
                    let focus = cx.focus_handle();
                    window.focus(&focus, cx);
                    focus
                }))
            };
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
                if let Some(focus) = &unrelated_focus {
                    assert!(
                        focus.is_focused(window),
                        "automatic {transition} must preserve other focus"
                    );
                } else {
                    assert!(
                        owner.editor.read(cx).focus_handle(cx).is_focused(window),
                        "retiring the focused pane must return keys to the visible editor"
                    );
                }
            });
            assert!(visual.debug_bounds("document-diff-left").is_none());
            if transition == "disable" {
                manager.enable("generated-preview").unwrap();
            } else if transition == "trust loss" {
                // Selection and input must follow real keyboard routing after automatic retirement.
                visual.simulate_keystrokes("ctrl-a");
                visual.update(|_, cx| {
                    let editor = app.read(cx).editor.read(cx);
                    assert_eq!(editor.selected_range(), 0..editor.text().len());
                });
                visual.simulate_input("自动清理后右侧输入😀");
                visual.run_until_parked();
                visual.update(|_, cx| {
                    assert_eq!(
                        app.read(cx).editor.read(cx).value().to_string(),
                        "自动清理后右侧输入😀"
                    );
                    assert_eq!(
                        left_editor.read(cx).value().to_string(),
                        "另一份本地文本😀\r\n"
                    );
                });
            }
        }
        assert_eq!(std::fs::read_to_string(path).unwrap(), "original on disk");
        assert_eq!(
            std::fs::read_to_string(left_path).unwrap(),
            "另一份本地文本😀\r\n"
        );
    });
}
