//! Regression checks for the public session gateway's authority, scope and capacity.
use super::*;

/// An execution gateway must require the same authority as the provider it delegates to.
#[test]
fn starting_a_session_requires_execution_and_panel_authority() {
    let provider = session_provider("workspace", Arc::new(AtomicBool::new(true)));
    let permissions = &provider.contracts[SESSION_CONTRACT].methods["start"].permissions;
    assert!(permissions.contains("process.exec"));
    assert!(permissions.contains("ui.panels"));
}

/// A completed start acknowledgement still owns a running process and must remain addressable.
#[test]
fn a_full_session_table_never_discards_running_executions() {
    let alive = Arc::new(AtomicBool::new(true));
    let mut sessions = HostSessions::new(alive.clone());
    let provider = session_provider("workspace", alive);
    for index in 0..=MAX_HOST_EXECUTIONS {
        let completion = Completion::new(5_000);
        completion.finish(Ok(
            serde_json::json!({"session": index.to_string(), "state": "started"}),
        ));
        let result = sessions.insert(
            provider.clone(),
            RunRequest {
                program: "program.exe".into(),
                args: vec![index.to_string()],
                cwd: None,
                name: None,
                env: vec![],
            },
            index.to_string(),
            completion,
            context_for(&provider.caller, &provider.alive),
        );
        if index < MAX_HOST_EXECUTIONS {
            assert!(result.is_ok());
        } else {
            assert_eq!(result.unwrap_err().code, ErrorCode::LimitExceeded);
        }
    }
    assert!(
        sessions.get(1).is_some(),
        "the oldest running process must still be stoppable"
    );
    assert!(sessions.entries.len() <= MAX_HOST_EXECUTIONS);
}

/// Switching a runtime's selected workspace must not expose another workspace's sessions.
#[test]
fn switching_workspaces_does_not_publish_the_previous_session_table() {
    let root = tempfile::tempdir().unwrap();
    let next = root.path().join("next");
    std::fs::create_dir(&next).unwrap();
    let mut manager = Manager::open(
        root.path().join("plugins"),
        plugin_protocol::Environment {
            workspace: root.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let provider = session_provider(&manager.host_scope(), manager.host_alive.clone());
    let origin = context_for(&provider.caller, &provider.alive);
    manager
        .host_sessions
        .insert(
            provider,
            RunRequest {
                program: "program.exe".into(),
                args: vec![],
                cwd: None,
                name: None,
                env: vec![],
            },
            "first-workspace".into(),
            Completion::new(5_000),
            origin,
        )
        .unwrap();
    manager
        .switch_workspace(
            plugin_protocol::Environment {
                workspace: next.display().to_string(),
                ..Default::default()
            },
            true,
        )
        .unwrap();
    let scope = manager.host_scope();
    let context = context_for(&host_caller(&scope), &manager.host_alive);
    let answer = session_answer(&mut manager, &context, "list", &serde_json::json!({})).unwrap();
    assert_eq!(answer["sessions"].as_array().unwrap().len(), 0);
}

/// Build an ordinary delegation source; no resource or reference internals are opened for tests.
fn context_for(caller: &Caller, alive: &Arc<AtomicBool>) -> CallContext {
    CallContext {
        native_waits: Vec::new(),
        menu: None,
        origin: crate::plugin_services::InvocationOrigin::Delegated,
        caller: caller.clone(),
        permissions: caller.permissions.clone(),
        lifetimes: vec![alive.clone()],
        ancestry: vec![],
    }
}

/// Revoked consumers cannot regain authority merely by reaching the host's service dispatcher.
#[test]
fn an_unprivileged_or_retired_source_cannot_start_a_session() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        root.path().join("plugins"),
        plugin_protocol::Environment {
            workspace: root.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let mut context = context_for(&host_caller(&manager.host_scope()), &manager.host_alive);
    context.caller.plugin = "consumer".into();
    context.caller.instance = "consumer@1".into();
    context.permissions.clear();
    let request = serde_json::json!({"program":"program.exe","args":[]});
    let error = session_answer(&mut manager, &context, "start", &request).unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    let source = Arc::new(AtomicBool::new(false));
    context.lifetimes.push(source);
    let error = session_answer(&mut manager, &context, "start", &request).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidHandle);
    assert!(manager.executions().is_empty());
}

/// Session identities confer no access to another consumer, even inside the same workspace.
#[test]
fn a_consumer_cannot_list_query_or_stop_another_consumers_session() {
    let root = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        root.path().join("plugins"),
        plugin_protocol::Environment {
            workspace: root.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let provider = session_provider(&manager.host_scope(), manager.host_alive.clone());
    let mut owner = context_for(&provider.caller, &provider.alive);
    owner.caller.instance = "first@1".into();
    owner.caller.plugin = "first".into();
    let session = manager
        .host_sessions
        .insert(
            provider,
            RunRequest {
                program: "program.exe".into(),
                args: vec![],
                cwd: None,
                name: None,
                env: vec![],
            },
            "one".into(),
            Completion::new(5_000),
            owner.clone(),
        )
        .unwrap();
    let mut stranger = owner;
    stranger.caller.instance = "second@1".into();
    stranger.caller.plugin = "second".into();
    let listed = session_answer(&mut manager, &stranger, "list", &serde_json::json!({})).unwrap();
    assert!(listed["sessions"].as_array().unwrap().is_empty());
    for method in ["status", "stop"] {
        let error = session_answer(
            &mut manager,
            &stranger,
            method,
            &serde_json::json!({"session": session.id().to_string()}),
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidHandle);
    }
}

/// Confirmed exits free space; unconfirmed starts remain live and cannot be silently evicted.
#[test]
fn finished_executions_release_capacity_for_later_programs() {
    let alive = Arc::new(AtomicBool::new(true));
    let provider = session_provider("workspace", alive.clone());
    let origin = context_for(&provider.caller, &provider.alive);
    let mut sessions = HostSessions::new(alive);
    for index in 0..(MAX_HOST_EXECUTIONS * 2) {
        let completion = Completion::new(5_000);
        completion.finish(Ok(
            serde_json::json!({"session": index.to_string(), "state":"started"}),
        ));
        let entry = sessions
            .insert(
                provider.clone(),
                RunRequest {
                    program: "program.exe".into(),
                    args: vec![index.to_string()],
                    cwd: None,
                    name: None,
                    env: vec![],
                },
                index.to_string(),
                completion,
                origin.clone(),
            )
            .unwrap();
        // Production flips this flag only after a validated pinned-provider status response.
        entry.ended.store(true, Ordering::Release);
        assert_eq!(entry.snapshot().state, ExecutionState::Exited);
        assert!(!entry.stoppable());
    }
    assert_eq!(sessions.entries.len(), MAX_HOST_EXECUTIONS);
}
