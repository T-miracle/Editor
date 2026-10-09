//! Document readers enter the production SDK/Manager/UI request path and observe native text.
use super::*;
use gpui_kit::{TestAppContext, VisualTestContext, gpui};
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, api, ui},
};
use serde_json::{Value as Json, json};

/// A fresh manager grants only this actual fixture's declared read and presentation permissions.
fn manager(workspace: &Path) -> (tempfile::TempDir, Manager) {
    let root = tempfile::tempdir().unwrap();
    let package = Package::read(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/community-api/history-preview-0.1.0.zip"),
    )
    .unwrap();
    let mut manager = Manager::open(
        root.path().into(),
        Environment {
            workspace: workspace.display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    (root, manager)
}

/// Invoke a guest command, then let the production worker publication route its owned request.
fn invoke(
    visual: &mut VisualTestContext,
    app: &Entity<EditorApp>,
    manager: &mut Manager,
    operation: Json,
) -> api::EditorValue {
    invoke_result(visual, app, manager, operation).expect("document operation must complete")
}

/// Preserve domain failures from either runtime admission or native revision checks.
fn invoke_result(
    visual: &mut VisualTestContext,
    app: &Entity<EditorApp>,
    manager: &mut Manager,
    operation: Json,
) -> Result<api::EditorValue, api::Failure> {
    invoke_for(visual, app, manager, "history-preview", operation)
}

/// Independent guests share this production publication adapter, without plugin-specific host code.
fn invoke_for(
    visual: &mut VisualTestContext,
    app: &Entity<EditorApp>,
    manager: &mut Manager,
    plugin: &str,
    operation: Json,
) -> Result<api::EditorValue, api::Failure> {
    manager
        .invoke_command(
            plugin,
            "probe",
            json!({"method":"editor", "operation":operation, "timeout_ms":30000}),
        )
        .unwrap();
    let scene = manager.live[plugin].views.values().next().unwrap();
    let ui::Kind::Text { text } = &scene.root.kind else {
        panic!("result expected")
    };
    let result: Result<api::Value, api::Failure> = serde_json::from_str(text).unwrap();
    let api::Value::Accepted(_) = result? else {
        panic!("request expected")
    };
    let request = manager
        .live
        .get_mut(plugin)
        .unwrap()
        .take_editor_requests()
        .pop()
        .unwrap();
    visual.update(|_, cx| {
        app.read(cx).extensions.clone().update(cx, |owner, cx| {
            let mut state = owner.worker.state.lock().unwrap();
            state.entries = manager.published_entries();
            state.editor_requests.push((plugin.into(), request.clone()));
            drop(state);
            owner.poll(cx);
        });
    });
    visual.run_until_parked();
    let api::RequestUpdate::Completed { result } = request.status() else {
        panic!("completion expected: {:?}", request.status())
    };
    manager.poll();
    result
}

/// Guest task completions advance the real consumer; no native test-only document API is used.
fn run_consumer(
    visual: &mut VisualTestContext,
    app: &Entity<EditorApp>,
    manager: &mut Manager,
    plugin: &str,
    command: &str,
) {
    manager
        .invoke_command(plugin, command, json!(null))
        .unwrap();
    for _ in 0..8 {
        let requests = manager.live.get_mut(plugin).unwrap().take_editor_requests();
        if requests.is_empty() {
            break;
        }
        visual.update(|_, cx| {
            app.read(cx).extensions.clone().update(cx, |owner, cx| {
                let mut state = owner.worker.state.lock().unwrap();
                state.entries = manager.published_entries();
                state.editor_requests.extend(
                    requests
                        .iter()
                        .cloned()
                        .map(|request| (plugin.into(), request)),
                );
                drop(state);
                owner.poll(cx);
            });
        });
        visual.run_until_parked();
        for request in requests {
            assert!(
                matches!(
                    request.status(),
                    api::RequestUpdate::Completed { result: Ok(_) }
                ),
                "consumer request failed: {:?}",
                request.status()
            );
        }
        manager.poll();
    }
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        manager
            .live
            .get_mut(plugin)
            .unwrap()
            .take_editor_requests()
            .is_empty(),
        "bounded consumer must finish"
    );
    let ui::Kind::Text { text } = &manager.live[plugin]
        .views
        .values()
        .next()
        .unwrap()
        .root
        .kind
    else {
        panic!("consumer completion expected")
    };
    assert!(
        text.contains("comparison is open") || text.contains("比较已打开"),
        "consumer must complete comparison: {text}"
    );
}

/// Forward the actual native ingress through the public manager, including close events after revocation.
fn deliver_native_events(
    visual: &mut VisualTestContext,
    app: &Entity<EditorApp>,
    manager: &mut Manager,
) {
    let events = visual.update(|_, cx| {
        app.read(cx)
            .extensions
            .read(cx)
            .worker
            .state
            .lock()
            .unwrap()
            .document_stream
            .take_batch(128)
            .unwrap()
    });
    for event in events {
        manager.document_event(event);
    }
    // Production subscriptions drain eight observations per tick; 16 ticks cover the ingress quota.
    for _ in 0..16 {
        manager.poll();
    }
}

mod comparison;
mod snapshots;
mod virtual_documents;
