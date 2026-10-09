//! Native user input completes real SDK requests through the same worker publication as production.
use super::*;
use gpui_kit::{TestAppContext, gpui};
use protocol::api::{self, EditorValue, RequestUpdate};
use std::io::{Cursor, Write};

/// This independent package retains the real component and passes ordinary package admission.
fn interaction_package(id: &str) -> Package {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/plugin-api-test");
    let file = folder.join("capability-example-0.18.0.zip");
    let mut files = Package::read(&file).unwrap().files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["id"] = serde_json::json!(id);
    manifest["api"]["required"]["ui.interaction"] = serde_json::json!("^1");
    manifest["permissions"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!("ui.interaction"));
    // Native menus use independent instance identities and ordinary one-way guest commands.
    manifest["commands"].as_array_mut().unwrap().push(serde_json::json!({"id":"menu-context", "title":"Menu target", "menus":[
        {"location":"editor","group":"a","order":1}, {"location":"selection","group":"a","order":1},
        {"location":"explorer","group":"a","order":1}, {"location":"tab","group":"a","order":1}
    ]}));
    manifest["commands"].as_array_mut().unwrap().push(
        serde_json::json!({"id":"menu-disabled", "title":"Directory only", "menus":[
            {"location":"explorer","group":"a","order":0,"enabled_when":{"directory":true}}
        ]}),
    );
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

/// Real explorer activation keeps its clicked target, conditions, ordering and incarnation gates.
#[gpui::test]
#[ignore = "build capability-example with current --plugin-package before running"]
fn native_plugin_menus_revalidate_context_and_remove_retired_contributions(
    cx: &mut TestAppContext,
) {
    use protocol::commands::Location;
    crate::tests::with_shortcut_editor(cx, false, vec![], |visual, app, active| {
        let root = active.parent().unwrap();
        let clicked = root.join("clicked.txt");
        std::fs::write(&clicked, "clicked").unwrap();
        let runtime = tempfile::tempdir().unwrap();
        let mut manager = plugin_runtime::Manager::open(
            runtime.path().into(),
            protocol::Environment {
                workspace: root.display().to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        let package = interaction_package("native-menu-consumer");
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
        let mut renderer = images::VectorRenderer::default();
        composable_tests::publish(&mut manager, &mut renderer, &app, visual);
        // The clicked path is an open, nonactive session. Its menu must retain that identity.
        visual.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_file(clicked.clone(), window, cx);
                let index = app
                    .tabs
                    .iter()
                    .position(|tab| tab.path() == active.canonicalize().unwrap())
                    .unwrap();
                app.activate_tab(index, window, cx);
            })
        });
        visual.run_until_parked();
        let (target, captured) = visual.update(|_, cx| {
            let app = app.read(cx);
            let target = app.plugin_menu_target(&clicked);
            let rows = app.plugin_menu_entries(&target, &[Location::Explorer], cx);
            let ours = rows
                .iter()
                .filter(|row| row.command.starts_with("menu-"))
                .collect::<Vec<_>>();
            assert_eq!(
                ours.iter()
                    .map(|row| row.command.as_str())
                    .collect::<Vec<_>>(),
                ["menu-disabled", "menu-context"]
            );
            assert!(ours[0].disabled);
            assert!(!ours[1].disabled);
            assert_eq!(
                app.plugin_menu_context(&target, cx).path.as_deref(),
                Some("clicked.txt")
            );
            let editor = app.plugin_menu_target(active);
            assert!(
                app.plugin_menu_entries(&editor, &[Location::Selection], cx)
                    .is_empty()
            );
            for location in [Location::Editor, Location::Tab] {
                assert!(
                    app.plugin_menu_entries(&editor, &[location], cx)
                        .iter()
                        .any(|row| row.command == "menu-context")
                );
            }
            (target, ours[1].clone())
        });
        visual.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_explorer_menu(Some(clicked.clone()), point(px(160.), px(180.)), window, cx)
            })
        });
        visual.run_until_parked();
        let row = visual
            .debug_bounds("plugin-menu-native-menu-consumer/menu-context")
            .unwrap();
        visual.simulate_click(row.center(), Default::default());
        composable_tests::pump(&mut manager, &app, visual);
        composable_tests::publish(&mut manager, &mut renderer, &app, visual);
        let protocol::ui::Kind::Text { text } =
            &manager.live[&package.manifest.id].views["welcome"]
                .root
                .kind
        else {
            panic!()
        };
        let context: protocol::commands::Context = serde_json::from_str(text).unwrap();
        assert_eq!(
            context.path.as_deref(),
            Some("clicked.txt"),
            "clicking a tree entry must not retarget to the active editor"
        );
        // Real selection state enables only selection contributions; a later disable removes every location.
        visual.update(|_, cx| {
            app.read(cx)
                .editor
                .clone()
                .update(cx, |editor, cx| editor.set_selected_range(0..1, cx))
        });
        visual.update(|_, cx| {
            let app = app.read(cx);
            let editor = app.plugin_menu_target(active);
            assert!(
                app.plugin_menu_entries(&editor, &[Location::Selection], cx)
                    .iter()
                    .any(|row| row.command == "menu-context")
            );
        });
        manager.disable(&package.manifest.id).unwrap();
        composable_tests::publish(&mut manager, &mut renderer, &app, visual);
        visual.update(|window, cx| {
            app.update(cx, |app, cx| {
                assert!(
                    app.plugin_menu_entries(&target, &[Location::Explorer], cx)
                        .is_empty()
                );
                app.invoke_plugin_menu(&captured, &target, window, cx);
            })
        });
        composable_tests::pump(&mut manager, &app, visual);
        assert!(!manager.live.contains_key(&package.manifest.id));
        // A fresh menu contains no inactive row and cannot keep an obsolete native callback alive.
        visual.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_explorer_menu(Some(clicked), point(px(160.), px(180.)), window, cx)
            })
        });
        visual.run_until_parked();
        assert!(
            visual
                .debug_bounds("plugin-menu-native-menu-consumer/menu-context")
                .is_none()
        );
    });
}

