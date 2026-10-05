//! Replays plugin scope changes and delayed startup results without external processes.

use super::*;
use crate::app::language_servers::ServiceLoadState;
use gpui_kit::{TestAppContext, component::Root, gpui, px, size};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

/// Recognition and parsers require installed declarations, even for extensions GPUI knows internally.
#[gpui::test]
fn documents_without_installed_providers_stay_plain(cx: &mut TestAppContext) {
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
    for name in [
        "main.rs",
        "Cargo.toml",
        "index.html",
        "main.jsx",
        "style.css",
        "data.json",
        "view.vue",
        "readme.md",
    ] {
        let path = directory.path().join(name);
        std::fs::write(&path, "sample").unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_file(path.clone(), window, cx)));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = app.read(cx);
            assert_eq!(
                app.editor.read(cx).language_name().as_ref(),
                "text",
                "{name}"
            );
            assert!(app.editor.read(cx).lsp().definition_provider.is_none());
            assert!(app.language_servers.is_empty());
        });
    }
}

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

/// A legacy installation retains preferences and files without advertising that its old component is starting.
#[gpui::test]
fn incompatible_installed_plugin_preserves_data_without_starting(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join(".runtime-plugin-test");
    std::fs::create_dir_all(&root).unwrap();
    let mut manifest = serde_json::from_str::<plugin_runtime::plugin_protocol::Manifest>(
        include_str!("../../../../../plugins/terminal/manifest.json"),
    )
    .unwrap();
    // Replay a real pre-rename installation through editor startup, not a migration-private API.
    manifest.id = "me.terminal".into();
    // The fixture describes a stored protocol-five installation, independent of today's bundle.
    manifest.protocol = 5;
    let old_data = root.join("data/me.terminal");
    std::fs::create_dir_all(&old_data).unwrap();
    std::fs::write(old_data.join("settings.json"), "preserved settings").unwrap();
    std::fs::write(old_data.join("state-project.json"), "preserved snapshot").unwrap();
    let installed = plugin_runtime::Installed {
        manifest: manifest.clone(),
        digest: "fixture".into(),
        grants: manifest.permissions.clone(),
        enabled: true,
        project_enabled: std::collections::BTreeSet::from(["retained-project".into()]),
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
        cx.debug_bounds("plugin-loading-indicator").is_none(),
        "an incompatible runtime plugin must not claim to start"
    );
    let migrated = plugin_runtime::Manager::read_registry(&root).unwrap();
    assert!(!migrated.contains_key("me.terminal"));
    assert_eq!(migrated["terminal"].manifest.id, "terminal");
    assert!(migrated["terminal"].enabled);
    assert_eq!(
        migrated["terminal"].grants,
        migrated["terminal"].manifest.permissions
    );
    assert!(
        migrated["terminal"]
            .project_enabled
            .contains("retained-project")
    );
    assert_eq!(
        std::fs::read_to_string(root.join("data/terminal/settings.json")).unwrap(),
        "preserved settings"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("data/terminal/state-project.json")).unwrap(),
        "preserved snapshot"
    );
    // Originals remain recoverable, and a second startup must not overwrite newer canonical data.
    assert!(old_data.join("settings.json").exists());
    std::fs::write(root.join("data/terminal/settings.json"), "new settings").unwrap();
    plugin_runtime::Manager::read_registry(&root).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("data/terminal/settings.json")).unwrap(),
        "new settings"
    );
}

/// No language reports loading until an installed package declares it.
#[test]
fn initial_state_requires_installed_plugins() {
    let directory = tempfile::tempdir().unwrap();
    extensions::contributions::refresh(directory.path()).unwrap();
    assert!(crate::language::providers::languages().is_empty());
}

