//! Two independent SDK consumers exercise visible comparison panes and owned-resource retirement.
use super::*;

/// Both independent SDK consumers render native panes, preserve unsaved text and own only their resources.
#[gpui::test]
#[ignore = "package plugins/history-preview and plugins/generated-preview with the current host first"]
fn community_consumers_compare_visible_panes_and_retire_resources(cx: &mut TestAppContext) {
    crate::tests::with_shortcut_editor(cx, false, Vec::new(), |visual, app, path| {
        let (_runtime, mut manager) = manager(path.parent().unwrap());
        let package = Package::read(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/community-api/generated-preview-0.1.1.zip"),
        )
        .unwrap();
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
        // The generic local opener preserves literal URI-looking characters as filesystem names.
        let literal_path = path.parent().unwrap().join("literal#%20.txt");
        std::fs::write(&literal_path, "本地😀\r\n").unwrap();
        let api::EditorValue::DocumentOpened(local) = invoke_for(
            visual,
            &app,
            &mut manager,
            "generated-preview",
            json!({"kind":"open_document", "resource":{"kind":"local", "path":"literal#%20.txt"}}),
        )
        .unwrap() else {
            panic!("local opener expected")
        };
        let api::ResourceIdentity::Local { path: identity } = &local.resource else {
            panic!("literal local identity expected")
        };
        assert!(identity.ends_with("literal#%20.txt"));
        let api::EditorValue::DocumentSnapshot(snapshot) = invoke_for(
            visual,
            &app,
            &mut manager,
            "generated-preview",
            json!({"kind":"read_document", "document":local.document}),
        )
        .unwrap() else {
            panic!("local snapshot expected")
        };
        assert_eq!(snapshot.text, "本地😀\r\n");
        visual.update(|window, cx| {
            app.update(cx, |owner, cx| {
                let opened_path = owner.active_path.clone().unwrap();
                owner.close_tab(opened_path, window, cx);
            })
        });
        // Read bounds are checked against a real large session before allocating its response.
        // Creating the local source keeps command inputs within their separate 64 KiB budget.
        std::fs::write(&literal_path, "x\n".repeat(128 * 1024 + 1)).unwrap();
        let api::EditorValue::DocumentOpened(bounded) = invoke_for(
            visual,
            &app,
            &mut manager,
            "generated-preview",
            json!({"kind":"open_document", "resource":local.resource}),
        )
        .unwrap() else {
            panic!("large local opener expected")
        };
        assert_eq!(
            invoke_for(
                visual,
                &app,
                &mut manager,
                "generated-preview",
                json!({"kind":"read_document", "document":bounded.document})
            )
            .unwrap_err()
            .code,
            api::ErrorCode::LimitExceeded
        );
        let api::EditorValue::DocumentSnapshot(snapshot) = invoke_for(visual, &app, &mut manager, "generated-preview", json!({"kind":"read_document", "document":bounded.document, "range":{"unit":"bytes", "start":0, "end":1}})).unwrap() else { panic!("bounded range expected") };
        assert_eq!(snapshot.text, "x");
        visual.update(|window, cx| {
            app.update(cx, |owner, cx| {
                let opened_path = owner.active_path.clone().unwrap();
                owner.close_tab(opened_path, window, cx);
            })
        });
        std::fs::remove_file(&literal_path).unwrap();
        visual.run_until_parked();
        visual.simulate_keystrokes("ctrl-a");
        visual.simulate_input("current 中😀\r\nline two");
        visual.run_until_parked();
        run_consumer(
            visual,
            &app,
            &mut manager,
            "history-preview",
            "compare-history",
        );
        let left = visual
            .debug_bounds("document-diff-left")
            .expect("left comparison must be painted");
        let right = visual
            .debug_bounds("document-diff-right")
            .expect("right comparison must be painted");
        assert!(left.size.width > gpui_kit::px(100.) && right.size.width > gpui_kit::px(100.));
        assert!(left.right() <= right.left());
        // A different feature owns this mark. Theme refresh and comparison disposal must preserve it.
        let foreign_marks = visual.update(|_, cx| {
            app.read(cx).tabs[1]
                .text
                .as_ref()
                .unwrap()
                .editor
                .clone()
                .update(cx, |editor, cx| {
                    editor.create_range_decorations_collection(
                        vec![gpui_base::input::RangeDecoration::new(0..3)],
                        cx,
                    )
                })
        });
        // Read the same native role, label and value used by accessibility, across real repaints.
        // TestWindow does not activate AccessKit; Windows UIA is separately checked in native QA.
        let original_locale = rust_i18n::locale().to_string();
        for locale in ["zh-CN", "en"] {
            rust_i18n::set_locale(locale);
            for dark in [false, true] {
                visual.update(|_, cx| {
                    crate::theme::apply_theme(crate::theme::builtin_theme(dark), cx)
                });
                for scale in [1., 1.5] {
                    visual.simulate_scale_factor_change(scale);
                    visual.update(|window, cx| {
                        window.refresh();
                        app.update(cx, |_, cx| cx.notify());
                    });
                    visual.run_until_parked();
                    visual.update(|window, cx| window.draw(cx).clear(cx));
                    let left = visual.debug_bounds("document-diff-left").unwrap();
                    let right = visual.debug_bounds("document-diff-right").unwrap();
                    assert!(left.size.width > px(100.) && right.size.width > px(100.));
                    assert!(left.right() <= right.left());
                    visual.update(|window, cx| {
                        let nodes = gpui_base::test_support::snapshots(window);
                        let input = nodes
                            .iter()
                            .find(|node| {
                                node.role() == Some(gpui_kit::Role::Document)
                                    && node.label().is_some_and(|label| {
                                        label.contains(t!("editor.diff_left").as_ref())
                                            && label.contains(t!("editor.readonly_label").as_ref())
                                    })
                            })
                            .expect("left source must expose a named readonly native document");
                        assert!(input.visible());
                        assert_eq!(input.value(), Some("历史版本😀\r\nprevious line\r\n"));
                        assert!(
                            nodes.iter().any(|node| {
                                node.role() == Some(gpui_kit::Role::Group)
                                    && node.label().is_some_and(|label| {
                                        label.contains(t!("editor.diff_right").as_ref())
                                    })
                                    && node.visible()
                            }),
                            "right source must expose its accessible name"
                        );
                        assert!(
                            nodes.iter().any(|node| {
                                node.role() == Some(gpui_kit::Role::Button)
                                    && node.label() == Some(t!("editor.close_diff").as_ref())
                                    && node.visible()
                            }),
                            "comparison close must be a labelled native button"
                        );
                        assert_eq!(foreign_marks.get_ranges(cx), vec![0..3]);
                    });
                }
            }
        }
        rust_i18n::set_locale(&original_locale);
        visual.simulate_scale_factor_change(1.);
        visual.update(|window, cx| {
            window.refresh();
            app.update(cx, |_, cx| cx.notify());
        });
        visual.run_until_parked();
        let initial = visual.update(|_, cx| {
            let owner = app.read(cx);
            assert_eq!(owner.tabs.len(), 2);
            owner.plugin_document_info(1, cx).unwrap()
        });
        let click = left.origin + gpui_kit::point(gpui_kit::px(20.), gpui_kit::px(60.));
        visual.simulate_mouse_down(click, gpui_kit::MouseButton::Left, Default::default());
        visual.simulate_mouse_up(click, gpui_kit::MouseButton::Left, Default::default());
        visual.update(|window, cx| {
            assert!(
                app.read(cx).comparison_readonly_has_focus(window, cx),
                "actual left pane must own focus"
            )
        });
        visual.simulate_keystrokes("ctrl-a");
        visual.simulate_input("forbidden");
        visual.simulate_keystrokes("ctrl-s");
        visual.run_until_parked();
        visual.update(|_, cx| {
            let owner = app.read(cx);
            assert_eq!(
                owner.tabs[1]
                    .text
                    .as_ref()
                    .unwrap()
                    .editor
                    .read(cx)
                    .value()
                    .to_string(),
                "历史版本😀\r\nprevious line\r\n"
            );
            assert!(
                owner.tabs[0].is_dirty(),
                "left Save cannot save the active right document"
            );
            assert_eq!(owner.status, t!("status.readonly_document").to_string());
        });
        assert_eq!(std::fs::read_to_string(path).unwrap(), "original on disk");
        // Closing a focused readonly pane must return keyboard ownership to the live right session.
        // Click the actual Base button hit region so a stale focus handle cannot pass this check.
        let close = visual.update(|window, _| {
            gpui_base::test_support::snapshots(window)
                .into_iter()
                .find(|node| {
                    node.role() == Some(gpui_kit::Role::Button)
                        && node.label() == Some(t!("editor.close_diff").as_ref())
                        && node.visible()
                })
                .expect("visible comparison close button")
                .bounds()
                .center()
        });
        visual.simulate_mouse_down(close, gpui_kit::MouseButton::Left, Default::default());
        visual.simulate_mouse_up(close, gpui_kit::MouseButton::Left, Default::default());
        visual.run_until_parked();
        visual.update(|window, cx| {
            let owner = app.read(cx);
            assert!(owner.document_comparison.is_none());
            assert!(
                owner.editor.read(cx).focus_handle(cx).is_focused(window),
                "closing comparison must restore the current document's keyboard focus"
            );
            assert_eq!(foreign_marks.get_ranges(cx), vec![0..3]);
        });
        run_consumer(
            visual,
            &app,
            &mut manager,
            "history-preview",
            "compare-history",
        );
        // Refresh replaces the readonly text and Base maps existing owner ranges with that change.
        // Subsequent comparison disposal must retain those mapped ranges rather than old offsets.
        let refreshed_marks = visual.update(|_, cx| foreign_marks.get_ranges(cx));
        assert!(!refreshed_marks.is_empty());
        // A user edit on the right invalidates the comparison rather than leaving stale colors.
        let click = right.origin + gpui_kit::point(gpui_kit::px(20.), gpui_kit::px(60.));
        visual.simulate_mouse_down(click, gpui_kit::MouseButton::Left, Default::default());
        visual.simulate_mouse_up(click, gpui_kit::MouseButton::Left, Default::default());
        visual.simulate_keystrokes("end");
        visual.simulate_input("!");
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            visual.debug_bounds("document-diff-left").is_none(),
            "changed text must dismiss stale presentation"
        );
        visual.update(|_, cx| assert_eq!(foreign_marks.get_ranges(cx), refreshed_marks));
        run_consumer(
            visual,
            &app,
            &mut manager,
            "history-preview",
            "refresh-history",
        );
        visual.update(|_, cx| {
            let refreshed = app.read(cx).plugin_document_info(1, cx).unwrap();
            assert_eq!(refreshed.document.id, initial.document.id);
            assert!(refreshed.document.revision > initial.document.revision);
        });
        run_consumer(
            visual,
            &app,
            &mut manager,
            "generated-preview",
            "preview-generated",
        );
        let generated = visual.update(|_, cx| {
            let owner = app.read(cx);
            assert_eq!(owner.tabs.len(), 3);
            let proposal = owner.tabs[2]
                .text
                .as_ref()
                .unwrap()
                .editor
                .read(cx)
                .value()
                .to_string();
            assert!(
                proposal.contains("CURRENT 中😀") && proposal.contains("!"),
                "proposal must use the unsaved revision"
            );
            owner.plugin_document_info(2, cx).unwrap()
        });
        // A candidate that cannot instantiate must not revoke the previous provider or its view.
        let mut files = package.files.clone();
        let mut manifest: Json = serde_json::from_slice(&files["manifest.json"]).unwrap();
        manifest["version"] = json!("0.1.2");
        files.insert(
            "manifest.json".into(),
            serde_json::to_vec(&manifest).unwrap(),
        );
        files.insert(
            "generated-preview.wasm".into(),
            vec![0, 97, 115, 109, 13, 0, 1, 0],
        );
        let failed = Package::from_files(files).unwrap();
        assert!(
            manager
                .install(&failed, failed.manifest.permissions.clone())
                .is_err()
        );
        assert!(matches!(
            invoke_for(
                visual,
                &app,
                &mut manager,
                "generated-preview",
                json!({"kind":"read_document", "document":generated.document})
            )
            .unwrap(),
            api::EditorValue::DocumentSnapshot(_)
        ));
        let api::EditorValue::Documents { documents, .. } =
            invoke(visual, &app, &mut manager, json!({"kind":"list_documents"}))
        else {
            panic!("documents expected")
        };
        assert_eq!(
            documents.len(),
            2,
            "another guest's virtual tab must not leak through enumeration"
        );
        assert_eq!(
            invoke_result(
                visual,
                &app,
                &mut manager,
                json!({"kind":"read_document", "document":generated.document})
            )
            .unwrap_err()
            .code,
            api::ErrorCode::PermissionDenied
        );
        assert!(visual.debug_bounds("document-diff-left").is_some());
        // Explicit release revokes authority and removes its pane/tab on the production frame path.
        manager.invoke_command("generated-preview", "probe", json!({"method":"close_resource", "handle":match generated.resource { api::ResourceIdentity::Virtual { handle } => handle, _ => unreachable!() }})).unwrap();
        visual.update(|_, cx| app.update(cx, |_, cx| cx.notify()));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("document-diff-left").is_none());
        visual.update(|_, cx| assert_eq!(app.read(cx).tabs.len(), 2));
        deliver_native_events(visual, &app, &mut manager);
        run_consumer(
            visual,
            &app,
            &mut manager,
            "generated-preview",
            "preview-generated",
        );
        visual.update(|_, cx| {
            let reopened = app.read(cx).plugin_document_info(2, cx).unwrap();
            assert_ne!(
                reopened.document.id, generated.document.id,
                "release and reopen creates a new native session"
            );
        });
        manager.disable("generated-preview").unwrap();
        manager.disable("history-preview").unwrap();
        visual.update(|_, cx| app.update(cx, |_, cx| cx.notify()));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        visual.update(|_, cx| assert_eq!(app.read(cx).tabs.len(), 1));
        assert!(visual.debug_bounds("document-diff-left").is_none());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "original on disk");
    });
}
