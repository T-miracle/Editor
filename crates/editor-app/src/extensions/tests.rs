//! Exercise dynamic native panels, declarative input commits and shortcut isolation.
use super::*;
use gpui_kit::component::WindowExt as _;
use gpui_kit::{EntityInputHandler, TestAppContext, VisualTestContext, gpui};

/// Installed theme definitions use the generic plugin environment API.
#[gpui::test]
fn editor_theme_api_publishes_external_terminal_overrides(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        let mut external = builtin_theme(true).clone();
        external.id = "external-dark".into();
        external.typography.ui.family = Some("External UI".into());
        external.typography.ui.size_px = Some(16.);
        external.typography.mono.family = Some("External Mono".into());
        external.plugins.clear();
        external.plugins.insert(
            "terminal".into(),
            plugin_schema::PluginTheme {
                colors: std::collections::BTreeMap::from([(
                    "ansi".into(),
                    plugin_schema::PluginThemeColor::Group(std::collections::BTreeMap::from([(
                        "red".into(),
                        plugin_schema::PluginThemeColor::Color("#f08070".into()),
                    )])),
                )]),
                typography: std::collections::BTreeMap::from([(
                    "tab".into(),
                    plugin_schema::PluginTextStyle {
                        family: Some("External Tabs".into()),
                        size_px: Some(18.),
                        bold: Some(true),
                    },
                )]),
                ..Default::default()
            },
        );
        apply_theme(&external, cx);

        let environment = environment(std::path::Path::new("workspace"), cx);
        assert!(environment.dark);
        assert_eq!(environment.background, 0x1e1f22);
        // Already installed guests retain their legacy theme tokens until updated.
        assert_eq!(environment.theme_colors.len(), 2);
        assert_eq!(environment.theme_colors["terminal.ansi.red"], 0xf08070);
        assert_eq!(environment.theme_colors["me.terminal.ansi.red"], 0xf08070);
        assert_eq!(environment.ui_font.family.as_deref(), Some("External UI"));
        assert_eq!(environment.ui_font.size_px, Some(16.));
        assert_eq!(
            environment.mono_font.family.as_deref(),
            Some("External Mono")
        );
        assert_eq!(
            environment.font_style("terminal", "tab", false),
            protocol::FontStyle {
                family: Some("External Tabs".into()),
                size_px: Some(18.),
                bold: Some(true),
            }
        );
    });
}

/// Explicit local-package confirmations still describe the version being installed accurately.
#[test]
fn package_confirmation_labels_cover_install_update_and_downgrade() {
    assert_eq!(surface::package_action("2.0.0", None), "安装");
    assert_eq!(surface::package_action("2.0.0", Some("1.0.0")), "更新");
    assert_eq!(surface::package_action("1.0.0", Some("2.0.0")), "降级安装");
    assert_eq!(surface::package_action("2.0.0", Some("2.0.0")), "已安装");
}

/// Clicking a market install action must immediately publish an inspecting state.
#[gpui::test]
fn market_install_button_reports_loading_before_inspection(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, editor_cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    let editor_window = editor_cx.update(|window, _| window.window_handle());
    editor_cx.update(|window, cx| {
        let owner = app.read(cx).extensions.clone();
        owner.update(cx, |owner, cx| {
            let manifest: protocol::Manifest =
                serde_json::from_str(include_str!("../../../../plugins/example/manifest.json"))
                    .unwrap();
            owner.manager_market = true;
            owner.manager_packages = vec![Package {
                manifest,
                files: Default::default(),
                digest: "fixture".into(),
                source: Some("first.zip".into()),
            }];
            cx.notify();
        });
        app.update(cx, |app, cx| app.toggle_extensions(window, cx));
    });
    let dialog_window = editor_cx
        .update(|_, cx| cx.windows())
        .into_iter()
        .find(|handle| *handle != editor_window)
        .unwrap();
    let dialog_cx = VisualTestContext::from_window(dialog_window, cx).into_mut();
    dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
    let install = dialog_cx
        .debug_bounds("plugin-install-action-region")
        .unwrap();
    dialog_cx.simulate_click(install.center(), Default::default());
    let action =
        dialog_cx.update(|_, cx| {
            let owner = app.read(cx).extensions.clone();
            let inspected = owner.read(cx).worker.recorded.lock().unwrap().try_iter()
            .any(|work| matches!(work, Work::Inspect(path) if path == PathBuf::from("first.zip")));
            assert!(inspected, "the first install click must reach the worker");
            owner
                .read(cx)
                .progress
                .as_ref()
                .map(|progress| progress.action)
        });
    assert!(
        action.is_some(),
        "inspection must mark the install action as loading"
    );
}

