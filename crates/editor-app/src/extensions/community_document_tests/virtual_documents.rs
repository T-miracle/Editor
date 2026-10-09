//! Readonly virtual input, replacement and native/runtime lifecycle acceptance.
use super::*;

/// Virtual content is opened through the real guest and remains readonly under native keyboard input.
#[gpui::test]
#[ignore = "package plugins/history-preview with the current host --plugin-package first"]
fn community_virtual_document_rejects_native_input(cx: &mut TestAppContext) {
    crate::tests::with_shortcut_editor(cx, false, Vec::new(), |visual, app, path| {
        let (_runtime, mut manager) = manager(path.parent().unwrap());
        let providers = visual.update(|_, cx| {
            serde_json::to_value(&app.read(cx).session_state.file_view_providers).unwrap()
        });
        let value = invoke(
            visual,
            &app,
            &mut manager,
            json!({"kind":"open_virtual_document", "title":"History.txt", "text":"历史😀\r\nreadonly"}),
        );
        let value = serde_json::to_value(value).unwrap();
        assert_eq!(value["DocumentOpened"]["resource"]["kind"], "virtual");
        assert_eq!(value["DocumentOpened"]["access"]["edit"], false);
        assert_eq!(value["DocumentOpened"]["access"]["save"], false);
        visual.update(|window, cx| {
            app.update(cx, |owner, cx| {
                owner.extensions.update(cx, |panel, _| {
                    panel.bundled.ready = true;
                    // The isolated fixture records actual actor messages; clear its initial local discovery.
                    let _ = panel
                        .worker
                        .recorded
                        .lock()
                        .unwrap()
                        .try_iter()
                        .collect::<Vec<_>>();
                });
                owner.sync_bundled_first_use(window, cx);
                assert_eq!(
                    serde_json::to_value(&owner.session_state.file_view_providers).unwrap(),
                    providers,
                    "virtual activation cannot replace remembered local providers"
                );
                assert_eq!(
                    owner.plugin_file_context(1).unwrap_err().code,
                    api::ErrorCode::UnsupportedOperation
                );
                let panel = owner.extensions.read(cx);
                assert!(
                    !panel
                        .worker
                        .recorded
                        .lock()
                        .unwrap()
                        .try_iter()
                        .any(|work| matches!(work, Work::InspectBundle(_))),
                    "virtual identities must not enter local first-use filesystem discovery"
                );
            })
        });
        visual.simulate_keystrokes("ctrl-a");
        visual.simulate_input("forbidden");
        visual.simulate_keystrokes("ctrl-s");
        visual.run_until_parked();
        visual.update(|_, cx| {
            let owner = app.read(cx);
            assert_eq!(
                owner.editor.read(cx).value().to_string(),
                "历史😀\r\nreadonly"
            );
            assert_eq!(owner.tabs.len(), 2);
            assert_eq!(
                owner.session_state.open_tabs.len(),
                1,
                "virtual tabs never enter persisted filesystem restoration"
            );
            assert!(!owner.tabs[1].is_dirty());
        });
        assert_eq!(std::fs::read_to_string(path).unwrap(), "original on disk");
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1,
            "no temporary backing document"
        );
        let initial: api::DocumentInfo =
            serde_json::from_value(value["DocumentOpened"].clone()).unwrap();
        let api::EditorValue::DocumentOpened(refreshed) = invoke(
            visual,
            &app,
            &mut manager,
            json!({"kind":"refresh_virtual_document", "document":initial.document, "text":"新内容\r\nline two😀"}),
        ) else {
            panic!("refresh expected")
        };
        assert_eq!(refreshed.document.id, initial.document.id);
        assert!(refreshed.document.revision > initial.document.revision);
        assert!(!refreshed.dirty);
        assert_eq!(
            invoke_result(
                visual,
                &app,
                &mut manager,
                json!({"kind":"read_document", "document":initial.document})
            )
            .unwrap_err()
            .code,
            api::ErrorCode::StaleRevision
        );
        invoke(
            visual,
            &app,
            &mut manager,
            json!({"kind":"locate_document", "document":refreshed.document, "position":{"line":1, "character":8}}),
        );
        visual.update(|_, cx| assert_eq!(app.read(cx).editor.read(cx).selected_range(), 19..19));
        assert_eq!(invoke_result(visual, &app, &mut manager, json!({"kind":"locate_document", "document":refreshed.document, "position":{"line":1, "character":9}})).unwrap_err().code, api::ErrorCode::InvalidRequest);
        assert_eq!(
            invoke_result(visual, &app, &mut manager, json!({"kind":"read_selection"}))
                .unwrap_err()
                .code,
            api::ErrorCode::UnsupportedOperation
        );
        invoke(
            visual,
            &app,
            &mut manager,
            json!({"kind":"open_document", "resource":refreshed.resource}),
        );
        let virtual_path = visual.update(|_, cx| app.read(cx).tabs[1].path().to_path_buf());
        visual
            .update(|window, cx| app.update(cx, |app, cx| app.close_tab(virtual_path, window, cx)));
        visual.run_until_parked();
        assert_eq!(
            invoke_result(
                visual,
                &app,
                &mut manager,
                json!({"kind":"open_document", "resource":refreshed.resource})
            )
            .unwrap_err()
            .code,
            api::ErrorCode::InvalidHandle
        );
        invoke(
            visual,
            &app,
            &mut manager,
            json!({"kind":"open_virtual_document", "title":"Second history", "text":"next"}),
        );
        manager.disable("history-preview").unwrap();
        visual.update(|_, cx| {
            app.update(cx, |app, cx| {
                cx.notify();
                app.editor_panel.update(cx, |_, cx| cx.notify());
            })
        });
        visual.run_until_parked();
        visual.update(|_, cx| {
            assert_eq!(
                app.read(cx).tabs.len(),
                1,
                "retirement removes native tabs along with authority"
            )
        });
    });
}
