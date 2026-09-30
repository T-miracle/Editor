//! Replays plugin scope changes and delayed startup results without external processes.

use super::*;
use gpui_kit::{TestAppContext, component::Root, gpui, px, size};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

#[gpui::test]
fn status_popup_blocks_title_bar_until_dismissed(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, window_cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    window_cx.simulate_resize(size(px(800.), px(600.)));
    window_cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.plugin_popup = Some((PluginPopupKind::Error, point(px(90.), px(500.))));
            cx.notify();
        });
        window.draw(cx).clear(cx);
    });
    assert!(window_cx.debug_bounds("plugin-popup-blocker").is_some());

    // Dismissing the popup must consume the press before the title bar handles it.
    let title = window_cx.debug_bounds("title-bar-drag-region").unwrap();
    window_cx.simulate_mouse_down(title.center(), MouseButton::Left, Default::default());
    window_cx.run_until_parked();
    assert!(window_cx.update(|_, cx| app.read(cx).plugin_popup.is_none()));
    assert!(!window_cx.update(|_, cx| app.read(cx).titlebar_should_move));
    // Once dismissed, the underlying panel should accept a new press normally.
    window_cx.simulate_mouse_up(title.center(), MouseButton::Left, Default::default());
    window_cx.update(|window, cx| window.draw(cx).clear(cx));
    window_cx.simulate_mouse_down(title.center(), MouseButton::Left, Default::default());
    window_cx.run_until_parked();
    assert!(window_cx.update(|_, cx| app.read(cx).titlebar_should_move));
}

/// A previously installed runtime plugin must appear in the startup indicator.
#[gpui::test]
fn installed_runtime_plugin_is_visible_while_starting(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join(".runtime-plugin-test");
    std::fs::create_dir_all(&root).unwrap();
    let manifest = serde_json::from_str::<plugin_runtime::plugin_protocol::Manifest>(include_str!(
        "../../../../../plugins/terminal/manifest.json"
    ))
    .unwrap();
    let installed = plugin_runtime::Installed {
        manifest: manifest.clone(),
        digest: "fixture".into(),
        grants: manifest.permissions.clone(),
        enabled: true,
        project_enabled: Default::default(),
        global_enabled: None,
        error: None,
    };
    std::fs::write(
        root.join("registry.json"),
        serde_json::to_vec(&BTreeMap::from([(manifest.id, installed)])).unwrap(),
    )
    .unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        Root::new(
            cx.new(|cx| EditorApp::new(workspace, None, window, cx)),
            window,
            cx,
        )
    });
    cx.simulate_resize(size(px(1000.), px(800.)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("plugin-loading-indicator").is_some(),
        "an enabled runtime plugin must show startup loading in the status bar"
    );
}

/// No language reports loading until an installed package declares it.
#[test]
fn initial_state_requires_installed_plugins() {
    let directory = tempfile::tempdir().unwrap();
    extensions::contributions::refresh(directory.path()).unwrap();
    assert!(PluginLoadEntry::initial().is_empty());
}

/// Installed package enablement is the source of truth for language availability.
#[test]
fn missing_plugin_does_not_start_its_language_server() {
    let directory = tempfile::tempdir().unwrap();
    extensions::contributions::refresh(directory.path()).unwrap();
    assert!(PluginLoadEntry::initial().is_empty());
}

