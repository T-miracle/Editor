//! Observable regressions discovered while reviewing the complete run/debug/build batch.
use super::*;

/// A target preparation settles its own pending start even when no interactive session is created.
#[test]
fn provider_preparation_receipts_release_pending_starts() {
    for success in [true, false] {
        let mut controls = RunControls::default();
        let mut config = local_config("provided");
        config.target = RunTarget::Provided {
            provider: "custom-targets".into(),
            binding: "{}".into(),
            label: "Target".into(),
            args: vec![],
        };
        config.build.push(editor_core::RunStep {
            name: "Build target".into(),
            target: editor_core::StepTarget::Action {
                target: config.target.clone(),
            },
        });
        controls.upsert(config, "C:/work").unwrap();
        let plan = controls
            .prepare_build("provided", "C:/work", MAX_PREPARED_STEPS)
            .unwrap();
        let request = controls.begin("provided");
        controls.begin_build("provided", &plan, request);
        // The real title bar reserves a plan identity, then stages a distinct step request.
        let request = controls.begin("provided");
        controls.note_step_request("provided", 0, request);
        assert!(controls.provider_prepared(
            "provided",
            0,
            request,
            if success {
                Ok("artifact.exe")
            } else {
                Err("Compiler failed")
            }
        ));
        assert!(
            !controls.is_pending("provided"),
            "terminal provider receipt must release only its original request"
        );
        assert!(
            !controls.has_work_in_flight(),
            "completed/failed Build owns no running work"
        );
    }
}

/// Repeated publications describe historical sessions without resurrecting their finished steps.
#[test]
fn ended_session_publications_cannot_restart_a_finished_sequence() {
    let mut controls = RunControls::default();
    controls.upsert(local_config("run"), "C:/work").unwrap();
    let plan = controls
        .prepare_launch("run", "C:/work", MAX_PREPARED_STEPS)
        .unwrap();
    let reserved = controls.begin("run");
    controls.begin_sequence("run", plan, reserved);
    let request = controls.begin("run");
    controls.note_step_request("run", 0, request);
    let publication = [(7, request, Some("original".into()))];
    controls.adopt_step_sessions(&publication);
    assert!(controls.observe_step("run", 0, StepOutcome::Exited { code: 0 }));
    assert!(!controls.is_preparing("run"));
    controls.adopt_step_sessions(&publication);
    assert!(
        !controls.is_preparing("run"),
        "ended publication is history, never a new running step"
    );
}

/// Saving an unrelated name edit must preserve literal argv, scripts, references and build order.
#[test]
fn draft_roundtrip_preserves_steps_and_provider_build_positions() {
    let mut config = local_config("provided");
    config.target = RunTarget::Provided {
        provider: "custom".into(),
        binding: "{}".into(),
        label: "Target".into(),
        args: vec!["".into(), "multi\nline".into()],
    };
    config.build = vec![
        editor_core::RunStep {
            name: "generate".into(),
            target: editor_core::StepTarget::Action {
                target: RunTarget::Program {
                    program: "tool.exe".into(),
                    args: vec!["a|b".into(), "".into(), " leading ".into()],
                },
            },
        },
        editor_core::RunStep {
            name: "build".into(),
            target: editor_core::StepTarget::Action {
                target: config.target.clone(),
            },
        },
    ];
    config.prelaunch = vec![editor_core::RunStep {
        name: "pipe = text".into(),
        target: editor_core::StepTarget::Action {
            target: RunTarget::Script {
                interpreter: "powershell.exe".into(),
                args: vec!["-NoProfile".into()],
                script: "'中文|literal'\nWrite-Output done".into(),
            },
        },
    }];
    let mut draft = RunConfigDraft::from_config(Some(&config), "ignored".into());
    draft.name = "renamed".into();
    let result = draft.to_config().unwrap();
    assert_eq!(
        serde_json::to_value(&result.build).unwrap(),
        serde_json::to_value(&config.build).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&result.prelaunch).unwrap(),
        serde_json::to_value(&config.prelaunch).unwrap()
    );
    assert_eq!(result.literal_arguments(), config.literal_arguments());
}

