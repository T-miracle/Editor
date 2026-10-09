//! Exact native snapshots, coordinates and ordered source transitions through the actual SDK.
use super::*;

/// Unsaved text and metadata are observed at one exact native revision, never reread from disk.
#[gpui::test]
#[ignore = "package plugins/history-preview with the current host --plugin-package first"]
fn community_document_snapshot_reads_unsaved_native_text(cx: &mut TestAppContext) {
    crate::tests::with_shortcut_editor(cx, false, Vec::new(), |visual, app, path| {
        let (_runtime, mut manager) = manager(path.parent().unwrap());
        visual.simulate_keystrokes("ctrl-a");
        visual.simulate_input("中😀\r\nsecond");
        visual.run_until_parked();
        let api::EditorValue::Documents { documents, active } =
            invoke(visual, &app, &mut manager, json!({"kind":"list_documents"}))
        else {
            panic!("documents expected")
        };
        let document = &documents[0];
        assert!(document.dirty);
        assert_eq!(document.byte_len, 15);
        assert_eq!(document.eol, api::DocumentEol::CrLf);
        assert_eq!(active.as_ref(), Some(&document.document));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "original on disk");
        let value = invoke(
            visual,
            &app,
            &mut manager,
            json!({"kind":"read_document", "document":document.document}),
        );
        let json = serde_json::to_value(value).unwrap();
        assert_eq!(json["DocumentSnapshot"]["text"], "中😀\r\nsecond");
        let api::EditorValue::DocumentSnapshot(snapshot) = invoke(
            visual,
            &app,
            &mut manager,
            json!({"kind":"read_document", "document":document.document,
            "range":{"unit":"utf16", "start":{"line":0,"character":1}, "end":{"line":0,"character":3}}}),
        ) else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.text, "😀");
        assert_eq!(snapshot.range, api::TextRange { start: 3, end: 7 });
        let api::EditorValue::DocumentSnapshot(snapshot) = invoke(
            visual,
            &app,
            &mut manager,
            json!({"kind":"read_document", "document":document.document,
            "range":{"unit":"utf16", "start":{"line":1,"character":0}, "end":{"line":1,"character":6}}}),
        ) else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.text, "second");
        assert_eq!(snapshot.range, api::TextRange { start: 9, end: 15 });
        for range in [
            json!({"unit":"bytes", "start":1, "end":2}),
            json!({"unit":"bytes", "start":0, "end":16}),
            json!({"unit":"utf16", "start":{"line":0,"character":2}, "end":{"line":0,"character":3}}),
        ] {
            assert_eq!(
                invoke_result(
                    visual,
                    &app,
                    &mut manager,
                    json!({"kind":"read_document", "document":document.document, "range":range})
                )
                .unwrap_err()
                .code,
                api::ErrorCode::InvalidRequest
            );
        }
        visual.simulate_keystrokes("end");
        visual.simulate_input("!");
        visual.run_until_parked();
        assert_eq!(
            invoke_result(
                visual,
                &app,
                &mut manager,
                json!({"kind":"read_document", "document":document.document})
            )
            .unwrap_err()
            .code,
            api::ErrorCode::StaleRevision
        );
        // Native input, viewport navigation, saving and close/reopen publish real source transitions.
        deliver_native_events(visual, &app, &mut manager);
        visual.simulate_keystrokes("ctrl-a");
        for _ in 0..50 {
            visual.simulate_input("line 中😀\r\nline two\r\n");
            visual.run_until_parked();
            // The native fixture records worker messages instead of running its actor thread.
            deliver_native_events(visual, &app, &mut manager);
        }
        visual.simulate_keystrokes("ctrl-home ctrl-end ctrl-s");
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let closed =
            visual.update(|_, cx| app.read(cx).plugin_document_info(0, cx).unwrap().document);
        let closed_path = visual.update(|_, cx| {
            let owner = app.read(cx);
            assert!(
                !owner.tabs[0].is_dirty(),
                "native save must complete before close: {}",
                owner.status
            );
            owner.tabs[0].path().to_path_buf()
        });
        visual.update(|window, cx| {
            app.update(cx, |owner, cx| owner.close_tab(closed_path, window, cx))
        });
        visual.run_until_parked();
        visual.update(|window, cx| {
            app.update(cx, |owner, cx| {
                owner.open_file(path.to_path_buf(), window, cx)
            })
        });
        visual.run_until_parked();
        let reopened =
            visual.update(|_, cx| app.read(cx).plugin_document_info(0, cx).unwrap().document);
        assert_ne!(closed.id, reopened.id);
        assert_eq!(
            invoke_result(
                visual,
                &app,
                &mut manager,
                json!({"kind":"read_document", "document":closed})
            )
            .unwrap_err()
            .code,
            api::ErrorCode::StaleRevision
        );
        deliver_native_events(visual, &app, &mut manager);
        // Each poll delivers a bounded batch, preserving the whole published native sequence.
        for _ in 0..8 {
            manager.poll();
        }
        manager
            .invoke_command("history-preview", "events", json!(null))
            .unwrap();
        let ui::Kind::Text { text } = &manager.live["history-preview"].views["history"].root.kind
        else {
            panic!("event log expected")
        };
        let log: Vec<api::Notification> = serde_json::from_str(text).unwrap();
        let events = log
            .into_iter()
            .filter_map(|notification| match notification {
                api::Notification::DocumentEvent { event, .. } => Some(event),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(
            events
                .windows(2)
                .all(|pair| pair[0].sequence < pair[1].sequence)
        );
        for predicate in [
            |kind: &api::DocumentEventKind| matches!(kind, api::DocumentEventKind::Opened(_)),
            |kind: &api::DocumentEventKind| {
                matches!(kind, api::DocumentEventKind::ContentChanged(_))
            },
            |kind: &api::DocumentEventKind| matches!(kind, api::DocumentEventKind::Closed(_)),
            |kind: &api::DocumentEventKind| {
                matches!(kind, api::DocumentEventKind::ActiveChanged(_))
            },
            |kind: &api::DocumentEventKind| {
                matches!(kind, api::DocumentEventKind::SelectionChanged { .. })
            },
            |kind: &api::DocumentEventKind| {
                matches!(kind, api::DocumentEventKind::ViewportChanged { .. })
            },
        ] {
            assert!(
                events.iter().any(|event| predicate(&event.kind)),
                "native event missing from {events:?}"
            );
        }
        let before = events
            .iter()
            .position(|event| matches!(event.kind, api::DocumentEventKind::WillSave(_)))
            .unwrap();
        let after = events
            .iter()
            .position(|event| {
                matches!(
                    event.kind,
                    api::DocumentEventKind::DidSave { result: Ok(()), .. }
                )
            })
            .unwrap();
        assert!(before < after, "save observations preserve source order");
    });
}
