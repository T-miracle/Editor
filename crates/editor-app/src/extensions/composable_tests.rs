//! Compose real native controls and a keyed drawing through the existing worker publication seam.
use super::*;
use gpui_kit::{TestAppContext, gpui};
use protocol::ui::{Action, Canvas, CanvasEvent, Document, Input, Kind, Node};

/// Standard text input, optional grid and canvas focus must not share a keyboard/IME event target.
#[gpui::test]
fn native_input_and_canvas_route_text_to_their_own_nodes_and_hide_reclaims_targets(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    let document = Document::new(
        Node::column(
            "root",
            vec![
                Node::row(
                    "toolbar",
                    vec![
                        Node::button("zoom", "放大"),
                        Node::input("caption", Input::default()).grow(),
                    ],
                )
                .height(40.),
                Node::new(
                    "viewport",
                    Kind::Canvas(Canvas {
                        focusable: true,
                        paint: vec![protocol::Paint::Fill {
                            rect: protocol::Rect {
                                x: 0.,
                                y: 0.,
                                w: 200.,
                                h: 200.,
                            },
                            color: 0x5599aa,
                            extend_to_bottom: true,
                        }],
                        ..Default::default()
                    }),
                )
                .grow(),
            ],
        )
        .grow(),
    )
    .revision(9);
    cx.update(|window, cx| {
        app.read(cx).extensions.clone().update(cx, |owner, cx| {
            let manifest: protocol::Manifest = serde_json::from_str(include_str!(
                "../../../../plugins/capability-example/manifest.json"
            ))
            .unwrap();
            let mut state = owner.worker.state.lock().unwrap();
            state.entries = vec![Installed {
                grants: manifest.permissions.clone(),
                manifest,
                digest: "composed".into(),
                enabled: true,
                project_enabled: Default::default(),
                global_enabled: None,
                retired_ui_contract: false,
                error: None,
            }];
            state
                .views
                .insert("capability-example/welcome".into(), Arc::new(document));
            drop(state);
            owner.poll(cx);
        });
        app.update(cx, |app, cx| app.sync_plugin_panels(window, cx));
    });
    cx.simulate_resize(size(px(1200.), px(800.)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    let viewport = cx.debug_bounds("plugin-ui-viewport").unwrap();
    let input = cx.debug_bounds("plugin-ui-caption").unwrap();
    assert!(viewport.size.width > px(0.) && viewport.size.height > px(0.));
    assert!(viewport.top() >= input.bottom());
    let initial = events(&app, cx);
    assert!(initial.iter().any(|event| matches!(
        event.action,
        Action::Canvas(CanvasEvent::Resize { grid: None, .. })
    )));
    cx.simulate_click(input.center(), Default::default());
    cx.simulate_input("中文 空格");
    cx.run_until_parked();
    let typed = events(&app, cx);
    assert!(typed.iter().any(|event| event.node == "caption"
        && matches!(&event.action,Action::Change(text) if text.contains("中文 空格"))));
    assert!(!typed.iter().any(|event| matches!(
        event.action,
        Action::Canvas(CanvasEvent::Text { .. } | CanvasEvent::Key { .. })
    )));
    cx.simulate_click(viewport.center(), Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_input("画 布");
    cx.run_until_parked();
    let typed = events(&app, cx);
    let text = typed
        .iter()
        .filter_map(|event| {
            if let Action::Canvas(CanvasEvent::Text { text }) = &event.action {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<String>();
    assert_eq!(text, "画 布");
    assert!(
        !typed
            .iter()
            .any(|event| event.node == "caption" && matches!(event.action, Action::Change(_)))
    );
    cx.update(|_, cx| {
        app.update(cx, |app, cx| {
            app.hide_plugin_panel("capability-example", "welcome", cx)
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("plugin-ui-viewport").is_none());
    assert!(cx.update(|_, cx| {
        app.read(cx).plugin_panels["capability-example/welcome"]
            .read(cx)
            .native_ui
            .is_none()
    }));
}

/// Observe public node events as they enter the production worker channel, without reaching into widgets.
fn events(
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
) -> Vec<protocol::ui::UiEvent> {
    cx.update(|_, cx| {
        app.read(cx)
            .extensions
            .read(cx)
            .worker
            .recorded
            .lock()
            .unwrap()
            .try_iter()
            .filter_map(|work| {
                if let Work::Event(_, _, Some(_), event) = work {
                    if let PluginEvent::Ui(event) = event {
                        return Some(event);
                    }
                }
                None
            })
            .collect()
    })
}

/// Native clicks and unsaved editor input complete a real WASM → raster → GPUI round trip.
#[gpui::test]
#[ignore = "build capability-example through scripts/build-capability-example.ps1 first"]
fn composed_wasm_preview_follows_memory_and_reclaims_the_editor_split(cx: &mut TestAppContext) {
    use std::io::{Cursor, Write};
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("drawing.svg");
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="160" height="100"><rect width="160" height="100" fill="red"/></svg>"#;
    std::fs::write(&path, svg).unwrap();
    let mut files = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example.zip"),
    )
    .unwrap()
    .files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["settings_hook"] = serde_json::json!(false);
    manifest["settings"]["label"]["default"] = serde_json::json!("composable-ui");
    manifest["panels"][0]["position"] = serde_json::json!("editor");
    manifest["panels"][0]["file_extensions"] = serde_json::json!(["svg"]);
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
    let package = Package::from_bytes(&archive.finish().unwrap().into_inner()).unwrap();
    let mut manager = plugin_runtime::Manager::open(
        directory.path().join("runtime"),
        protocol::Environment {
            workspace: directory.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let mut renderer = images::VectorRenderer::default();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    cx.simulate_resize(size(px(1200.), px(800.)));
    publish(&mut manager, &mut renderer, &app, cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_file(path.clone(), window, cx)));
    cx.run_until_parked();
    pump(&mut manager, &app, cx);
    publish(&mut manager, &mut renderer, &app, cx);
    assert!(cx.debug_bounds("editor-plugin-layout").is_some());
    assert!(cx.debug_bounds("plugin-ui-caption").is_some());
    cx.update(|_, cx| {
        let owner = app.read(cx).extensions.read(cx);
        let images = &owner.worker.state.lock().unwrap().images;
        assert!(images["capability-example/welcome/canvas/viewport"][0].is_some());
    });
    let zoom = cx.debug_bounds("plugin-ui-zoom").unwrap();
    cx.simulate_click(
        point(zoom.left() + px(20.), zoom.center().y),
        Default::default(),
    );
    cx.run_until_parked();
    pump(&mut manager, &app, cx);
    publish(&mut manager, &mut renderer, &app, cx);
    let tree = manager.live["capability-example"].views["welcome"].as_ref();
    let Kind::Canvas(canvas) = &tree.active_node("viewport").unwrap().kind else {
        unreachable!()
    };
    assert!(matches!(canvas.paint[0], protocol::Paint::Svg {rect,..} if rect.w==200.));
    // The document's memory revision changes while disk content remains untouched.
    cx.update(|window, cx| {
        app.read(cx)
            .editor
            .clone()
            .update(cx, |editor, cx| editor.focus(window, cx))
    });
    cx.simulate_input("未保存 ");
    cx.run_until_parked();
    pump(&mut manager, &app, cx);
    publish(&mut manager, &mut renderer, &app, cx);
    let tree = manager.live["capability-example"].views["welcome"].as_ref();
    assert!(serde_json::to_string(tree).unwrap().contains("未保存 "));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), svg);
    assert!(tree.source.is_some());
    manager.uninstall("capability-example", true).unwrap();
    publish(&mut manager, &mut renderer, &app, cx);
    assert!(cx.debug_bounds("editor-plugin-layout").is_none());
    assert!(cx.debug_bounds("plugin-ui-viewport").is_none());
    assert!(cx.update(|_, cx| app.read(cx).plugin_panels.is_empty()));
}

/// Replay only production worker events; an old native callback is explicitly rejected, not a crash.
pub(super) fn pump(
    manager: &mut plugin_runtime::Manager,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
) {
    let mut launches = Vec::new();
    pump_recording(manager, app, cx, &mut launches);
}

/// Deliver queued worker work while recording each host launch the runtime accepted.
///
/// Host launches travel the same public contract any plugin uses, so the test harness performs
/// exactly what the production worker does with this work item.
///
/// Returns the provider answers collected for pending status queries, which a frame then publishes
/// the way the production worker does.
pub(super) fn pump_recording(
    manager: &mut plugin_runtime::Manager,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
    launches: &mut Vec<(u64, String, u64)>,
) -> (
    Vec<(String, u64, super::RunStatus)>,
    Vec<(String, u64, Result<(), String>)>,
) {
    pump_recording_and_debug(manager, app, cx, launches, &mut BTreeMap::new())
}

/// Keep deferred debug requests across frames, matching the production actor's nonblocking loop.
/// This is UI transport instrumentation only; every call still crosses the public runtime manager.
pub(super) fn pump_recording_and_debug(
    manager: &mut plugin_runtime::Manager,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
    launches: &mut Vec<(u64, String, u64)>,
    debug: &mut BTreeMap<u64, (String, plugin_runtime::DebugRequest)>,
) -> (
    Vec<(String, u64, super::RunStatus)>,
    Vec<(String, u64, Result<(), String>)>,
) {
    // The same target dispatcher is retained by native target acceptance; older execution fixtures need none.
    pump_recording_all(
        manager,
        app,
        cx,
        launches,
        debug,
        &mut super::worker::targets::TargetCalls::default(),
        &mut super::worker::configurations::ConfigurationCalls::default(),
    )
}

/// Retain both independent deferred transports; every domain response still originates in real WASM.
pub(super) fn pump_recording_all(
    manager: &mut plugin_runtime::Manager,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
    launches: &mut Vec<(u64, String, u64)>,
    debug: &mut BTreeMap<u64, (String, plugin_runtime::DebugRequest)>,
    targets: &mut super::worker::targets::TargetCalls,
    configurations: &mut super::worker::configurations::ConfigurationCalls,
) -> (
    Vec<(String, u64, super::RunStatus)>,
    Vec<(String, u64, Result<(), String>)>,
) {
    let mut stop_results: Vec<(String, u64, Result<(), String>)> = Vec::new();
    let mut work: Vec<_> = cx.update(|_, cx| {
        let recorder = app
            .read(cx)
            .extensions
            .read(cx)
            .worker
            .recorded
            .lock()
            .unwrap();
        let mut collected = Vec::new();
        // Drain by receive rather than by iterator: an item staged between two frames must not be
        // skipped, which is exactly what a status query depends on.
        while let Ok(item) = recorder.try_recv() {
            collected.push(item);
        }
        collected
    });
    // A host launch reaches the runtime through the same public contract any plugin uses, so the
    // test harness performs exactly what the production worker would do with this work item. These
    // are taken first so the ordinary branches below keep their by-value patterns.
    // A status query observes one step's program, so the harness asks the same runtime the
    // production worker would ask and records the answer where the worker publishes it.
    let published = cx.update(|_, cx| app.read(cx).extensions.read(cx).worker.state.clone());
    work = work
        .into_iter()
        .filter_map(|work| work.admit(manager, &published))
        .collect();
    work = work
        .into_iter()
        .filter_map(|work| targets.dispatch(work, manager, &published))
        .collect();
    targets.poll(manager, &published);
    // Mirror the production actor for actual configuration packages, retaining their deferred requests.
    work = work
        .into_iter()
        .filter_map(|work| match work {
            Work::ConfigurationCatalog { .. }
            | Work::ConfigurationCall { .. }
            | Work::CancelConfigurations { .. } => {
                configurations.dispatch(work, manager, &published);
                None
            }
            other => Some(other),
        })
        .collect();
    configurations.poll(manager, &published);
    let mut statuses: Vec<(String, u64, super::RunStatus)> = Vec::new();
    work.retain(|item| {
        let Work::ForceDebug { session, request } = item else {
            return true;
        };
        // Mirror production: ownership revocation is admission, not a native completion receipt.
        let pending = manager.force_debug_session(session).unwrap();
        debug.insert(*request, ("stop".into(), pending));
        false
    });
    work.retain(|item| {
        let Work::DebugCall {
            request,
            configuration,
            method,
            arguments,
        } = item
        else {
            return true;
        };
        let answer = manager.begin_configured_debug_call(
            configuration.as_deref(),
            method,
            arguments.clone(),
        );
        cx.update(|_, cx| {
            let mut state = app
                .read(cx)
                .extensions
                .read(cx)
                .worker
                .state
                .lock()
                .unwrap();
            match answer {
                Ok(pending) => {
                    if method == "start" {
                        state.debug_answers.push((
                            *request,
                            super::DebugAnswerMessage::Connecting(pending.session().into()),
                        ));
                    }
                    debug.insert(*request, (method.clone(), pending));
                }
                Err(error) => state.debug_answers.push((
                    *request,
                    super::DebugAnswerMessage::Failed(format!("{error:#}")),
                )),
            }
        });
        false
    });
    debug.retain(|request, (method, pending)| {
        let result = match pending.status() {
            protocol::api::RequestUpdate::Completed { result } => {
                result.map_err(|error| error.message)
            }
            protocol::api::RequestUpdate::Cancelled { reason, .. } => {
                Err(format!("Debug request cancelled: {reason:?}"))
            }
            _ => return true,
        };
        cx.update(|_, cx| {
            app.read(cx)
                .extensions
                .read(cx)
                .worker
                .state
                .lock()
                .unwrap()
                .debug_answers
                .push((*request, super::worker::debug_answer(method, result)))
        });
        false
    });
    work.retain(|item| match item {
        Work::LocateRun { session, request } => {
            let completion = manager
                .locate_execution(*session)
                .expect("the pinned provider can locate its session");
            manager.poll_request(&completion);
            assert!(matches!(
                completion.status(),
                protocol::api::RequestUpdate::Completed { result: Ok(_) }
            ));
            cx.update(|_, cx| {
                app.read(cx)
                    .extensions
                    .read(cx)
                    .worker
                    .state
                    .lock()
                    .unwrap()
                    .locate_results
                    .push((*session, *request, Ok(())))
            });
            false
        }
        _ => true,
    });
    // A stop is performed exactly as the production worker performs it: through the session's own
    // provider, with the answer recorded where the worker publishes it.
    work.retain(|item| match item {
        Work::StopRun {
            session,
            config,
            mode,
            request_id,
        } => {
            let result = manager
                .stop_execution_with(
                    *session,
                    plugin_runtime::StopOptions {
                        mode: *mode,
                        ..Default::default()
                    },
                )
                .map_err(|error| format!("{error:#}"));
            stop_results.push((config.clone(), *request_id, result));
            false
        }
        _ => true,
    });
    cx.update(|_, cx| {
        let mut pending = app
            .read(cx)
            .extensions
            .read(cx)
            .worker
            .run_queries
            .lock()
            .unwrap();
        work.retain(|item| match item {
            Work::PollRun {
                session,
                config,
                request_id,
            } => {
                match manager.query_execution(*session) {
                    Ok(completion) => {
                        pending.insert((config.clone(), *request_id), completion);
                    }
                    Err(_) => {
                        statuses.push((config.clone(), *request_id, super::RunStatus::Unknown))
                    }
                }
                false
            }
            _ => true,
        });
        // Pending is a lifecycle state. Only a real terminal receipt can finish the sequence.
        pending.retain(|(config, request), completion| {
            manager.poll_request(completion);
            let status = match completion.status() {
                protocol::api::RequestUpdate::Completed { result: Ok(value) } => {
                    super::RunStatus::from_value(&value)
                }
                status if status.is_terminal() => super::RunStatus::Unknown,
                _ => return true,
            };
            statuses.push((config.clone(), *request, status));
            false
        });
    });
    let _ = &mut statuses;
    work.retain(|item| match item {
        Work::StartRun {
            request,
            config,
            request_id,
        } => {
            let session = match manager.start_configuration_execution(config, request.clone()) {
                Ok(session) => session,
                Err(error) => panic!(
                    "a compatible execution provider is installed: {error:#} (request {request:?})"
                ),
            };
            launches.push((session.id(), config.clone(), *request_id));
            false
        }
        _ => true,
    });
    for work in work {
        let work = match work {
            Work::ImportPreference {
                plugin,
                owner,
                workspace,
                key,
                data,
                ..
            } => {
                let succeeded = manager
                    .import_preference(&plugin, key, data.clone())
                    .is_ok();
                cx.update(|_, cx| {
                    super::package_ui_test_support::owner_receipt(
                        &owner,
                        &workspace,
                        data,
                        succeeded,
                        &app.read(cx).extensions.read(cx).worker,
                    )
                });
                continue;
            }
            work => work,
        };
        // Lifecycle UI acceptance uses the same ordinary manager entries as the worker actor.
        // These branches never manufacture a session state or reach into provider internals.
        match &work {
            Work::Disable(id) => {
                manager.disable(id).unwrap();
                continue;
            }
            Work::Uninstall(id, delete_data) => {
                manager.uninstall(id, *delete_data).unwrap();
                continue;
            }
            Work::Invoke {
                plugin,
                command,
                arguments,
                expected_epoch,
            } => {
                // Use the actor's real-instance admission after any earlier lifecycle work in
                // this batch; a queued callback must not borrow the replacement's ownership.
                let accepted = cx.update(|_, cx| {
                    let worker = &app.read(cx).extensions.read(cx).worker;
                    worker
                        .state
                        .lock()
                        .unwrap()
                        .admit_command(
                            manager,
                            worker.trusted.load(std::sync::atomic::Ordering::Acquire),
                            plugin,
                            command,
                            *expected_epoch,
                        )
                        .is_ok()
                });
                if !accepted {
                    continue;
                }
                let result = manager.invoke_command(plugin, command, arguments.clone());
                // Fault acceptance still uses the ordinary typed callback and published failure.
                if let Err(error) = result {
                    assert!(
                        manager
                            .published_entries()
                            .iter()
                            .any(|entry| &entry.manifest.id == plugin && entry.error.is_some()),
                        "{error:#}"
                    );
                }
                continue;
            }
            _ => {}
        }
        if let Work::ImageInput {
            plugin,
            panel,
            document,
            selection,
            origin,
            images,
            reservation,
            ..
        } = work
        {
            manager
                .offer_image_input(&plugin, &panel, document, selection, origin, images)
                .unwrap();
            drop(reservation);
            continue;
        }
        if let Work::Event(id, _, panel, event) = work {
            // A surface can enqueue its last resize before retirement publication, just as in
            // production. The retired provider never receives that stale callback.
            if !manager.live.contains_key(&id) {
                continue;
            }
            if let Err(error) = manager.event(&id, panel, event) {
                assert!(
                    matches!(error.downcast_ref::<protocol::api::Failure>(), Some(error) if error.code==protocol::api::ErrorCode::StaleRevision),
                    "{error:#}"
                );
            }
        }
    }
    (statuses, stop_results)
}

/// The existing worker publication seam also supplies the actual asynchronous vector renderer output.
pub(super) fn publish(
    manager: &mut plugin_runtime::Manager,
    renderer: &mut images::VectorRenderer,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
) {
    publish_with_launches(manager, renderer, app, cx, &[]);
}

/// Publish the worker's view while joining host sessions to the launches that requested them.
pub(super) fn publish_with_launches(
    manager: &mut plugin_runtime::Manager,
    renderer: &mut images::VectorRenderer,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
    launches: &[(u64, String, u64)],
) {
    // A frame on its own carries no pending provider answer.
    publish_frame(manager, renderer, app, cx, launches, Vec::new(), Vec::new());
}

/// Publish one frame together with the answers the harness collected for pending status queries.
pub(super) fn publish_frame(
    manager: &mut plugin_runtime::Manager,
    renderer: &mut images::VectorRenderer,
    app: &Entity<EditorApp>,
    cx: &mut gpui_kit::VisualTestContext,
    launches: &[(u64, String, u64)],
    mut statuses: Vec<(String, u64, super::RunStatus)>,
    stop_results: Vec<(String, u64, Result<(), String>)>,
) {
    let scenes: BTreeMap<_, _> = manager
        .live
        .iter()
        .flat_map(|(id, instance)| {
            instance
                .views
                .iter()
                .map(move |(panel, scene)| (format!("{id}/{panel}"), scene.clone()))
        })
        .collect();
    let resources = manager.image_resources();
    // The real worker forwards typed presentation requests to the window. Provider location must
    // reveal only the panel it asked for; the harness does not guess from its manifest.
    let editor_requests = manager
        .live
        .iter_mut()
        .flat_map(|(id, instance)| {
            instance
                .take_editor_requests()
                .into_iter()
                .map(|request| (id.clone(), request))
        })
        .collect::<Vec<_>>();
    let images = renderer.prepare_resources(&scenes, &resources);
    // Host sessions are published through the same shared state the production worker writes, so the
    // editor's reconciliation runs here exactly as it does in a real frame.
    let host_executions = manager
        .executions()
        .into_iter()
        .map(|session| {
            let id = session.id();
            let snapshot = session.snapshot();
            (
                id,
                snapshot.plugin,
                snapshot.state,
                snapshot.provider_session,
                snapshot.failure.map(|failure| failure.message),
            )
        })
        .collect::<Vec<_>>();
    cx.update(|window, cx| {
        app.read(cx).extensions.clone().update(cx, |owner, cx| {
            let mut state = owner.worker.state.lock().unwrap();
            // A successfully opened Manager, rather than fabricated readiness or epochs,
            // supplies the same complete ownership publication as the production actor.
            state.publish_manager(manager);
            state.diagnostics = manager
                .installed
                .keys()
                .map(|id| (id.clone(), manager.diagnostics(id)))
                .collect();
            state.processes = manager
                .live
                .iter()
                .map(|(id, instance)| (id.clone(), instance.process_count()))
                .collect();
            state.debug_observations = manager.debug_observations();
            state.debug_availability = Some(manager.debug_availability());
            state.debug_abilities = manager
                .debug_availability()
                .ok()
                .and_then(|provider| manager.debug_abilities(&provider));
            state.debug_provider_abilities = manager.all_debug_abilities();
            state.views = scenes;
            state.images = images;
            state.editor_requests.extend(editor_requests);
            // A provider answer about a preparation step reaches the UI the way the worker publishes it.
            state.run_status.extend(statuses.drain(..));
            state.stop_results.extend(stop_results);
            // The launch identity recorded by the harness joins each published session to its request.
            state.host_executions = host_executions
                .iter()
                .map(|(id, plugin, state_, provider, failure)| {
                    let (config, request_id) = launches
                        .iter()
                        .find(|(session, _, _)| session == id)
                        .map(|(_, config, request_id)| (config.clone(), *request_id))
                        .unwrap_or_default();
                    HostRunSnapshot {
                        id: *id,
                        config,
                        request_id,
                        plugin: plugin.clone(),
                        state: *state_,
                        provider_session: provider.clone(),
                        failure: failure.clone(),
                    }
                })
                .collect();
            drop(state);
            owner.poll(cx);
        });
        app.update(cx, |app, cx| {
            app.sync_plugin_panels(window, cx);
            // A frame reconciles published sessions before painting, which is what this mirrors.
            app.sync_run_controls(window, cx);
        });
        for panel in app
            .read(cx)
            .plugin_panels
            .values()
            .cloned()
            .collect::<Vec<_>>()
        {
            panel.update(cx, |panel, cx| panel.poll(cx));
        }
        window.draw(cx).clear(cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}