/// A failed contributor retains its old identities while successful contributors remain usable.
#[test]
fn discovery_failures_preserve_sources_without_hiding_other_candidates() {
    let mut controls = RunControls::default();
    let candidate = |provider: &str| plugin_schema::DiscoveredTarget {
        id: provider.into(),
        provider: provider.into(),
        target_type: "tool@1".into(),
        label: provider.into(),
        program: provider.into(),
        found_in: "project.json".into(),
        fields: [("provider_binding".into(), "{}".into())].into(),
    };
    controls.reconcile_discovered(&[candidate("a")]);
    let a = controls.confirm_target("a", "C:/work").unwrap();
    let (_, error) = controls
        .accept_target_catalog(crate::extensions::TargetCatalog {
            candidates: vec![candidate("b")],
            failed: [("a".into(), "tool missing".into())].into(),
            declarative_error: None,
        })
        .unwrap();
    assert!(error.contains("a: tool missing"));
    assert!(!controls.target_missing(&a));
    assert!(
        controls
            .launch_blocker(&a)
            .unwrap()
            .contains("tool missing")
    );
    let b = controls.confirm_target("b", "C:/work").unwrap();
    assert!(controls.launch_blocker(&b).is_none());
    controls
        .accept_target_catalog(crate::extensions::TargetCatalog {
            candidates: vec![candidate("b")],
            ..Default::default()
        })
        .unwrap();
    assert!(
        controls.target_missing(&a),
        "successful empty source invalidates its old target"
    );
}

/// A harmless local target used without invoking any executable in these persistence checks.
fn local_config(id: &str) -> RunConfig {
    RunConfig {
        id: id.into(),
        name: id.into(),
        target: RunTarget::Program {
            program: "tool.exe".into(),
            args: vec![],
        },
        directory: None,
        env: Default::default(),
        tool_paths: vec![],
        build: vec![],
        prelaunch: vec![],
        source: editor_core::RunConfigSource::Local,
        from_target: None,
        provider: None,
        breakpoints: Default::default(),
        local: true,
    }
}

/// A retained real pause and a queued start both require the same native leave confirmation.
#[test]
fn debug_programs_are_owned_work_until_their_real_exit() {
    let mut controls = RunControls::default();
    controls.begin_debug_session("debug");
    controls.note_debug_state(
        "debug",
        editor_core::DebugSessionState::Paused {
            source: "main.rs".into(),
            line: 8,
            reason: None,
        },
    );
    assert!(
        controls.has_work_in_flight(),
        "a paused debugger still owns a native target tree"
    );
    controls.note_debug_state("debug", editor_core::DebugSessionState::Exited);
    assert!(
        !controls.has_work_in_flight(),
        "finished debug history alone does not block leaving"
    );
    controls.begin_debug_session("starting");
    controls.begin_debug_start_request("starting").unwrap();
    assert!(
        controls.has_work_in_flight(),
        "queued adapter creation must be confirmed before leaving"
    );
}

/// Changing the start default cannot revoke optional controls on an already pinned live session.
#[test]
fn live_debug_controls_keep_the_original_provider_capabilities() {
    let mut controls = RunControls::default();
    controls.note_debug_availability(Ok("original".into()));
    controls.note_debug_capabilities(editor_core::DebugCapabilities {
        breakpoints: true,
        resume_pause: true,
        step: true,
        inspect: true,
    });
    controls.begin_debug_session("a");
    controls.note_debug_provider_owner("a", "original");
    controls.note_debug_state(
        "a",
        editor_core::DebugSessionState::Paused {
            source: "a.rs".into(),
            line: 8,
            reason: None,
        },
    );
    assert!(controls.debug_controls().step[0].1.is_ok());
    controls.note_debug_availability(Ok("new-default".into()));
    controls.note_debug_capabilities(editor_core::DebugCapabilities::default());
    assert!(
        controls.debug_controls().step[0].1.is_ok(),
        "the target still belongs to original, which supports stepping"
    );
}

