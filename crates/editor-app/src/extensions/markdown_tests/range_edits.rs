//! An independent SDK guest probes document editing authority through the production request queue.
use super::*;
use gpui_kit::EntityInputHandler as _;
use harness::NativeMarkdown;
use protocol::api::{EditorOperation as Op, EditorValue as Value, RequestUpdate, TextRange};
use std::io::{Cursor, Write};

/// Only the declared capabilities change; WASM dispatch remains the independently built public example.
fn edit_peer() -> Package {
    let mut files = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
    )
    .unwrap()
    .files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["api"]["required"]["editor.edit"] = serde_json::json!("^1");
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

/// Request handles originate in real WASM, retaining its grant, workspace, deadline and retirement gates.
fn request(fixture: &mut NativeMarkdown, operation: Op) -> plugin_runtime::EditorRequest {
    fixture
        .manager
        .invoke_command(
            "capability-example",
            "scope-probe",
            serde_json::to_value(protocol::api::Operation::Editor {
                operation,
                timeout_ms: 30000,
            })
            .unwrap(),
        )
        .unwrap();
    fixture
        .manager
        .live
        .get_mut("capability-example")
        .unwrap()
        .take_editor_requests()
        .pop()
        .unwrap()
}

/// Execute via the public worker publication seam, including deferred document change notifications.
fn execute(
    fixture: &mut NativeMarkdown,
    request: &plugin_runtime::EditorRequest,
    ui: &mut gpui_kit::VisualTestContext,
) {
    ui.update(|_, cx| {
        fixture
            .app
            .read(cx)
            .extensions
            .read(cx)
            .worker
            .state
            .lock()
            .unwrap()
            .editor_requests
            .push(("capability-example".into(), request.clone()));
    });
    fixture.settle(ui);
}

