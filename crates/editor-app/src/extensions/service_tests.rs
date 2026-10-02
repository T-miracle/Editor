//! Choose between real independent providers through the native settings UI and public runtime.
use super::*;
use gpui_kit::{TestAppContext, VisualTestContext, gpui};
#[path = "../../../plugin-runtime/tests/support/service_packages.rs"]
mod packages;

/// Host choice persists before a consumer opens a new provider reference; no guest ID is hardcoded in UI.
#[gpui::test]
#[ignore = "build capability-example through the public SDK first"]
fn native_service_provider_selection_routes_the_same_consumer_to_a_different_package(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let mut manager = plugin_runtime::Manager::open(
        directory.path().join("runtime"),
        protocol::Environment {
            workspace: workspace.root().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    for (id, provider) in [
        ("provider-a", true),
        ("provider-b", true),
        ("service-consumer", false),
    ] {
        let package = packages::package(id, provider, true, if provider { "1.0.0" } else { "^1" });
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
    }
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, editor_cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    let owner = editor_cx.update(|_, cx| app.read(cx).extensions.clone());
    editor_cx.update(|_, cx| {
        owner.update(cx, |owner, cx| {
            let mut state = owner.worker.state.lock().unwrap();
            state.entries = manager.published_entries();
            state.plugin_service_choices = manager.service_choices();
            drop(state);
            owner.poll(cx);
        })
    });
    editor_cx.run_until_parked();
    let editor_window = editor_cx.update(|window, _| window.window_handle());
    let trigger = editor_cx.debug_bounds("settings-trigger").unwrap();
    editor_cx.simulate_click(trigger.center(), Default::default());
    let dialog = editor_cx
        .update(|_, cx| cx.windows())
        .into_iter()
        .find(|handle| *handle != editor_window)
        .unwrap();
    let form = VisualTestContext::from_window(dialog, editor_cx).into_mut();
    form.run_until_parked();
    let nav = form.debug_bounds("settings-nav-plugins").unwrap();
    form.simulate_click(nav.center(), Default::default());
    form.simulate_resize(size(px(1100.), px(850.)));
    form.run_until_parked();
    form.update(|window, cx| window.draw(cx).clear(cx));
    let selector = form
        .debug_bounds("service-provider-workspace-example.echo")
        .unwrap();
    // Equal-width local segmented tabs: automatic, first provider, second provider.
    form.simulate_click(
        point(
            selector.left() + selector.size.width * 5. / 6.,
            selector.center().y,
        ),
        Default::default(),
    );
    form.run_until_parked();
    let work = form
        .update(|_, cx| {
            owner
                .read(cx)
                .worker
                .recorded
                .lock()
                .unwrap()
                .try_iter()
                .find_map(|work| match work {
                    Work::SetServiceProvider {
                        request,
                        owner,
                        scope,
                        contract,
                        provider,
                    } => Some((request, owner, scope, contract, provider)),
                    _ => None,
                })
        })
        .expect("native choice sends typed host work");
    assert_eq!(work.4.as_deref(), Some("provider-b"));
    manager
        .set_service_provider(work.1, work.2, &work.3, work.4.as_deref())
        .unwrap();
    form.update(|_, cx| {
        owner.update(cx, |owner, cx| {
            let mut state = owner.worker.state.lock().unwrap();
            state.plugin_service_choices = manager.service_choices();
            state.configuration_result = Some((work.0, Ok(())));
            state.configuration_revision += 1;
            drop(state);
            owner.poll(cx);
        })
    });
    form.run_until_parked();
    assert!(form.debug_bounds("plugin-settings-status").is_some());
    manager
        .invoke_command(
            "service-consumer",
            "service-open",
            serde_json::json!("example.echo"),
        )
        .unwrap();
    manager
        .invoke_command(
            "service-consumer",
            "service-call",
            serde_json::json!({"method":"echo","value":"from UI"}),
        )
        .unwrap();
    manager.poll();
    assert!(
        serde_json::to_string(
            manager.live["service-consumer"].scenes["welcome"]
                .ui
                .as_ref()
                .unwrap()
        )
        .unwrap()
        .contains("provider-b:from UI")
    );
}