/// Location arrives after the paused receipt; epochs also distinguish repeated stops on one line.
#[test]
fn debug_pause_epochs_locate_once_and_reject_late_state() {
    let mut controls = RunControls::default();
    controls.begin_debug_session("debug");
    controls.note_debug_provider_session("debug", "debug-1");
    let mut report = plugin_runtime::DebugSession::from_value(
        &serde_json::json!({"session":"debug-1","state":"paused","pause":1}),
    )
    .unwrap();
    assert!(
        !controls.observe_debug_report(&report),
        "no source has been reported yet"
    );
    report.source = Some("main.rs".into());
    report.line = Some(8);
    assert!(
        controls.observe_debug_report(&report),
        "the actual source arriving must locate the first pause"
    );
    assert!(
        !controls.observe_debug_report(&report),
        "repeated status does not move the caret again"
    );
    let scope = controls.debug_pause_scope().unwrap();
    let request = controls
        .begin_debug_request(DebugMethod::Control(DebugControl::Resume), None)
        .unwrap();
    controls.note_debug_action();
    report.pause = Some(3);
    assert!(
        controls.observe_debug_report(&report),
        "a new stop at the same line is a new pause"
    );
    assert!(
        controls.apply_debug_frames(scope, vec![]).is_err(),
        "old pause references expired"
    );
    let old = plugin_runtime::DebugSession::from_value(
        &serde_json::json!({"session":"debug-1","state":"running","pause":2}),
    )
    .unwrap();
    controls.apply_debug_state_reply(request, &old).unwrap();
    assert!(!controls.debug_action_pending());
    assert!(
        matches!(
            controls.debug_state(),
            editor_core::DebugSessionState::Paused { line: 8, .. }
        ),
        "late control replies cannot overwrite the newer stop"
    );
}

/// Historical locations settle once; a superseded selection cannot overwrite a newer status.
#[test]
fn historical_session_location_uses_its_own_request_identity() {
    let mut controls = RunControls::default();
    controls
        .upsert(local_config("first"), "location-test")
        .unwrap();
    controls.select("first", "location-test");
    let older = controls.begin_location(1, "first");
    let newer = controls.begin_location(2, "first");
    assert!(!controls.finish_location(1, older));
    assert!(
        controls.finish_location(2, newer),
        "a retained ended session requires no active program"
    );
    assert!(
        !controls.finish_location(2, newer),
        "the result is consumed once"
    );
    let changed = controls.begin_location(1, "first");
    controls
        .upsert(local_config("second"), "location-test")
        .unwrap();
    controls.select("second", "location-test");
    assert!(!controls.finish_location(1, changed));
}

/// Selecting an entry persists a machine preference without replacing a hand-edited project file.
#[test]
fn selecting_a_configuration_preserves_external_shared_edits() {
    let project = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();
    let workspace = project.path().display().to_string();
    let mut controls = RunControls::load_with_project(
        &workspace,
        Some(local.path().into()),
        Some(project.path().into()),
    );
    let mut shared = local_config("shared");
    shared.local = false;
    controls.upsert(shared, &workspace).unwrap();
    let mut file = editor_core::load_shared(project.path()).unwrap();
    file.configurations[0].name = "edited in the file".into();
    editor_core::save_shared(project.path(), &file).unwrap();
    let before = std::fs::read(editor_core::project_path(project.path())).unwrap();
    assert!(controls.select("shared", &workspace));
    assert_eq!(
        std::fs::read(editor_core::project_path(project.path())).unwrap(),
        before
    );
}

/// A cached project definition is never a substitute for a malformed or deleted shared definition.
#[test]
fn unreadable_or_deleted_shared_files_do_not_launch_cached_definitions() {
    let project = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();
    let workspace = project.path().display().to_string();
    let mut controls = RunControls::load_with_project(
        &workspace,
        Some(local.path().into()),
        Some(project.path().into()),
    );
    let mut shared = local_config("shared");
    shared.local = false;
    controls.upsert(shared, &workspace).unwrap();
    let path = editor_core::project_path(project.path());
    std::fs::write(&path, b"{broken").unwrap();
    let broken = RunControls::load_with_project(
        &workspace,
        Some(local.path().into()),
        Some(project.path().into()),
    );
    assert!(broken.error.is_some());
    assert!(broken.launch_blocker("shared").is_some());
    assert!(
        broken
            .prepare_launch("shared", &workspace, MAX_PREPARED_STEPS)
            .is_err()
    );
    std::fs::remove_file(path).unwrap();
    let deleted = RunControls::load_with_project(
        &workspace,
        Some(local.path().into()),
        Some(project.path().into()),
    );
    assert!(deleted.launch_blocker("shared").is_some());
}