/// A retired startup cannot report its timeout against a project-enabled replacement.
#[gpui::test]
fn stale_rust_timeout_after_project_reenable(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let view_slot = Rc::new(RefCell::new(None));
    let capture = view_slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(view.clone());
        Root::new(view, window, cx)
    });
    let view = view_slot.borrow_mut().take().unwrap();
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |app, cx| {
            let manifest = plugin_schema::PluginManifest::parse(include_str!(
                "../../../../../plugins/rust/plugin.toml"
            ))
            .unwrap();
            let language = manifest.languages[0].clone();
            let old_server = Arc::new(
                language_navigation::LanguageServer::new(directory.path(), language.clone())
                    .unwrap(),
            );
            app.language_servers
                .insert("rust".into(), old_server.clone());
            // Global disable removes this instance; a pending startup still owns its Arc.
            app.language_servers.remove("rust");
            let new_server = Arc::new(
                language_navigation::LanguageServer::new(directory.path(), language).unwrap(),
            );
            app.language_servers
                .insert("rust".into(), new_server.clone());
            app.plugin_loads = vec![PluginLoadEntry {
                plugin: language_plugins::BundledPlugin::Rust,
                package_root: PathBuf::new(),
                state: PluginLoadState::Loading,
                grammar_loaded: true,
                server_loading: true,
            }];
            // Replay the retired startup's timeout after the project override has activated.
            app.finish_server_loading(
                "rust",
                &old_server,
                Err(anyhow::anyhow!("language server readiness timed out")),
                cx,
            );
            assert_eq!(app.plugin_loads[0].state, PluginLoadState::Loading);
            assert!(app.plugin_loads[0].server_loading);
            app.finish_server_loading("rust", &old_server, Ok(()), cx);
            assert_eq!(app.plugin_loads[0].state, PluginLoadState::Loading);
            assert!(app.plugin_loads[0].server_loading);
            app.finish_server_loading("rust", &new_server, Ok(()), cx);
            assert_eq!(app.plugin_loads[0].state, PluginLoadState::Enabled);
            assert!(!app.plugin_loads[0].server_loading);
            app.finish_server_loading(
                "rust",
                &old_server,
                Err(anyhow::anyhow!("late timeout")),
                cx,
            );
            assert_eq!(app.plugin_loads[0].state, PluginLoadState::Enabled);
            app.finish_server_loading(
                "rust",
                &new_server,
                Err(anyhow::anyhow!("current server failure")),
                cx,
            );
            assert_eq!(
                app.plugin_loads[0].state,
                PluginLoadState::Error("current server failure".into())
            );
            app.language_servers.remove("rust");
            app.plugin_loads[0].state = PluginLoadState::Disabled;
            app.finish_server_loading("rust", &new_server, Ok(()), cx);
            assert_eq!(app.plugin_loads[0].state, PluginLoadState::Disabled);
        });
    });
}

/// Builds a declarative registry with Rust enabled exclusively for this workspace.
fn project_rust_registry(workspace: &Workspace) -> PathBuf {
    let root = workspace.root().join(".runtime-plugin-test");
    let digest = "a".repeat(64);
    let package = root.join("packages/me.rust").join(&digest);
    std::fs::create_dir_all(&package).unwrap();
    // This fixture exercises declaration and scope resolution; grammar is already validated below.
    std::fs::write(
        package.join("plugin.toml"),
        include_str!("../../../../../plugins/rust/plugin.toml")
            .replace("file_icons = \"icons.json\"", ""),
    )
    .unwrap();
    let installed = plugin_runtime::Installed {
        manifest: serde_json::from_str(include_str!("../../../../../plugins/rust/manifest.json"))
            .unwrap(),
        digest,
        grants: Default::default(),
        enabled: false,
        project_enabled: [workspace.root().display().to_string()]
            .into_iter()
            .collect(),
        global_enabled: None,
        error: None,
    };
    std::fs::write(
        root.join("registry.json"),
        serde_json::to_vec(&BTreeMap::from([("me.rust", installed)])).unwrap(),
    )
    .unwrap();
    root
}