/// Installed package enablement is the source of truth for language availability.
#[test]
fn missing_plugin_does_not_start_its_language_server() {
    let directory = tempfile::tempdir().unwrap();
    extensions::contributions::refresh(directory.path()).unwrap();
    assert!(crate::language::providers::languages().is_empty());
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
            // An unfamiliar identity exercises the same callback lifetime guard without starting native code.
            let (_manager, plan) = crate::tests::declared_language_service(directory.path());
            let old_server =
                Arc::new(language_navigation::LanguageServer::from_service(plan.clone()).unwrap());
            app.language_servers
                .insert("rust".into(), old_server.clone());
            // Global disable removes this instance; a pending startup still owns its Arc.
            app.language_servers.remove("rust");
            let new_server =
                Arc::new(language_navigation::LanguageServer::from_service(plan).unwrap());
            app.language_servers
                .insert("rust".into(), new_server.clone());
            app.language_service_states
                .insert("rust".into(), ServiceLoadState::Loading);
            // Replay the retired startup's timeout after the project override has activated.
            app.finish_server_loading(
                "rust",
                &old_server,
                Err(anyhow::anyhow!("language server readiness timed out")),
                cx,
            );
            assert_eq!(
                app.language_service_states["rust"],
                ServiceLoadState::Loading
            );
            app.finish_server_loading("rust", &old_server, Ok(()), cx);
            assert_eq!(
                app.language_service_states["rust"],
                ServiceLoadState::Loading
            );
            app.finish_server_loading("rust", &new_server, Ok(()), cx);
            assert_eq!(app.language_service_states["rust"], ServiceLoadState::Ready);
            app.finish_server_loading(
                "rust",
                &old_server,
                Err(anyhow::anyhow!("late timeout")),
                cx,
            );
            assert_eq!(app.language_service_states["rust"], ServiceLoadState::Ready);
            app.finish_server_loading(
                "rust",
                &new_server,
                Err(anyhow::anyhow!("current server failure")),
                cx,
            );
            assert_eq!(
                app.language_service_states["rust"],
                ServiceLoadState::Failed("current server failure".into())
            );
            app.language_servers.remove("rust");
            app.language_service_states.remove("rust");
            app.finish_server_loading("rust", &new_server, Ok(()), cx);
            assert!(!app.language_service_states.contains_key("rust"));
        });
    });
}

/// Builds a declarative registry with Rust enabled exclusively for this workspace.
fn project_rust_registry(workspace: &Workspace) -> PathBuf {
    let root = workspace.root().join(".runtime-plugin-test");
    let digest = "a".repeat(64);
    let package = root.join("packages/rust").join(&digest);
    std::fs::create_dir_all(&package).unwrap();
    // This fixture exercises declaration and scope resolution; grammar is already validated below.
    std::fs::write(
        package.join("plugin.toml"),
        include_str!("../../../../../plugins/rust/plugin.toml")
            .replace("file_icons = \"icons.json\"", ""),
    )
    .unwrap();
    let installed = plugin_runtime::Installed {
        manifest: crate::extensions::test_manifest(include_str!(
            "../../../../../plugins/rust/manifest.json"
        )),
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
        serde_json::to_vec(&BTreeMap::from([("rust", installed)])).unwrap(),
    )
    .unwrap();
    root
}

/// Trust can be revoked while declarations are restored but the worker has not published entries yet.
#[gpui::test]
fn restricting_startup_withdraws_declarations_before_worker_publication(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    project_rust_registry(&workspace);
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            assert!(crate::language::providers::language_for_path(Path::new("main.rs")).is_some());
            assert!(app.extensions.read(cx).entries.is_empty());
            app.set_workspace_trusted(false, cx);
            app.sync_runtime_contributions(window, cx);
            assert!(crate::language::providers::language_for_path(Path::new("main.rs")).is_none());
            assert!(app.dynamic_languages.entries.is_empty());
            assert!(app.language_servers.is_empty());
            assert!(!SessionState::load(app.workspace.root()).workspace_trusted);
        })
    });
}