/// Refusing a private path neither publishes it nor replaces the last valid local definition.
#[test]
fn refusing_a_private_shared_path_preserves_the_previous_configuration() {
    let project = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();
    let workspace = project.path().display().to_string();
    let mut controls = RunControls::load_with_project(
        &workspace,
        Some(local.path().into()),
        Some(project.path().into()),
    );
    controls.upsert(local_config("run"), &workspace).unwrap();
    let mut edited = local_config("run");
    edited.local = false;
    edited.target = RunTarget::Program {
        program: "C:/Users/private/tool.exe".into(),
        args: vec![],
    };
    assert!(controls.upsert(edited, &workspace).is_err());
    assert_eq!(
        controls.configuration("run").unwrap().target.executable(),
        "tool.exe"
    );
    assert!(!editor_core::project_path(project.path()).exists());
}

/// A late response cannot bind to a newer request after an earlier request has been released.
#[test]
fn debug_request_identities_are_not_reused() {
    let mut controls = RunControls::default();
    controls.note_debug_state(
        "a",
        editor_core::DebugSessionState::Paused {
            source: "a.rs".into(),
            line: 1,
            reason: None,
        },
    );
    controls.begin_debug_pause().unwrap();
    let old = controls
        .begin_debug_request(DebugMethod::Frames, None)
        .unwrap();
    controls.abandon_debug_request(old);
    let new = controls
        .begin_debug_request(DebugMethod::Frames, None)
        .unwrap();
    assert!(new > old);
    assert_eq!(
        controls.apply_debug_answer(old, Some(vec![]), None),
        Err(editor_core::InspectionError::NoSession)
    );
    assert_eq!(controls.pending_debug_requests(), 1);
}

/// Two sessions can both have their first pause; the pause number alone does not identify its owner.
#[test]
fn debug_inspection_does_not_cross_session_selection() {
    let mut controls = RunControls::default();
    controls.note_debug_state(
        "a",
        editor_core::DebugSessionState::Paused {
            source: "a.rs".into(),
            line: 1,
            reason: None,
        },
    );
    controls.begin_debug_pause().unwrap();
    let request = controls
        .begin_debug_request(DebugMethod::Frames, None)
        .unwrap();
    controls.note_debug_state(
        "b",
        editor_core::DebugSessionState::Paused {
            source: "b.rs".into(),
            line: 1,
            reason: None,
        },
    );
    assert!(controls.select_debug_session("b"));
    controls.begin_debug_pause().unwrap();
    let answer = controls.apply_debug_answer(
        request,
        Some(vec![editor_core::StackFrame {
            id: 0,
            name: "from a".into(),
            source: "a.rs".into(),
            line: 1,
        }]),
        None,
    );
    assert_eq!(answer, Err(editor_core::InspectionError::WrongSession));
    assert!(controls.debug_frames().is_empty());
}

/// Discovery offers corrections without silently replacing the executable a user has stored.
#[test]
fn rediscovery_never_changes_a_stored_program_without_confirmation() {
    let mut controls = RunControls::default();
    let mut mine = local_config("run");
    mine.from_target = Some("target".into());
    controls.upsert(mine, "C:/work").unwrap();
    controls.reconcile_discovered(&[plugin_schema::DiscoveredTarget {
        id: "target".into(),
        provider: "provider".into(),
        target_type: "binary".into(),
        label: "candidate".into(),
        program: "new-tool.exe".into(),
        fields: Default::default(),
        found_in: "project.json".into(),
    }]);
    assert_eq!(
        controls.configuration("run").unwrap().target.executable(),
        "tool.exe"
    );
}

/// A step changes its original target even if the user inspects another target before the reply.
#[test]
fn a_step_response_updates_its_owner_without_changing_selection() {
    let mut controls = RunControls::default();
    let paused = |source: &str, line| editor_core::DebugSessionState::Paused {
        source: source.into(),
        line,
        reason: None,
    };
    controls.note_debug_state("a", paused("a.rs", 1));
    controls.begin_debug_pause().unwrap();
    let request = controls
        .begin_debug_request(DebugMethod::Step(editor_core::DebugStep::Over), None)
        .unwrap();
    controls.note_debug_state("b", paused("b.rs", 2));
    controls.select_debug_session("b");
    controls.begin_debug_pause().unwrap();
    assert_eq!(
        controls.apply_debug_step(request, paused("a.rs", 3)),
        Ok(())
    );
    assert_eq!(controls.debug_state(), paused("b.rs", 2));
    controls.select_debug_session("a");
    assert_eq!(controls.debug_state(), paused("a.rs", 3));
}