/// A second package's consent opens in a dialog above even a long README.
#[gpui::test]
fn second_market_install_shows_visible_consent(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, editor_cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    let editor_window = editor_cx.update(|window, _| window.window_handle());
    let first_manifest: protocol::Manifest =
        serde_json::from_str(include_str!("../../../../plugins/example/manifest.json")).unwrap();
    let second_manifest: protocol::Manifest =
        serde_json::from_str(include_str!("../../../../plugins/rust/manifest.json")).unwrap();
    let first = Package {
        manifest: first_manifest.clone(),
        files: Default::default(),
        digest: "first".into(),
        source: Some("first.zip".into()),
    };
    let second = Package {
        manifest: second_manifest.clone(),
        digest: "second".into(),
        source: Some("second.zip".into()),
        files: BTreeMap::from([(
            "README.md".into(),
            format!("# Rust\n\n{}", "A long explanation.\n\n".repeat(160)).into_bytes(),
        )]),
    };
    editor_cx.update(|window, cx| {
        let owner = app.read(cx).extensions.clone();
        owner.update(cx, |owner, cx| {
            owner.manager_market = true;
            owner.manager_selected = Some(second_manifest.id.clone());
            owner.manager_packages = vec![first, second.clone()];
            let mut state = owner.worker.state.lock().unwrap();
            state.entries = vec![Installed {
                manifest: first_manifest.clone(),
                digest: "first".into(),
                grants: first_manifest.permissions.clone(),
                enabled: true,
                project_enabled: Default::default(),
                global_enabled: Some(true),
                error: None,
            }];
            drop(state);
            owner.poll(cx);
        });
        app.update(cx, |app, cx| app.toggle_extensions(window, cx));
    });
    let dialog_window = editor_cx
        .update(|_, cx| cx.windows())
        .into_iter()
        .find(|handle| *handle != editor_window)
        .unwrap();
    let dialog_cx = VisualTestContext::from_window(dialog_window, cx).into_mut();
    dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
    let install = dialog_cx
        .debug_bounds("plugin-install-action-region")
        .unwrap();
    dialog_cx.simulate_click(install.center(), Default::default());
    dialog_cx.update(|_, cx| {
        let owner = app.read(cx).extensions.clone();
        let inspected = owner
            .read(cx)
            .worker
            .recorded
            .lock()
            .unwrap()
            .try_iter()
            .any(|work| matches!(work, Work::Inspect(path) if path == PathBuf::from("second.zip")));
        assert!(inspected, "the second install click must reach the worker");
        owner.update(cx, |owner, cx| {
            let mut state = owner.worker.state.lock().unwrap();
            state.pending = Some(second.clone());
            state.progress = None;
            drop(state);
            owner.poll(cx);
        });
    });
    dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
    let open = dialog_cx.update(|window, cx| window.has_active_dialog(cx));
    assert!(
        open,
        "the second installation must open a confirmation dialog"
    );
    dialog_cx.run_until_parked();
    dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
    dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        dialog_cx.debug_bounds("plugin-install-consent").is_some(),
        "package origin and permissions should render inside the dialog"
    );
    assert!(
        dialog_cx.debug_bounds("plugin-install-cancel").is_some(),
        "the dialog must show a cancel button"
    );
    assert!(
        dialog_cx.debug_bounds("plugin-install-confirm").is_some(),
        "the dialog must show a confirm button"
    );
    let viewport_height = dialog_cx.update(|window, _| window.viewport_size().height);
    assert!(
        dialog_cx
            .debug_bounds("plugin-install-confirm")
            .unwrap()
            .bottom()
            <= viewport_height,
        "the confirm button must stay within the visible window"
    );
    assert!(
        dialog_cx.debug_bounds("plugin-readme-region").is_some(),
        "README should remain in the detail pane behind the dialog"
    );
    let cancel = dialog_cx.debug_bounds("plugin-install-cancel").unwrap();
    dialog_cx.simulate_click(cancel.center(), Default::default());
    dialog_cx.run_until_parked();
    assert!(
        !dialog_cx.update(|window, cx| window.has_active_dialog(cx)),
        "the cancel button should dismiss the install confirmation"
    );
    assert!(
        dialog_cx.update(|_, cx| app.read(cx).extensions.read(cx).pending.is_none()),
        "dismissing the dialog must clear the pending package"
    );
    // Reopen the same package to prove cancellation does not block its next install.
    dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
    let install = dialog_cx
        .debug_bounds("plugin-install-action-region")
        .unwrap();
    dialog_cx.simulate_click(install.center(), Default::default());
    dialog_cx.update(|_, cx| {
        let owner = app.read(cx).extensions.clone();
        owner.update(cx, |owner, cx| {
            let mut state = owner.worker.state.lock().unwrap();
            state.pending = Some(second.clone());
            state.progress = None;
            drop(state);
            owner.poll(cx);
        });
    });
    dialog_cx.run_until_parked();
    dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
    dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
    let confirm = dialog_cx.debug_bounds("plugin-install-confirm").unwrap();
    dialog_cx.simulate_click(confirm.center(), Default::default());
    assert!(
        dialog_cx.update(|_, cx| {
            app.read(cx)
                .extensions
                .read(cx)
                .worker
                .recorded
                .lock()
                .unwrap()
                .try_iter()
                .any(|work| matches!(work, Work::Install(package) if package.digest == "second"))
        }),
        "confirming the reopened dialog must queue installation"
    );
    // Running installation owns a native progress dialog, with a responsive cancel control.
    dialog_cx.update(|_, cx| {
        app.read(cx)
            .extensions
            .clone()
            .update(cx, |owner, cx| owner.poll(cx))
    });
    dialog_cx.run_until_parked();
    dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
    dialog_cx.run_until_parked();
    let progress_window = dialog_cx
        .update(|_, cx| cx.windows())
        .into_iter()
        .find(|handle| *handle != editor_window && *handle != dialog_window)
        .expect("installation progress window");
    let progress_cx = VisualTestContext::from_window(progress_window, cx).into_mut();
    progress_cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        progress_cx
            .debug_bounds("plugin-install-progress")
            .is_some()
    );
    let button = progress_cx
        .debug_bounds("plugin-install-progress-close-region")
        .unwrap();
    progress_cx.simulate_click(button.center(), Default::default());
    let control = progress_cx.update(|_, cx| {
        app.read(cx)
            .extensions
            .read(cx)
            .worker
            .state
            .lock()
            .unwrap()
            .install_control
            .clone()
            .unwrap()
    });
    let package = super::language_tests::packages::language_package("cancelled-install");
    let mut manager = plugin_runtime::Manager::open(
        directory.path().join("cancelled-runtime"),
        protocol::Environment {
            workspace: directory.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        manager
            .install_with_control(&package, Default::default(), &control)
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
}

/// Uninstall choices belong to a modal and dispatch the selected data policy.
#[gpui::test]
fn uninstall_dialog_offers_both_data_choices(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, editor_cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    let editor_window = editor_cx.update(|window, _| window.window_handle());
    let manifest: protocol::Manifest =
        serde_json::from_str(include_str!("../../../../plugins/example/manifest.json")).unwrap();
    editor_cx.update(|window, cx| {
        let owner = app.read(cx).extensions.clone();
        owner.update(cx, |owner, cx| {
            // An available upgrade must not replace the installed plugin's uninstall control.
            let mut upgrade = manifest.clone();
            upgrade.version = "2.0.0".into();
            owner.manager_packages = vec![Package {
                manifest: upgrade,
                digest: "upgrade".into(),
                files: Default::default(),
                source: Some("upgrade.zip".into()),
            }];
            let mut state = owner.worker.state.lock().unwrap();
            state.entries = vec![Installed {
                manifest: manifest.clone(),
                digest: "fixture".into(),
                grants: manifest.permissions.clone(),
                enabled: false,
                project_enabled: Default::default(),
                global_enabled: Some(false),
                error: None,
            }];
            drop(state);
            owner.poll(cx);
        });
        app.update(cx, |app, cx| app.toggle_extensions(window, cx));
    });
    let dialog_window = editor_cx
        .update(|_, cx| cx.windows())
        .into_iter()
        .find(|handle| *handle != editor_window)
        .unwrap();
    let dialog_cx = VisualTestContext::from_window(dialog_window, cx).into_mut();
    dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
    let uninstall = dialog_cx
        .debug_bounds("plugin-uninstall-action-region")
        .unwrap();
    let update = dialog_cx
        .debug_bounds("plugin-update-action-region")
        .unwrap();
    // The installed tab must offer upgrades before removal without a market-tab visit.
    assert!(!dialog_cx.update(|_, cx| app.read(cx).extensions.read(cx).manager_market));
    assert!(
        update.right() <= uninstall.left(),
        "update must be left of uninstall"
    );
    assert!(
        dialog_cx
            .debug_bounds("plugin-install-action-region")
            .is_none()
    );
    let project = dialog_cx
        .debug_bounds("plugin-project-scope-region")
        .unwrap();
    let global = dialog_cx
        .debug_bounds("plugin-global-scope-region")
        .unwrap();
    assert!(
        global.right() <= project.left(),
        "project scope must be right of global scope"
    );
    dialog_cx.simulate_click(uninstall.center(), Default::default());
    dialog_cx.run_until_parked();
    dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
    dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(dialog_cx.update(|window, cx| window.has_active_dialog(cx)));
    assert!(dialog_cx.debug_bounds("plugin-remove-cancel").is_some());
    assert!(dialog_cx.debug_bounds("plugin-remove-preserve").is_some());
    assert!(dialog_cx.debug_bounds("plugin-remove-delete").is_some());
    assert!(dialog_cx.debug_bounds("plugin-readme-region").is_some());
    let cancel = dialog_cx.debug_bounds("plugin-remove-cancel").unwrap();
    dialog_cx.simulate_click(cancel.center(), Default::default());
    dialog_cx.run_until_parked();
    assert!(!dialog_cx.update(|window, cx| window.has_active_dialog(cx)));
    assert!(dialog_cx.update(|_, cx| app.read(cx).extensions.read(cx).confirm.is_none()));
    // The first retry keeps data; the next retry removes it.
    for delete_data in [false, true] {
        dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
        let uninstall = dialog_cx
            .debug_bounds("plugin-uninstall-action-region")
            .unwrap();
        dialog_cx.simulate_click(uninstall.center(), Default::default());
        dialog_cx.run_until_parked();
        dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
        dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
        let button = if delete_data {
            "plugin-remove-delete"
        } else {
            "plugin-remove-preserve"
        };
        let action = dialog_cx.debug_bounds(button).unwrap();
        dialog_cx.simulate_click(action.center(), Default::default());
        assert!(
            dialog_cx.update(|_, cx| {
                app.read(cx)
                    .extensions
                    .read(cx)
                    .worker
                    .recorded
                    .lock()
                    .unwrap()
                    .try_iter()
                    .any(|work| {
                        matches!(work, Work::Uninstall(id, delete) if id == manifest.id && delete == delete_data)
                    })
            }),
            "the selected data policy must reach the worker"
        );
        assert!(
            dialog_cx.update(|_, cx| {
                let owner = app.read(cx).extensions.clone();
                owner.read(cx).progress.as_ref().is_some_and(|progress| {
                    progress.action == LifecycleAction::Uninstall
                        && progress.delete_data == Some(delete_data)
                })
            }),
            "the uninstall button must expose the selected operation as loading"
        );
        dialog_cx.update(|_, cx| {
            let owner = app.read(cx).extensions.clone();
            owner.update(cx, |owner, cx| {
                owner.worker.state.lock().unwrap().progress = None;
                owner.poll(cx);
            });
        });
        dialog_cx.run_until_parked();
    }
}

/// Both plugin-owned SVG variants must render with their intended foreground ink.
#[test]
fn terminal_theme_icons_render_in_opposite_colors() {
    use gpui_kit::{Image, ImageFormat, SvgRenderer};

    for (bytes, expected) in [
        (
            include_bytes!("../../../../plugins/terminal/icons/terminal_light.svg").as_slice(),
            0,
        ),
        (
            include_bytes!("../../../../plugins/terminal/icons/terminal_dark.svg").as_slice(),
            255,
        ),
    ] {
        let image = Image::from_bytes(ImageFormat::Svg, bytes.to_vec());
        let rendered = image
            .to_image_data(SvgRenderer::new(Arc::new(())))
            .expect("terminal SVG should render");
        let pixels = rendered.as_bytes(0).expect("SVG should have a frame");
        assert!(
            pixels.chunks_exact(4).any(|pixel| {
                pixel[3] > 0 && pixel[..3].iter().all(|channel| *channel == expected)
            }),
            "terminal icon did not contain the expected light or dark ink"
        );
    }
}

/// A manifest dynamically adds a bottom dock; generic input commits Enter and blur once.
#[gpui::test]
fn plugin_panel_registration_input_and_ime(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        init(cx);
        cx.bind_keys([KeyBinding::new(
            "ctrl-s",
            SaveDocument,
            Some("EditorShell && !PluginSurface"),
        )]);
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
    let mut manifest: protocol::Manifest =
        serde_json::from_str(include_str!("../../../../plugins/terminal/manifest.json")).unwrap();
    // This test explicitly opens the dock; the packaged terminal now starts hidden.
    manifest.panels[0].default_visible = true;
    let scene = Scene {
        panel: "terminal".into(),
        font: "Cascadia Mono".into(),
        font_size: 14.,
        cursor: protocol::Rect {
            x: 8.,
            y: 8.,
            w: 8.,
            h: 20.,
        },
        ..Scene::default()
    };
    cx.simulate_resize(size(px(1100.), px(800.)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let full_editor = cx.debug_bounds("editor-panel-content").unwrap();
    let (owner, panel) = cx.update(|window, cx| {
        let owner = app.read(cx).extensions.clone();
        owner.update(cx, |owner, cx| {
            let mut state = owner.worker.state.lock().unwrap();
            state.entries = vec![Installed {
                manifest: manifest.clone(),
                digest: "fixture".into(),
                grants: manifest.permissions.clone(),
                enabled: true,
                project_enabled: Default::default(),
                global_enabled: None,
                error: None,
            }];
            state
                .scenes
                .insert("terminal/terminal".into(), Arc::new(scene.clone()));
            drop(state);
            owner.poll(cx);
        });
        app.update(cx, |app, cx| app.sync_plugin_panels(window, cx));
        let panel = app.read(cx).plugin_panels["terminal/terminal"].clone();
        panel.update(cx, |panel, cx| {
            panel.poll(cx);
            panel.focus(window, cx);
        });
        window.draw(cx).clear(cx);
        (owner, panel)
    });
    let bounds = cx
        .debug_bounds("plugin-surface")
        .expect("manifest panel is visible without restarting");
    assert!(bounds.top() > px(300.));
    assert!(bounds.size.height > px(100.));
    cx.simulate_keystrokes("ctrl-s");
    // A shell command must retain spaces through the native keyboard/text input path.
    cx.simulate_keystrokes("space");
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.replace_text_in_range(None, "中文", window, cx)
        })
    });
    let messages = cx.update(|_, cx| {
        owner
            .read(cx)
            .worker
            .recorded
            .lock()
            .unwrap()
            .try_iter()
            .collect::<Vec<_>>()
    });
    fn event(work: &Work) -> Option<&PluginEvent> {
        if let Work::Event(_, _, event) = work {
            if let PluginEvent::Surface { event, .. } = event {
                Some(event)
            } else {
                Some(event)
            }
        } else {
            None
        }
    }
    assert!(
        messages
            .iter()
            .filter_map(event)
            .any(|event| matches!(event,PluginEvent::Key{key,ctrl:true,..}if key=="s"))
    );
    assert!(
        messages
            .iter()
            .filter_map(event)
            .any(|event| matches!(event,PluginEvent::Text(text)if text=="中文"))
    );
    assert_eq!(
        messages
            .iter()
            .filter_map(event)
            .filter(|event| matches!(event, PluginEvent::Text(text) if text == " "))
            .count(),
        1,
        "the space key must deliver a printable space to the plugin"
    );
    // Drag the dock's upper edge, then verify both the visible size and persisted height.
    let edge = point(bounds.center().x, bounds.top() - px(PANEL_HEADER_HEIGHT));
    let target = edge - point(px(0.), px(100.));
    cx.simulate_mouse_down(edge, MouseButton::Left, Default::default());
    cx.simulate_mouse_move(
        edge - point(px(0.), px(10.)),
        MouseButton::Left,
        Default::default(),
    );
    cx.simulate_mouse_move(target, MouseButton::Left, Default::default());
    cx.simulate_mouse_up(target, MouseButton::Left, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let resized = cx.debug_bounds("plugin-surface").unwrap();
    assert!(
        resized.size.height > bounds.size.height + px(60.),
        "dragging the dock edge must enlarge the terminal: {bounds:?} -> {resized:?}"
    );
    cx.update(|_, cx| {
        let height = app.read(cx).session_state.plugin_dock_sizes["bottom"];
        assert!((height - (resized.size.height / px(1.) + PANEL_HEADER_HEIGHT)).abs() < 2.);
    });
    // The same handle must shrink the dock and forward its new viewport to the guest.
    let edge = point(resized.center().x, resized.top() - px(PANEL_HEADER_HEIGHT));
    let target = edge + point(px(0.), px(70.));
    cx.simulate_mouse_down(edge, MouseButton::Left, Default::default());
    cx.simulate_mouse_move(
        edge + point(px(0.), px(10.)),
        MouseButton::Left,
        Default::default(),
    );
    cx.simulate_mouse_move(target, MouseButton::Left, Default::default());
    cx.simulate_mouse_up(target, MouseButton::Left, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let shrunk = cx.debug_bounds("plugin-surface").unwrap();
    assert!(shrunk.size.height < resized.size.height - px(40.));
    let messages = cx.update(|_, cx| {
        owner
            .read(cx)
            .worker
            .recorded
            .lock()
            .unwrap()
            .try_iter()
            .collect::<Vec<_>>()
    });
    assert!(messages.iter().filter_map(event).any(|event| matches!(event, PluginEvent::Resize { height, .. } if (*height - shrunk.size.height / px(1.)).abs() < 2.)));
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            let mut scene = scene.clone();
            scene.widgets.push(protocol::Widget {
                id: "rename:1".into(),
                rect: protocol::Rect {
                    x: 600.,
                    y: 0.,
                    w: 150.,
                    h: 28.,
                },
                label: "powershell".into(),
                edit: true,
                style: Default::default(),
            });
            panel
                .scenes
                .insert("terminal/terminal".into(), Arc::new(scene.clone()));
            panel
                .worker
                .state
                .lock()
                .unwrap()
                .scenes
                .insert("terminal/terminal".into(), Arc::new(scene));
            panel.sync_edit(window, cx);
            cx.notify();
        });
        window.draw(cx).clear(cx);
    });
    // Native input mounting focuses on the deferred UI turn; Enter must follow it.
    cx.run_until_parked();
    cx.update(|window, cx| {
        let input = panel.read(cx).editing.as_ref().unwrap().input.clone();
        input.update(cx, |input, cx| input.replace_all("构建任务", window, cx));
        assert_eq!(input.read(cx).value().as_str(), "构建任务");
        window.draw(cx).clear(cx);
    });
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let messages = cx.update(|_, cx| {
        owner
            .read(cx)
            .worker
            .recorded
            .lock()
            .unwrap()
            .try_iter()
            .collect::<Vec<_>>()
    });
    assert_eq!(messages.iter().filter_map(event).filter(|event|matches!(event,PluginEvent::Edit{id,text}if id=="rename:1"&&text=="构建任务")).count(),1);
    // Hiding the last dock panel must return its entire height to the editor.
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.visible.set(false);
            cx.notify();
        });
        app.read(cx)
            .dock_area
            .clone()
            .update(cx, |_, cx| cx.notify());
        window.draw(cx).clear(cx);
    });
    assert_eq!(
        cx.debug_bounds("editor-panel-content").unwrap(),
        full_editor,
        "hiding the terminal must not leave an empty bottom dock"
    );
    // Showing it again preserves the user's chosen dock height.
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.visible.set(true);
            cx.notify();
        });
        app.read(cx)
            .dock_area
            .clone()
            .update(cx, |_, cx| cx.notify());
        window.draw(cx).clear(cx);
    });
    assert_eq!(cx.debug_bounds("plugin-surface").unwrap().size, shrunk.size);
    // Uninstall removes the last native contribution and must also reclaim its region.
    cx.update(|window, cx| {
        owner.update(cx, |owner, cx| {
            owner.entries.clear();
            owner.worker.state.lock().unwrap().entries.clear();
            cx.notify();
        });
        app.update(cx, |app, cx| app.sync_plugin_panels(window, cx));
        assert!(app.read(cx).plugin_panels.is_empty());
        window.draw(cx).clear(cx);
    });
    assert_eq!(
        cx.debug_bounds("editor-panel-content").unwrap(),
        full_editor,
        "uninstalling the last plugin must not leave an empty bottom dock"
    );
}