/// Project-only language recognition survives repeated publication and disappears when its exception is removed.
#[gpui::test]
fn project_only_language_survives_registry_refresh(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let root = project_rust_registry(&workspace);
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            for _ in 0..2 {
                extensions::contributions::refresh_for_workspace(&root, app.workspace.root())
                    .unwrap();
                app.sync_runtime_contributions(window, cx);
                assert_eq!(
                    crate::language::providers::language_for_path(Path::new("main.rs")),
                    Some("rust".into())
                );
            }
            let mut registry = plugin_runtime::Manager::read_registry(&root).unwrap();
            registry.get_mut("rust").unwrap().project_enabled.clear();
            std::fs::write(
                root.join("registry.json"),
                serde_json::to_vec(&registry).unwrap(),
            )
            .unwrap();
            extensions::contributions::refresh_for_workspace(&root, app.workspace.root()).unwrap();
            app.sync_runtime_contributions(window, cx);
            assert!(crate::language::providers::language_for_path(Path::new("main.rs")).is_none());
            assert!(app.dynamic_languages.entries.is_empty());
        })
    });
}

/// Removing a provider resets its open aliases without altering unrelated loaded editor state.
#[gpui::test]
fn removed_html_plugin_resets_aliases_and_preserves_unrelated_state(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    for name in ["index.html", "legacy.HTM", "style.css"] {
        std::fs::write(directory.path().join(name), "").unwrap();
    }
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    cx.run_until_parked();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            extensions::contributions::refresh(directory.path()).unwrap();
            for name in ["index.html", "legacy.HTM", "style.css"] {
                app.open_file(directory.path().join(name), window, cx);
            }
        });
    });
    // Opening starts in plain text and schedules highlighting after the first frame.
    cx.run_until_parked();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            assert_eq!(app.tabs.len(), 3);
            // HTML is plain text before installation; simulate the loaded grammar on both aliases.
            for tab in &app.tabs[..2] {
                assert_eq!(
                    tab.text
                        .as_ref()
                        .unwrap()
                        .editor
                        .read(cx)
                        .language_name()
                        .as_ref(),
                    "text"
                );
                tab.text
                    .as_ref()
                    .unwrap()
                    .editor
                    .update(cx, |editor, cx| editor.set_highlighter("html", cx));
            }
            // Set the unrelated tab's loaded state without relying on frame callbacks.
            app.tabs[2]
                .text
                .as_ref()
                .unwrap()
                .editor
                .update(cx, |editor, cx| editor.set_highlighter("css", cx));
            app.dynamic_language_ids.insert("html".into());
            // The registry no longer includes HTML after disable/uninstall.
            app.sync_runtime_contributions(window, cx);
            assert!(app.dynamic_languages.entries.is_empty());
            for tab in &app.tabs[..2] {
                assert_eq!(
                    tab.text
                        .as_ref()
                        .unwrap()
                        .editor
                        .read(cx)
                        .language_name()
                        .as_ref(),
                    "text"
                );
            }
            assert_eq!(
                app.tabs[2]
                    .text
                    .as_ref()
                    .unwrap()
                    .editor
                    .read(cx)
                    .language_name()
                    .as_ref(),
                "css"
            );
        });
    });
}

/// Normal loading and unconfirmed errors expose clickable icons and anchored plugin lists.
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
            // Readiness supplies loading state; startup failures separately emit an owned runtime log.
            // Keep both seams in this fixture so a persistent Failed state cannot recreate confirmed alerts.
            app.language_service_states.insert(
                "custom-language".into(),
                ServiceLoadState::Failed("missing service".into()),
            );
            app.extensions.read(cx).runtime_logs().append(
                "custom-plugin",
                plugin_runtime::LogLevel::Error,
                "language.status:custom-plugin/analysis",
                "missing service",
            );
            cx.notify();
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
            app.language_service_states
                .insert("custom-language".into(), ServiceLoadState::Loading);
            cx.notify();
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
