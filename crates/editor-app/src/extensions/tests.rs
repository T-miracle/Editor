//! Exercise dynamic native panels, declarative input commits and shortcut isolation.
use super::*;
use gpui_kit::component::WindowExt as _;
use gpui_kit::{TestAppContext, VisualTestContext, gpui};

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
        // Imported theme data exposes one current namespace, never duplicate runtime aliases.
        assert_eq!(environment.theme_colors.len(), 1);
        assert_eq!(environment.theme_colors["terminal.ansi.red"], 0xf08070);
        assert!(
            !environment
                .theme_colors
                .contains_key("me.terminal.ansi.red")
        );
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
            let manifest: protocol::Manifest = crate::extensions::test_manifest(include_str!(
                "../../../../plugins/example/manifest.json"
            ));
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
        crate::extensions::test_manifest(include_str!("../../../../plugins/example/manifest.json"));
    let second_manifest: protocol::Manifest =
        crate::extensions::test_manifest(include_str!("../../../../plugins/rust/manifest.json"));
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
                retired_ui_contract: false,
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
    // A short installation status must not reserve the canvas of a settings dialog.
    let progress_size = progress_cx.update(|window, _| window.viewport_size());
    assert!(progress_size.width <= px(520.));
    assert!(progress_size.height <= px(260.));
    assert!(
        progress_cx
            .debug_bounds("plugin-install-progress")
            .is_some()
    );
    let button = progress_cx
        .debug_bounds("plugin-install-progress-close-region")
        .unwrap();
    // The compact viewport must still keep cancellation visible and clickable.
    assert!(button.origin.y + button.size.height <= progress_size.height);
    // Long progress details scroll inside the compact window without moving its action row.
    progress_cx.update(|_, cx| {
        let owner = app.read(cx).extensions.clone();
        owner.update(cx, |owner, cx| {
            owner
                .worker
                .state
                .lock()
                .unwrap()
                .installation
                .as_mut()
                .unwrap()
                .message = "正在准备插件依赖与资源。\n".repeat(80);
            cx.notify();
        });
    });
    progress_cx.run_until_parked();
    progress_cx.update(|window, cx| window.draw(cx).clear(cx));
    let progress_region = progress_cx.debug_bounds("plugin-install-progress").unwrap();
    progress_cx.simulate_event(gpui_kit::ScrollWheelEvent {
        position: progress_region.center(),
        delta: gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-600.))),
        touch_phase: gpui_kit::TouchPhase::Moved,
        modifiers: Default::default(),
    });
    progress_cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(
        button,
        progress_cx
            .debug_bounds("plugin-install-progress-close-region")
            .unwrap()
    );
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
        crate::extensions::test_manifest(include_str!("../../../../plugins/example/manifest.json"));
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
                retired_ui_contract: false,
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
fn incompatible_plugin_details_keep_preferences_and_update_uninstall_actions(
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
    let project = workspace.root().display().to_string();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    let editor_window = cx.update(|window, _| window.window_handle());
    cx.update(|window, cx| {
        let owner = app.read(cx).extensions.clone();
        owner.update(cx, |owner, cx| {
            let mut old: protocol::Manifest =
                serde_json::from_str(include_str!("../../../../plugins/terminal/manifest.json"))
                    .unwrap();
            // Keep a genuinely legacy installed fixture after the delivered package migrates.
            old.protocol = 5;
            // Same-version SDK repacks must still offer an update for an incompatible installed protocol.
            let current = crate::extensions::test_manifest(include_str!(
                "../../../../plugins/terminal/manifest.json"
            ));
            owner.manager_selected = Some(old.id.clone());
            owner.manager_packages = vec![Package {
                manifest: current,
                digest: "current".into(),
                files: Default::default(),
                source: Some("sdk-repack.zip".into()),
            }];
            owner.worker.state.lock().unwrap().entries = vec![Installed {
                grants: old.permissions.clone(),
                manifest: old,
                digest: "old".into(),
                enabled: false,
                global_enabled: Some(false),
                retired_ui_contract: false,
                project_enabled: [project.clone()].into(),
                error: None,
            }];
            owner.poll(cx);
        });
        app.update(cx, |app, cx| app.toggle_extensions(window, cx));
    });
    let dialog = cx
        .update(|_, cx| cx.windows())
        .into_iter()
        .find(|handle| *handle != editor_window)
        .unwrap();
    let dialog_cx = VisualTestContext::from_window(dialog, cx).into_mut();
    dialog_cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        dialog_cx
            .debug_bounds("plugin-incompatible-status")
            .is_some()
    );
    assert!(
        dialog_cx
            .debug_bounds("plugin-update-action-region")
            .is_some()
    );
    assert!(
        dialog_cx
            .debug_bounds("plugin-uninstall-action-region")
            .is_some()
    );
    assert!(dialog_cx.debug_bounds("plugin-restart-action").is_none());
    let global = dialog_cx
        .debug_bounds("plugin-global-scope-region")
        .unwrap();
    let project_scope = dialog_cx
        .debug_bounds("plugin-project-scope-region")
        .unwrap();
    dialog_cx.simulate_click(global.center(), Default::default());
    dialog_cx.simulate_click(project_scope.center(), Default::default());
    dialog_cx.run_until_parked();
    dialog_cx.update(|_, cx| {
        let owner = app.read(cx).extensions.read(cx);
        let entry = &owner.entries[0];
        assert!(!entry.global_enabled.unwrap());
        assert!(entry.project_enabled_in(&project));
        assert!(
            !owner
                .worker
                .recorded
                .lock()
                .unwrap()
                .try_iter()
                .any(|work| matches!(
                    work,
                    Work::Enable(_)
                        | Work::Disable(_)
                        | Work::Restart(_)
                        | Work::SetProjectEnabled(_, _)
                ))
        );
    });
    let update = dialog_cx
        .debug_bounds("plugin-update-action-region")
        .unwrap();
    dialog_cx.simulate_click(update.center(), Default::default());
    dialog_cx.update(|_, cx| {
        assert!(app.read(cx).extensions.read(cx).worker.recorded.lock().unwrap().try_iter()
            .any(|work| matches!(work, Work::Inspect(path) if path == PathBuf::from("sdk-repack.zip"))));
    });
}
