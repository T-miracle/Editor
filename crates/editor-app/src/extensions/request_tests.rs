//! Drive real component editor requests against native documents and dock surfaces.
use super::*;
use gpui_kit::{TestAppContext, gpui};
use protocol::api::{self, EditorOperation as Op, EditorValue, RequestUpdate};

/// Read selection and save the exact requested revision through the real GPUI document path.
#[gpui::test]
#[ignore = "build with scripts/build-capability-example.ps1 first"]
fn typed_editor_requests_read_selection_and_save_without_switching_documents(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sample.txt");
    std::fs::write(&path, "before").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let package = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
    )
    .unwrap();
    let mut manager = plugin_runtime::Manager::open(
        directory.path().join("runtime"),
        protocol::Environment {
            workspace: workspace.root().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let initial = path.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, Some(initial), window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    cx.simulate_resize(size(px(1200.), px(800.)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        app.read(cx).extensions.clone().update(cx, |owner, cx| {
            let mut state = owner.worker.state.lock().unwrap();
            state.entries = manager.published_entries();
            for (id, instance) in &manager.live {
                for (panel, scene) in &instance.scenes {
                    state.scenes.insert(format!("{id}/{panel}"), scene.clone());
                }
            }
            drop(state);
            owner.poll(cx);
        });
        app.update(cx, |app, cx| app.sync_plugin_panels(window, cx));
        app.read(cx).editor.clone().update(cx, |editor, cx| {
            editor.set_value("after edit", window, cx);
            editor.set_selected_range(0..5, cx);
        });
    });
    cx.run_until_parked();
    let request = |manager: &mut plugin_runtime::Manager, operation| {
        manager
            .invoke_command(
                &package.manifest.id,
                "scope-probe",
                serde_json::to_value(api::Operation::Editor {
                    operation,
                    timeout_ms: 30000,
                })
                .unwrap(),
            )
            .unwrap();
        manager
            .live
            .get_mut(&package.manifest.id)
            .unwrap()
            .take_editor_requests()
            .pop()
            .unwrap()
    };
    let read = request(&mut manager, Op::ReadSelection);
    // Traverse the worker publication and deferred UI queue, not just the editor implementation.
    cx.update(|_, cx| {
        app.read(cx).extensions.clone().update(cx, |owner, cx| {
            owner
                .worker
                .state
                .lock()
                .unwrap()
                .editor_requests
                .push((package.manifest.id.clone(), read.clone()));
            owner.poll(cx);
        })
    });
    cx.run_until_parked();
    let RequestUpdate::Completed {
        result: Ok(EditorValue::Selection { document, text }),
    } = read.status()
    else {
        panic!("selection expected: {:?}", read.status())
    };
    assert_eq!(text, "after");
    let save = request(
        &mut manager,
        Op::SaveDocument {
            document: document.clone(),
        },
    );
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.perform_editor_request(&package.manifest.id, save.clone(), window, cx)
        })
    });
    cx.run_until_parked();
    assert!(
        matches!(
            save.status(),
            RequestUpdate::Completed {
                result: Ok(EditorValue::Saved { .. })
            }
        ),
        "{:?}",
        save.status()
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "after edit");
    // Typed visibility reaches the actual dock, and hiding must return occupied space to the editor.
    let mut shown_width = None;
    for visible in [true, false, true] {
        let panel = request(
            &mut manager,
            Op::SetPanelVisibility {
                panel: "welcome".into(),
                visible,
            },
        );
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.perform_editor_request(&package.manifest.id, panel.clone(), window, cx)
            })
        });
        cx.run_until_parked();
        for _ in 0..2 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }
        assert_eq!(cx.debug_bounds("plugin-ui-welcome-text").is_some(), visible);
        let width = cx.debug_bounds("editor-panel-content").unwrap().size.width;
        if visible {
            shown_width = Some(width);
        } else {
            assert!(
                width > shown_width.unwrap(),
                "hidden panel must reclaim dock width"
            );
        }
        assert!(matches!(
            panel.status(),
            RequestUpdate::Completed {
                result: Ok(EditorValue::PanelVisibility { .. })
            }
        ));
    }
    // External reloads are silent to the editor's input listener, but must invalidate API versions.
    std::fs::write(&path, "external revision").unwrap();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.apply_reconciliation(
                Reconciliation {
                    snapshot: Some(app.workspace.snapshot()),
                    documents: vec![(
                        path.canonicalize().unwrap(),
                        Ok("external revision".into()),
                        Instant::now(),
                    )],
                    renames: Vec::new(),
                    native: true,
                },
                window,
                cx,
            );
            assert!(app.plugin_document_version(0).unwrap().revision > document.revision);
        })
    });
    let stale = request(
        &mut manager,
        Op::SaveDocument {
            document: document.clone(),
        },
    );
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.perform_editor_request(&package.manifest.id, stale.clone(), window, cx)
        })
    });
    assert!(matches!(
        stale.status(),
        RequestUpdate::Completed {
            result: Err(api::Failure {
                code: api::ErrorCode::StaleRevision,
                ..
            })
        }
    ));
    // A save paused in background preparation must not overwrite an external change at UI commit.
    let current = cx.update(|_, cx| app.read(cx).plugin_document_version(0).unwrap());
    let conflict = request(
        &mut manager,
        Op::SaveDocument {
            document: current.clone(),
        },
    );
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.perform_editor_request(&package.manifest.id, conflict.clone(), window, cx)
        });
        std::fs::write(&path, "newer external content").unwrap();
    });
    cx.run_until_parked();
    assert!(matches!(
        conflict.status(),
        RequestUpdate::Completed {
            result: Err(api::Failure {
                code: api::ErrorCode::Conflict,
                ..
            })
        }
    ));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "newer external content"
    );
    // A renamed document never leaves a recreated file at the stale path after preparation finishes.
    std::fs::write(&path, "external revision").unwrap();
    let renamed = directory.path().join("renamed.txt");
    let saving = request(&mut manager, Op::SaveDocument { document: current });
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.perform_editor_request(&package.manifest.id, saving.clone(), window, cx);
            let old = path.canonicalize().unwrap();
            std::fs::rename(&path, &renamed).unwrap();
            app.apply_reconciliation(
                Reconciliation {
                    snapshot: Some(app.workspace.snapshot()),
                    documents: Vec::new(),
                    renames: vec![(old, renamed.canonicalize().unwrap())],
                    native: true,
                },
                window,
                cx,
            );
        })
    });
    cx.run_until_parked();
    assert!(matches!(
        saving.status(),
        RequestUpdate::Completed { result: Err(_) }
    ));
    assert!(!path.exists());
    assert_eq!(
        std::fs::read_to_string(&renamed).unwrap(),
        "external revision"
    );
    // Deleting a file on disk does not close its still-open editor entity or invalidate its identity.
    cx.update(|_, cx| app.update(cx, |app, cx| app.sync_plugin_documents(cx)));
    let open = cx.update(|_, cx| app.read(cx).plugin_document_version(0).unwrap());
    std::fs::remove_file(&renamed).unwrap();
    cx.update(|_, cx| {
        app.update(cx, |app, cx| {
            assert_eq!(app.plugin_document_version(0).unwrap(), open);
            app.sync_plugin_documents(cx);
            assert!(app.plugin_documents.contains_key(&open.id));
        })
    });
    std::fs::write(&renamed, "external revision").unwrap();
    // The real UI source carries monotonically changing document versions to the worker mailbox.
    manager
        .invoke_command(
            &package.manifest.id,
            "scope-probe",
            serde_json::json!({"method":"subscribe_documents"}),
        )
        .unwrap();
    cx.update(|_, cx| app.update(cx, |app, cx| app.sync_plugin_documents(cx)));
    let changes = cx.update(|_, cx| {
        app.read(cx)
            .extensions
            .read(cx)
            .worker
            .state
            .lock()
            .unwrap()
            .document_events
            .take_batch(64)
            .unwrap()
    });
    assert!(!changes.is_empty());
    for change in changes {
        manager.document_changed(change);
    }
    manager.poll();
    let scene = manager.live[&package.manifest.id].scene.as_ref().unwrap();
    let protocol::ui::Kind::Text { text } = &scene.ui.as_ref().unwrap().root.kind else {
        panic!("text expected")
    };
    assert!(text.contains("Document"));
    // Desktop capabilities use the same admission/completion gate and never expose raw host commands.
    for operation in [
        Op::WriteClipboard {
            text: "SDK 中文 paste".into(),
        },
        Op::ReadClipboard,
    ] {
        let call = request(&mut manager, operation);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.perform_editor_request(&package.manifest.id, call.clone(), window, cx)
            })
        });
        cx.run_until_parked();
        match call.status() {
            RequestUpdate::Completed {
                result: Ok(EditorValue::Clipboard { text }),
            } => assert_eq!(text, "SDK 中文 paste"),
            RequestUpdate::Completed {
                result: Ok(EditorValue::Unit),
            } => {}
            status => panic!("clipboard completion expected: {status:?}"),
        }
    }
    let private = manager
        .data_directory(&package.manifest.id)
        .join("settings.json");
    std::fs::write(&private, "{\"value\":1}").unwrap();
    let call = request(
        &mut manager,
        Op::OpenDataFile {
            path: "settings.json".into(),
        },
    );
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.perform_editor_request(&package.manifest.id, call.clone(), window, cx)
        })
    });
    cx.run_until_parked();
    assert!(matches!(
        call.status(),
        RequestUpdate::Completed {
            result: Ok(EditorValue::Unit)
        }
    ));
    let expected = private.canonicalize().unwrap();
    assert!(cx.update(|_, cx| {
        app.read(cx)
            .tabs
            .iter()
            .any(|tab| tab.session.path() == expected)
    }));
}