/// Restored project overrides retain the server when the worker publishes the same package.
#[gpui::test]
fn project_only_rust_startup_survives_registry_refresh(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let root = project_rust_registry(&workspace);
    let view_slot = Rc::new(RefCell::new(None));
    let capture = view_slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(view.clone());
        Root::new(view, window, cx)
    });
    let view = view_slot.borrow_mut().take().unwrap();
    cx.run_until_parked();
    cx.update(|window, cx| {
        view.update(cx, |app, cx| {
            extensions::contributions::refresh_for_workspace(&root, app.workspace.root()).unwrap();
            let language = language_plugins::language_for_path(Path::new("main.rs")).unwrap();
            let server = Arc::new(
                language_navigation::LanguageServer::new(app.workspace.root(), language).unwrap(),
            );
            app.language_servers.insert("rust".into(), server.clone());
            app.plugin_loads = PluginLoadEntry::initial();
            assert_eq!(
                app.plugin_loads.len(),
                1,
                "project override must expose Rust"
            );
            app.plugin_loads[0].grammar_loaded = true;
            app.begin_server_loading("rust", cx);
            // The worker republishes the same effective registry after startup restoration.
            app.sync_runtime_contributions(window, cx);
            assert!(
                app.language_servers
                    .get("rust")
                    .is_some_and(|active| Arc::ptr_eq(active, &server)),
                "unchanged project-only Rust must keep its in-flight startup"
            );
            assert!(app.plugin_loads[0].server_loading);
            app.finish_server_loading("rust", &server, Ok(()), cx);
            assert_eq!(app.plugin_loads[0].state, PluginLoadState::Enabled);
            app.sync_runtime_contributions(window, cx);
            assert_eq!(app.plugin_loads[0].state, PluginLoadState::Enabled);

            // A real package replacement must still invalidate the previous instance.
            let mut registry = plugin_runtime::Manager::read_registry(&root).unwrap();
            let replacement = root.join("packages/me.rust").join("b".repeat(64));
            std::fs::create_dir_all(&replacement).unwrap();
            std::fs::copy(
                app.plugin_loads[0].package_root.join("plugin.toml"),
                replacement.join("plugin.toml"),
            )
            .unwrap();
            registry.get_mut("me.rust").unwrap().digest = "b".repeat(64);
            std::fs::write(
                root.join("registry.json"),
                serde_json::to_vec(&registry).unwrap(),
            )
            .unwrap();
            extensions::contributions::refresh_for_workspace(&root, app.workspace.root()).unwrap();
            app.sync_runtime_contributions(window, cx);
            assert!(!app.language_servers.contains_key("rust"));
            assert!(!app.plugin_loads[0].grammar_loaded);
            assert_eq!(
                app.plugin_loads[0].package_root,
                replacement.canonicalize().unwrap()
            );
            app.finish_server_loading("rust", &server, Err(anyhow::anyhow!("retired timeout")), cx);
            assert_eq!(app.plugin_loads[0].state, PluginLoadState::Loading);

            // Removing the project exception must remove Rust despite the retained-state optimization.
            registry.get_mut("me.rust").unwrap().project_enabled.clear();
            std::fs::write(
                root.join("registry.json"),
                serde_json::to_vec(&registry).unwrap(),
            )
            .unwrap();
            extensions::contributions::refresh_for_workspace(&root, app.workspace.root()).unwrap();
            app.sync_runtime_contributions(window, cx);
            assert!(app.plugin_loads.is_empty());
            assert!(language_plugins::language_for_path(Path::new("main.rs")).is_none());
        })
    });
}

/// Both transient states expose clickable icons and anchored plugin lists.
#[gpui::test]
fn status_indicators_open_plugin_lists(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let view_slot = Rc::new(RefCell::new(None));
    let capture = view_slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(view.clone());
        Root::new(view, window, cx)
    });
    let view = view_slot.borrow_mut().take().unwrap();
    cx.run_until_parked();
    cx.simulate_resize(size(px(1000.), px(800.)));
    cx.update(|window, cx| {
        view.update(cx, |app, cx| {
            // The status UI also renders failures from packages loaded after startup.
            app.plugin_loads.push(PluginLoadEntry {
                plugin: language_plugins::BundledPlugin::Rust,
                package_root: PathBuf::new(),
                state: PluginLoadState::Loading,
                grammar_loaded: false,
                server_loading: false,
            });
            app.set_plugin_state(
                language_plugins::BundledPlugin::Rust,
                PluginLoadState::Error("missing grammar".to_owned()),
                cx,
            );
        });
        window.draw(cx).clear(cx);
    });
    let indicator = cx
        .debug_bounds("plugin-error-indicator")
        .expect("plugin failure should have a status indicator");
    cx.simulate_click(indicator.center(), Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let error_popup = cx
        .debug_bounds("plugin-status-popup")
        .expect("clicking the error indicator should paint a popup");
    assert!(error_popup.origin.y + error_popup.size.height < indicator.center().y);

    cx.update(|window, cx| {
        view.update(cx, |app, cx| {
            app.set_plugin_state(
                language_plugins::BundledPlugin::Rust,
                PluginLoadState::Loading,
                cx,
            );
        });
        window.draw(cx).clear(cx);
    });
    let loading = cx
        .debug_bounds("plugin-loading-indicator")
        .expect("loading plugin should have a spinning status indicator");
    // The first outside click only dismisses the previous popup; it cannot activate the indicator.
    cx.simulate_click(loading.center(), Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.update(|_, cx| view.read(cx).plugin_popup.is_none()));
    cx.simulate_click(loading.center(), Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let popup = cx
        .debug_bounds("plugin-status-popup")
        .expect("clicking the loading indicator should paint a popup");
    // A painted popup must have content and sit just above the clicked point.
    assert!(
        popup.size.height > px(20.),
        "popup has no visible content: {popup:?}"
    );
    let gap = loading.center().y - (popup.origin.y + popup.size.height);
    assert!(
        (px(4.)..=px(16.)).contains(&gap),
        "popup is not just above the loading click: popup={popup:?}, indicator={loading:?}"
    );
    assert!(popup.origin.x <= loading.center().x);
    assert!(loading.center().x <= popup.origin.x + popup.size.width);
}