/// Malformed or obsolete offsets must produce typed failures without editing or consuming undo history.
#[gpui::test]
#[ignore = "build markdown and current capability-example through the public SDK first"]
fn delivered_range_edits_guard_utf8_selection_revision_and_cancellation(cx: &mut TestAppContext) {
    let original = "甲😀乙\n第二行";
    let (mut fixture, ui) = NativeMarkdown::mount(cx, &[("notes.md", original)]);
    let peer = edit_peer();
    fixture
        .manager
        .install(&peer, peer.manifest.permissions.clone())
        .unwrap();
    fixture.open("notes.md", ui);
    let document = ui.update(|_, cx| {
        let app = fixture.app.read(cx);
        app.plugin_document_version(app.active_tab_index().unwrap())
            .unwrap()
    });
    assert_eq!(document.path, "notes.md");
    let read = request(
        &mut fixture,
        Op::ReadDocumentSelection {
            document: document.clone(),
        },
    );
    execute(&mut fixture, &read, ui);
    assert!(
        matches!(read.status(), RequestUpdate::Completed {
        result: Ok(Value::DocumentSelection { document: ref observed, range: TextRange { start: 0, end: 0 }, .. })
    } if observed == &document),
        "{:?}; current {:?}",
        read.status(),
        ui.update(|_, cx| {
            let app = fixture.app.read(cx);
            app.active_tab_index()
                .map(|index| app.plugin_document_version(index))
        })
    );

    for (range, selection, expected_selection, revision, expected_code) in [
        (
            TextRange { start: 1, end: 2 },
            TextRange { start: 0, end: 0 },
            None,
            document.revision,
            protocol::api::ErrorCode::InvalidRequest,
        ),
        (
            TextRange { start: 0, end: 3 },
            TextRange { start: 2, end: 2 },
            None,
            document.revision,
            protocol::api::ErrorCode::InvalidRequest,
        ),
        (
            TextRange { start: 0, end: 3 },
            TextRange { start: 0, end: 3 },
            Some(TextRange { start: 0, end: 3 }),
            document.revision,
            protocol::api::ErrorCode::StaleRevision,
        ),
        (
            TextRange { start: 0, end: 3 },
            TextRange { start: 0, end: 3 },
            None,
            document.revision + 1,
            protocol::api::ErrorCode::StaleRevision,
        ),
    ] {
        let mut version = document.clone();
        version.revision = revision;
        let call = request(
            &mut fixture,
            Op::ReplaceDocumentRange {
                document: version,
                range,
                text: "中".into(),
                selection,
                expected_selection,
            },
        );
        execute(&mut fixture, &call, ui);
        assert!(
            matches!(call.status(), RequestUpdate::Completed { result: Err(ref failure) } if failure.code == expected_code)
        );
        assert_eq!(
            ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
            original
        );
    }
    let operation = Op::ReplaceDocumentRange {
        document: document.clone(),
        range: TextRange { start: 0, end: 3 },
        text: "中文".into(),
        selection: TextRange { start: 0, end: 6 },
        expected_selection: Some(TextRange { start: 0, end: 0 }),
    };
    let cancelled = request(&mut fixture, operation.clone());
    fixture
        .manager
        .invoke_command(
            "capability-example",
            "scope-probe",
            serde_json::json!({
                "method": "cancel_request", "handle": cancelled.handle(), "mode": "try_terminate"
            }),
        )
        .unwrap();
    execute(&mut fixture, &cancelled, ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        original
    );
    let edit = request(&mut fixture, operation);
    execute(&mut fixture, &edit, ui);
    let RequestUpdate::Completed {
        result: Ok(Value::Edited {
            document: next,
            selection,
        }),
    } = edit.status()
    else {
        panic!("Expected the actual edited version: {:?}", edit.status());
    };
    assert_eq!(next.revision, document.revision + 1);
    assert_eq!(
        next,
        ui.update(|_, cx| {
            let app = fixture.app.read(cx);
            app.plugin_document_version(app.active_tab_index().unwrap())
                .unwrap()
        })
    );
    assert_eq!(selection, TextRange { start: 0, end: 6 });
    assert!(ui.update(|_, cx| {
        let app = fixture.app.read(cx);
        app.tabs[app.active_tab_index().unwrap()].session.is_dirty()
    }));
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        "中文😀乙\n第二行"
    );
    ui.simulate_keystrokes("ctrl-z");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        original
    );
    assert_eq!(
        std::fs::read_to_string(fixture.directory.path().join("notes.md")).unwrap(),
        original
    );

    // A real worker batch must observe the first replacement's committed revision before the next call.
    let batch_version = ui.update(|_, cx| {
        let app = fixture.app.read(cx);
        app.plugin_document_version(app.active_tab_index().unwrap())
            .unwrap()
    });
    let first = request(
        &mut fixture,
        Op::ReplaceDocumentRange {
            document: batch_version.clone(),
            range: TextRange { start: 0, end: 3 },
            text: "中".into(),
            selection: TextRange { start: 0, end: 3 },
            expected_selection: None,
        },
    );
    let second = request(
        &mut fixture,
        Op::ReplaceDocumentRange {
            document: batch_version.clone(),
            range: TextRange { start: 0, end: 3 },
            text: "文".into(),
            selection: TextRange { start: 0, end: 3 },
            expected_selection: None,
        },
    );
    let old_read = request(
        &mut fixture,
        Op::ReadDocumentSelection {
            document: batch_version,
        },
    );
    ui.update(|_, cx| {
        fixture
            .app
            .read(cx)
            .extensions
            .read(cx)
            .worker
            .state
            .lock()
            .unwrap()
            .editor_requests
            .extend(
                [first.clone(), second.clone(), old_read.clone()]
                    .map(|call| ("capability-example".into(), call)),
            );
    });
    fixture.settle(ui);
    assert!(matches!(
        first.status(),
        RequestUpdate::Completed {
            result: Ok(Value::Edited { .. })
        }
    ));
    for rejected in [&second, &old_read] {
        assert!(
            matches!(rejected.status(), RequestUpdate::Completed { result: Err(ref failure) }
            if failure.code == protocol::api::ErrorCode::StaleRevision),
            "{:?}",
            rejected.status()
        );
    }
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        "中😀乙\n第二行"
    );
    ui.simulate_keystrokes("ctrl-z");
    ui.run_until_parked();
    fixture.settle(ui);
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        original
    );

    // Native preedit is an open IME transaction. Formatting must not split or commit it implicitly.
    ui.update(|window, cx| {
        fixture
            .app
            .read(cx)
            .editor
            .clone()
            .update(cx, |editor, cx| {
                editor.set_selected_range(0..0, cx);
                editor.replace_and_mark_text_in_range(None, "拼", Some(0..1), window, cx);
            })
    });
    ui.run_until_parked();
    fixture.settle(ui);
    let composing = ui.update(|_, cx| {
        let app = fixture.app.read(cx);
        app.plugin_document_version(app.active_tab_index().unwrap())
            .unwrap()
    });
    let composing_text = ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string());
    for operation in [
        Op::ReadDocumentSelection {
            document: composing.clone(),
        },
        Op::ReplaceDocumentRange {
            document: composing.clone(),
            range: TextRange { start: 0, end: 0 },
            text: "# ".into(),
            selection: TextRange { start: 2, end: 2 },
            expected_selection: None,
        },
    ] {
        let call = request(&mut fixture, operation);
        execute(&mut fixture, &call, ui);
        assert!(
            matches!(call.status(), RequestUpdate::Completed { result: Err(ref failure) }
            if failure.code == protocol::api::ErrorCode::InvalidState)
        );
        assert_eq!(
            ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
            composing_text
        );
    }
    ui.update(|window, cx| {
        fixture
            .app
            .read(cx)
            .editor
            .clone()
            .update(cx, |editor, cx| editor.unmark_text(window, cx))
    });
    let retired = request(
        &mut fixture,
        Op::ReplaceDocumentRange {
            document: composing,
            range: TextRange { start: 0, end: 0 },
            text: "# ".into(),
            selection: TextRange { start: 2, end: 2 },
            expected_selection: None,
        },
    );
    fixture.manager.disable("capability-example").unwrap();
    execute(&mut fixture, &retired, ui);
    assert!(matches!(retired.status(), RequestUpdate::Cancelled { .. }));
    assert_eq!(
        ui.update(|_, cx| fixture.app.read(cx).editor.read(cx).text().to_string()),
        composing_text
    );
}