/// Failure to commit this machine's values must happen before any project command is published.
#[test]
fn a_failed_local_save_does_not_publish_or_remove_a_shared_definition() {
    let project = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();
    let workspace = project.path().display().to_string();
    let mut controls = RunControls::load_with_project(
        &workspace,
        Some(local.path().into()),
        Some(project.path().into()),
    );
    let mut shared = local_config("shared");
    shared.local = false;
    controls.upsert(shared, &workspace).unwrap();
    let path = editor_core::project_path(project.path());
    let before = std::fs::read(&path).unwrap();
    // A directory at the atomic-write staging path is a portable, deterministic I/O failure.
    let local_path = std::fs::read_dir(local.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::create_dir(local_path.with_extension("json.tmp")).unwrap();
    let mut edited = controls.configuration("shared").unwrap().clone();
    edited.name = "rejected edit".into();
    assert!(controls.upsert(edited, &workspace).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(controls.configuration("shared").unwrap().name, "shared");
    assert!(controls.remove("shared", &workspace).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(controls.configuration("shared").is_some());
}

/// A failed project write also restores the local overrides that were staged for that edit.
#[test]
fn a_failed_shared_save_restores_local_values() {
    let project = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();
    let workspace = project.path().display().to_string();
    let mut controls = RunControls::load_with_project(
        &workspace,
        Some(local.path().into()),
        Some(project.path().into()),
    );
    let mut shared = local_config("shared");
    shared.local = false;
    controls.upsert(shared, &workspace).unwrap();
    let previous = editor_core::load(local.path(), &workspace).unwrap();
    let project_path = editor_core::project_path(project.path());
    let bytes = std::fs::read(&project_path).unwrap();
    std::fs::create_dir(project_path.with_extension("json.tmp")).unwrap();
    let mut edited = controls.configuration("shared").unwrap().clone();
    edited.name = "rejected edit".into();
    edited
        .env
        .insert("NEW_LOCAL_VALUE".into(), "private".into());
    assert!(controls.upsert(edited, &workspace).is_err());
    assert_eq!(
        editor_core::load(local.path(), &workspace)
            .unwrap()
            .to_json()
            .unwrap(),
        previous.to_json().unwrap()
    );
    assert_eq!(std::fs::read(project_path).unwrap(), bytes);
}

/// New launches read the current project document while retaining their own local overrides.
#[test]
fn launch_planning_rereads_shared_files_in_the_same_window() {
    let project = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();
    let workspace = project.path().display().to_string();
    let mut controls = RunControls::load_with_project(
        &workspace,
        Some(local.path().into()),
        Some(project.path().into()),
    );
    let mut shared = local_config("shared");
    shared.local = false;
    controls.upsert(shared, &workspace).unwrap();
    let mut file = editor_core::load_shared(project.path()).unwrap();
    file.configurations[0].target = RunTarget::Program {
        program: "new-tool.exe".into(),
        args: vec![],
    };
    editor_core::save_shared(project.path(), &file).unwrap();
    let plan = controls.launch_plan("shared", &workspace).unwrap();
    assert_eq!(plan.steps.last().unwrap().request.program, "new-tool.exe");
    let path = editor_core::project_path(project.path());
    std::fs::write(&path, b"{broken").unwrap();
    assert!(controls.launch_plan("shared", &workspace).is_err());
    assert!(
        controls
            .debug_launch_request("shared", &workspace)
            .is_none()
    );
    std::fs::remove_file(path).unwrap();
    assert!(controls.launch_plan("shared", &workspace).is_err());
}

/// Discovery admission and execution read the same on-disk target, in both edit directions.
#[test]
fn shared_target_discovery_uses_the_current_launch_snapshot() {
    for cached_provided in [true, false] {
        let project = tempfile::tempdir().unwrap();
        let local = tempfile::tempdir().unwrap();
        let workspace = project.path().display().to_string();
        let mut controls = RunControls::load_with_project(
            &workspace,
            Some(local.path().into()),
            Some(project.path().into()),
        );
        let provided = RunTarget::Provided {
            provider: "missing-target-source".into(),
            binding: "{}".into(),
            label: "Old artifact".into(),
            args: vec![],
        };
        let mut config = local_config("shared");
        config.local = false;
        if cached_provided {
            config.target = provided.clone();
            config.from_target = Some("old-target".into());
        }
        config.build.push(editor_core::RunStep {
            name: "Build".into(),
            target: editor_core::StepTarget::Action {
                target: config.target.clone(),
            },
        });
        controls.upsert(config, &workspace).unwrap();
        controls
            .accept_target_catalog(crate::extensions::TargetCatalog {
                failed: [(
                    "missing-target-source".into(),
                    "SOURCE_DISCOVERY_FAILED".into(),
                )]
                .into(),
                ..Default::default()
            })
            .unwrap();
        let mut file = editor_core::load_shared(project.path()).unwrap();
        file.configurations[0].target = if cached_provided {
            RunTarget::Program {
                program: "new-tool.exe".into(),
                args: vec!["literal value".into()],
            }
        } else {
            provided
        };
        file.configurations[0].build = vec![editor_core::RunStep {
            name: "Build edited target".into(),
            target: editor_core::StepTarget::Action {
                target: file.configurations[0].target.clone(),
            },
        }];
        editor_core::save_shared(project.path(), &file).unwrap();
        // Do not reload controls: both directions reproduce a real external file edit in an open window.
        if cached_provided {
            assert!(
                controls.launch_blocker("shared").is_none(),
                "an unrelated stale source must not reject a manual replacement"
            );
            let plan = controls.launch_plan("shared", &workspace).unwrap();
            assert!(
                plan.steps.iter().all(
                    |step| step.preparation.is_none() && step.request.program == "new-tool.exe"
                )
            );
            assert_eq!(plan.steps.last().unwrap().request.args, ["literal value"]);
            assert_eq!(
                controls
                    .prepare_build("shared", &workspace, 16)
                    .unwrap()
                    .steps[0]
                    .request
                    .program,
                "new-tool.exe"
            );
        } else {
            assert!(
                controls
                    .launch_blocker("shared")
                    .unwrap()
                    .contains("SOURCE_DISCOVERY_FAILED")
            );
            assert!(
                controls
                    .launch_plan("shared", &workspace)
                    .unwrap_err()
                    .contains("SOURCE_DISCOVERY_FAILED")
            );
            assert!(
                controls
                    .prepare_build("shared", &workspace, 16)
                    .unwrap_err()
                    .contains("SOURCE_DISCOVERY_FAILED")
            );
        }
    }
}

/// Referencing a build cannot bypass the discovery, matching-action or empty-build checks.
#[test]
fn referenced_builds_obey_the_same_admission_as_direct_builds() {
    for failure in ["discovery", "missing", "matching", "empty"] {
        let mut controls = RunControls::default();
        let mut build = local_config("build");
        build.name = "Library".into();
        if failure != "empty" {
            build.target = RunTarget::Provided {
                provider: "unavailable-source".into(),
                binding: "{}".into(),
                label: "Library".into(),
                args: vec![],
            };
            build.build.push(editor_core::RunStep {
                name: "Build library".into(),
                target: editor_core::StepTarget::Action {
                    target: if failure == "matching" {
                        local_config("manual").target
                    } else {
                        build.target.clone()
                    },
                },
            });
        }
        controls.upsert(build, "C:/work").unwrap();
        if failure == "discovery" || failure == "missing" {
            controls
                .accept_target_catalog(crate::extensions::TargetCatalog {
                    failed: if failure == "discovery" {
                        [("unavailable-source".into(), "DISCOVERY_REFUSED".into())].into()
                    } else {
                        Default::default()
                    },
                    ..Default::default()
                })
                .unwrap();
        }
        let mut launch = local_config("launch");
        launch.prelaunch.push(editor_core::RunStep {
            name: "Prepare library".into(),
            target: editor_core::StepTarget::Build {
                config: "Library".into(),
            },
        });
        controls.upsert(launch, "C:/work").unwrap();
        let direct = controls.prepare_build("build", "C:/work", 16).unwrap_err();
        let referenced = controls.launch_plan("launch", "C:/work").unwrap_err();
        assert_eq!(
            referenced, direct,
            "referenced build must retain its direct admission cause: {failure}"
        );
    }
}

/// A confirmed target repair uses new shared arguments and never revives a removed shared entry.
#[test]
fn target_repairs_preserve_external_edits_and_respect_deleted_shared_entries() {
    for explicit_candidate in [true, false] {
        let project = tempfile::tempdir().unwrap();
        let local = tempfile::tempdir().unwrap();
        let workspace = project.path().display().to_string();
        let mut controls = RunControls::load_with_project(
            &workspace,
            Some(local.path().into()),
            Some(project.path().into()),
        );
        let old = plugin_schema::DiscoveredTarget {
            id: "tool".into(),
            provider: "tools".into(),
            target_type: "native-tool".into(),
            label: "Tool".into(),
            program: "old.exe".into(),
            found_in: "tool.json".into(),
            fields: Default::default(),
        };
        let mut config = editor_core::configuration_for(&old, "shared".into(), "Tool".into());
        config.local = false;
        controls.upsert(config, &workspace).unwrap();
        let mut file = editor_core::load_shared(project.path()).unwrap();
        file.configurations[0].target = RunTarget::Program {
            program: "old.exe".into(),
            args: vec!["external literal | 中文".into()],
        };
        editor_core::save_shared(project.path(), &file).unwrap();
        let mut candidate = old;
        candidate.program = "repaired.exe".into();
        controls.reconcile_discovered(&[candidate]);
        let repair = |controls: &mut RunControls| {
            if explicit_candidate {
                controls.repair_target_with("shared", "tool", &workspace)
            } else {
                controls.repair_target("shared", &workspace)
            }
        };
        repair(&mut controls).unwrap();
        let stored = editor_core::load_shared(project.path()).unwrap();
        assert_eq!(stored.configurations[0].target.executable(), "repaired.exe");
        assert_eq!(
            stored.configurations[0].target.arguments(),
            ["external literal | 中文"]
        );
        editor_core::save_shared(project.path(), &editor_core::SharedSet::default()).unwrap();
        assert!(
            repair(&mut controls).is_err(),
            "repair cannot revive a deleted shared entry"
        );
        assert!(
            editor_core::load_shared(project.path())
                .unwrap()
                .configurations
                .is_empty()
        );
    }
}

/// A confirmed discovery performs its declared build once and its final program once.
#[test]
fn a_discovered_build_is_executable_preparation_without_a_self_reference() {
    let mut controls = RunControls::default();
    controls.reconcile_discovered(&[plugin_schema::DiscoveredTarget {
        id: "target".into(),
        provider: "provider".into(),
        target_type: "binary".into(),
        label: "candidate".into(),
        program: "cargo.exe".into(),
        fields: [
            ("program_args".into(), "run".into()),
            ("build_program".into(), "cargo.exe".into()),
            ("build_args".into(), "build".into()),
        ]
        .into(),
        found_in: "project.json".into(),
    }]);
    let id = controls.confirm_target("target", "C:/work").unwrap();
    let plan = controls.launch_plan(&id, "C:/work").unwrap();
    assert_eq!(plan.steps.len(), 2);
    assert_eq!(plan.steps[0].request.args, ["build"]);
    assert_eq!(plan.steps[1].request.args, ["run"]);
}

/// A shared definition governs future launches, while an active session keeps its launch snapshot.
#[test]
fn shared_file_changes_do_not_prevent_locating_an_active_session() {
    let project = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();
    let workspace = project.path().display().to_string();
    let mut controls = RunControls::load_with_project(
        &workspace,
        Some(local.path().into()),
        Some(project.path().into()),
    );
    let mut shared = local_config("shared");
    shared.local = false;
    controls.upsert(shared, &workspace).unwrap();
    controls.sessions.insert(
        7,
        RunSession {
            id: 7,
            config: "shared".into(),
            plugin: "provider".into(),
            state: plugin_runtime::ExecutionState::Running,
            provider_session: Some("original program".into()),
            failure: None,
        },
    );
    let path = editor_core::project_path(project.path());
    std::fs::write(&path, b"{broken").unwrap();
    assert_eq!(
        controls.plan_launch("shared", &workspace),
        LaunchPlan::Existing { session: 7 }
    );
    assert_eq!(controls.launch_blocker("shared"), None);
    std::fs::remove_file(path).unwrap();
    assert_eq!(
        controls.plan_launch("shared", &workspace),
        LaunchPlan::Existing { session: 7 }
    );
    assert_eq!(controls.launch_blocker("shared"), None);
}

/// Editing another Shell field must preserve empty/multiline argv and append the script only at launch.
#[test]
fn shell_name_edit_preserves_literal_argument_boundaries() {
    let mut config = local_config("shell");
    config.target = RunTarget::Script {
        interpreter: "powershell.exe".into(),
        args: vec!["-c".into(), "".into(), "line1\nline2".into()],
        script: "echo ok".into(),
    };
    let mut draft = RunConfigDraft::from_config(Some(&config), config.id.clone());
    draft.name = "renamed shell".into();
    let saved = draft.to_config().unwrap();
    assert_eq!(saved.target, config.target);
    assert_eq!(
        saved.literal_arguments(),
        ["-c", "", "line1\nline2", "echo ok"]
    );
}

/// A terminal snapshot can precede its exit-code reply; preparation must wait for that reply.
#[test]
fn an_exited_preparation_session_waits_for_its_actual_exit_code() {
    for code in [Some(0), Some(3), None] {
        let mut controls = RunControls::default();
        let mut config = local_config("run");
        config.build.push(editor_core::RunStep {
            name: "build".into(),
            target: editor_core::StepTarget::Action {
                target: RunTarget::Program {
                    program: "tool.exe".into(),
                    args: vec!["build".into()],
                },
            },
        });
        controls.upsert(config, "C:/work").unwrap();
        let plan = controls
            .prepare_launch("run", "C:/work", MAX_PREPARED_STEPS)
            .unwrap();
        let request = controls.begin("run");
        controls.begin_sequence("run", plan, request);
        controls.sequence_started("run", 0, 7, Some("provider-session".into()));
        controls.sessions.insert(
            7,
            RunSession {
                id: 7,
                config: "run".into(),
                plugin: "provider".into(),
                state: plugin_runtime::ExecutionState::Exited,
                provider_session: Some("provider-session".into()),
                failure: None,
            },
        );
        let next = |controls: &RunControls| {
            controls.preparation("run").unwrap().next_action(
                |session| controls.session_known(session),
                |session| controls.session_can_report_result(session),
            )
        };
        // Neither an early snapshot nor an absent exit code is evidence of a successful build.
        assert_eq!(next(&controls), SequenceAction::Wait);
        let poll = controls.begin_poll("run", 0, 7);
        controls.reconcile_run_status(&[(
            "run".into(),
            poll,
            crate::extensions::RunStatus::Ended { code },
        )]);
        if code == Some(0) {
            assert_eq!(next(&controls), SequenceAction::Start { index: 1 });
        } else {
            assert!(matches!(next(&controls), SequenceAction::Blocked { .. }));
        }
    }
}
/// A refused native launch reports its actual tool/permission error rather than a generic missing result.
#[test]
fn preparation_keeps_the_actual_native_start_failure() {
    let mut controls = RunControls::default();
    controls.upsert(local_config("run"), "C:/work").unwrap();
    let plan = controls.launch_plan("run", "C:/work").unwrap();
    let request = controls.begin("run");
    controls.begin_sequence("run", plan, request);
    controls.sequence_started("run", 0, 7, None);
    controls.sessions.insert(
        7,
        RunSession {
            id: 7,
            config: "run".into(),
            plugin: "provider".into(),
            state: plugin_runtime::ExecutionState::Failed,
            provider_session: None,
            failure: Some("Native tool not found: missing.exe".into()),
        },
    );
    let Some(SequenceAction::Blocked { reason }) = controls.preparation_action("run") else {
        panic!("failed launch must block");
    };
    assert!(reason.contains("missing.exe"));
    assert!(!reason.contains("未报告结果"));
}