/// Unicode text is entered through Base Input; confirmation reaches the guest's correlated task.
#[gpui::test]
#[ignore = "build capability-example with current --plugin-package before running"]
fn native_plugin_input_confirms_unicode_and_restores_editor_focus(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sample.txt");
    std::fs::write(&path, "sample").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let mut manager = plugin_runtime::Manager::open(
        directory.path().join("runtime"),
        protocol::Environment {
            workspace: workspace.root().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let package = interaction_package("native-interaction-consumer");
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, Some(path), window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    cx.simulate_resize(size(px(1200.), px(800.)));
    cx.update(|window, cx| app.read(cx).editor.focus_handle(cx).focus(window, cx));
    manager
        .invoke_command(
            &package.manifest.id,
            "scope-probe",
            serde_json::to_value(api::Operation::Editor {
                operation: api::EditorOperation::Interaction {
                    operation: protocol::interaction::Operation::Input {
                        title: "名字 / Name".into(),
                        value: String::new(),
                        placeholder: None,
                        password: false,
                        max_bytes: 64,
                    },
                },
                timeout_ms: 30000,
            })
            .unwrap(),
        )
        .unwrap();
    let request = manager
        .live
        .get_mut(&package.manifest.id)
        .unwrap()
        .take_editor_requests()
        .pop()
        .unwrap();
    cx.update(|_, cx| {
        app.read(cx).extensions.clone().update(cx, |owner, cx| {
            owner
                .worker
                .state
                .lock()
                .unwrap()
                .editor_requests
                .push((package.manifest.id.clone(), request.clone()));
            owner.poll(cx);
        })
    });
    cx.run_until_parked();
    cx.simulate_input("你好");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(matches!(request.status(), RequestUpdate::Completed {
        result: Ok(EditorValue::Interaction(protocol::interaction::Value::Input(text)))
    } if text == "你好"));
    assert!(cx.update(|window, cx| app.read(cx).editor.focus_handle(cx).is_focused(window)));
    manager.poll();
    let protocol::ui::Kind::Text { text } = &manager.live[&package.manifest.id].views["welcome"]
        .root
        .kind
    else {
        panic!()
    };
    assert!(
        text.contains("你好"),
        "the actual guest must receive its own completed choice"
    );
    // The second independent identity reuses public native controls, with a different active task.
    let second = interaction_package("second-interaction-consumer");
    manager
        .install(&second, second.manifest.permissions.clone())
        .unwrap();
    cx.update(|_, cx| apply_theme(builtin_theme(true), cx));
    let mut start = |operation| {
        manager
            .invoke_command(
                &second.manifest.id,
                "scope-probe",
                serde_json::to_value(api::Operation::Editor {
                    operation: api::EditorOperation::Interaction { operation },
                    timeout_ms: 30000,
                })
                .unwrap(),
            )
            .unwrap();
        let request = manager
            .live
            .get_mut(&second.manifest.id)
            .unwrap()
            .take_editor_requests()
            .pop()
            .unwrap();
        cx.update(|_, cx| {
            app.read(cx).extensions.clone().update(cx, |owner, cx| {
                owner
                    .worker
                    .state
                    .lock()
                    .unwrap()
                    .editor_requests
                    .push((second.manifest.id.clone(), request.clone()));
                owner.poll(cx);
            })
        });
        cx.run_until_parked();
        request
    };
    let pick = start(protocol::interaction::Operation::QuickPick {
        title: "选择 / Choose".into(),
        items: vec![
            protocol::interaction::PickItem {
                id: "alpha".into(),
                label: "Alpha".into(),
                description: None,
            },
            protocol::interaction::PickItem {
                id: "stable-choice".into(),
                label: "中文选项".into(),
                description: Some("Description".into()),
            },
        ],
    });
    drop(start);
    assert!(
        cx.debug_bounds("plugin-interaction-source-second-interaction-consumer")
            .is_some()
    );
    cx.simulate_input("中文");
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    assert!(
        matches!(pick.status(), RequestUpdate::Completed { result: Ok(EditorValue::Interaction(protocol::interaction::Value::Picked(id))) } if id == "stable-choice")
    );
    manager.poll();
    // Subsequent requests enter production publication without fabricating handles or UI state.
    let publish =
        |manager: &mut plugin_runtime::Manager, operation, cx: &mut gpui_kit::VisualTestContext| {
            manager
                .invoke_command(
                    &second.manifest.id,
                    "scope-probe",
                    serde_json::to_value(api::Operation::Editor {
                        operation: api::EditorOperation::Interaction { operation },
                        timeout_ms: 30000,
                    })
                    .unwrap(),
                )
                .unwrap();
            let request = manager
                .live
                .get_mut(&second.manifest.id)
                .unwrap()
                .take_editor_requests()
                .pop()
                .unwrap();
            cx.update(|_, cx| {
                app.read(cx).extensions.clone().update(cx, |owner, cx| {
                    owner
                        .worker
                        .state
                        .lock()
                        .unwrap()
                        .editor_requests
                        .push((second.manifest.id.clone(), request.clone()));
                    owner.poll(cx);
                })
            });
            cx.run_until_parked();
            request
        };
    let confirm = publish(
        &mut manager,
        protocol::interaction::Operation::Confirm {
            title: "Confirm".into(),
            message: "用户取消 / Cancelled by user".into(),
        },
        cx,
    );
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    confirm.finish(Ok(EditorValue::Interaction(
        protocol::interaction::Value::Confirmed,
    )));
    assert!(matches!(
        confirm.status(),
        RequestUpdate::Cancelled {
            reason: api::ErrorCode::Cancelled,
            ..
        }
    ));
    assert!(cx.update(|window, cx| app.read(cx).editor.focus_handle(cx).is_focused(window)));
    manager.poll();
    let notice = publish(
        &mut manager,
        protocol::interaction::Operation::Notify {
            title: "Notice".into(),
            message: "Second plugin".into(),
            severity: protocol::interaction::Severity::Information,
        },
        cx,
    );
    assert!(
        cx.update(|window, cx| app.read(cx).editor.focus_handle(cx).is_focused(window)),
        "nonmodal messages must retain editor focus"
    );
    let button = cx.debug_bounds("plugin-notice-dismiss").unwrap();
    cx.simulate_click(button.center(), Default::default());
    cx.run_until_parked();
    assert!(matches!(
        notice.status(),
        RequestUpdate::Completed {
            result: Ok(EditorValue::Interaction(
                protocol::interaction::Value::Dismissed
            ))
        }
    ));
    manager.poll();
    cx.update(|window, cx| app.read(cx).editor.focus_handle(cx).focus(window, cx));
    let progress = publish(
        &mut manager,
        protocol::interaction::Operation::Progress {
            title: "Check".into(),
            message: "Running".into(),
            cancellable: true,
        },
        cx,
    );
    assert!(cx.update(|window, cx| app.read(cx).editor.focus_handle(cx).is_focused(window)));
    manager
        .invoke_command(
            &second.manifest.id,
            "scope-probe",
            serde_json::to_value(api::Operation::Editor {
                operation: api::EditorOperation::Interaction {
                    operation: protocol::interaction::Operation::UpdateProgress {
                        request: progress.handle().clone(),
                        message: "一半 / Half".into(),
                        percent: Some(50),
                    },
                },
                timeout_ms: 30000,
            })
            .unwrap(),
        )
        .unwrap();
    assert_eq!(progress.progress(), ("一半 / Half".into(), Some(50)));
    let button = cx.debug_bounds("plugin-progress-cancel").unwrap();
    cx.simulate_click(button.center(), Default::default());
    cx.run_until_parked();
    assert!(matches!(
        progress.status(),
        RequestUpdate::Cancelled {
            reason: api::ErrorCode::Cancelled,
            ..
        }
    ));
    progress.finish(Ok(EditorValue::Interaction(
        protocol::interaction::Value::Finished,
    )));
    assert!(matches!(progress.status(), RequestUpdate::Cancelled { .. }));
    manager.poll();
    cx.run_until_parked();
    assert!(
        cx.update(|_, cx| app.read(cx).plugin_interactions.is_empty()),
        "all terminal controls must release their native state"
    );
}