/// A deferred legacy editor action cannot open data after replacement or trust revocation.
#[gpui::test]
fn deferred_legacy_effects_recheck_epoch_and_trust(cx: &mut TestAppContext) {
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
    for revoked in [false, true] {
        cx.update(|_, cx| {
            let owner = app.read(cx).extensions.clone();
            owner.update(cx, |owner, cx| {
                let manifest: protocol::Manifest = serde_json::from_str(include_str!(
                    "../../../../plugins/terminal/manifest.json"
                ))
                .unwrap();
                owner
                    .worker
                    .trusted
                    .store(true, std::sync::atomic::Ordering::Release);
                {
                    let mut state = owner.worker.state.lock().unwrap();
                    state.entries = vec![Installed {
                        grants: manifest.permissions.clone(),
                        manifest,
                        digest: "fixture".into(),
                        enabled: true,
                        project_enabled: Default::default(),
                        global_enabled: None,
                        error: None,
                    }];
                    state.instance_epochs.insert("terminal".into(), 7);
                    state.effects.push((
                        "terminal".into(),
                        Request::Editor {
                            command: "open_data:late.json".into(),
                        },
                    ));
                }
                // Poll captures the old effect and queues its deferred native side effect.
                owner.poll(cx);
                if revoked {
                    owner
                        .worker
                        .trusted
                        .store(false, std::sync::atomic::Ordering::Release);
                } else {
                    owner
                        .worker
                        .state
                        .lock()
                        .unwrap()
                        .instance_epochs
                        .insert("terminal".into(), 8);
                }
            });
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert!(!app.read(cx).status.contains("late.json"));
            assert!(app.read(cx).pending_plugin_file.is_none());
            let owner = app.read(cx).extensions.read(cx);
            assert!(!owner.worker.recorded.lock().unwrap().try_iter().any(|work| matches!(work, Work::Event(_, _, PluginEvent::Command { id, .. }) if id == "open_data:late.json.result")));
        });
    }
}
