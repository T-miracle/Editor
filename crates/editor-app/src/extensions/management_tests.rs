//! Real declarative ZIPs enter through the public manager before native detail controls are exercised.
use super::*;
use gpui_kit::{TestAppContext, VisualTestContext, gpui};
use std::io::{Cursor, Write};

/// Resource-only fixtures require no external tools and still use package inspection and installation.
fn package(id: &str, version: &str) -> Package {
    let manifest = serde_json::json!({
        "id": id, "name": id, "version": version, "protocol": 7,
        "api": {"base": "^1"}, "contributions": "plugin.toml", "storage_limit": 1024
    });
    let declaration = format!(
        "[plugin]\nid = \"{id}\"\nname = \"{id}\"\nversion = \"{version}\"\nhost_version = \">=0.1.0\"\n"
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
        ("plugin.toml", declaration.into_bytes()),
        (
            "README.md",
            format!(
                "# {id}\n\n{}",
                "说明文本 / explanation paragraph.\n\n".repeat(160)
            )
            .into_bytes(),
        ),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&zip.finish().unwrap().into_inner()).unwrap()
}

/// Keep isolated storage alive for the whole scenario, and restore the process-wide locale on return.
fn with_manager(
    cx: &mut TestAppContext,
    locale: &str,
    dark: bool,
    scenario: impl FnOnce(&mut VisualTestContext, &Entity<ExtensionPanel>),
) {
    let previous_locale = rust_i18n::locale().to_string();
    rust_i18n::set_locale(locale);
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(dark), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let mut manager = plugin_runtime::Manager::open(
        workspace.root().join(".runtime-plugin-test"),
        protocol::Environment {
            workspace: workspace.root().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    for id in ["manager-first", "manager-second"] {
        let package = package(id, "1.0.0");
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
    }
    // This real override makes every action, including the project option, applicable at once.
    manager.disable("manager-first").unwrap();
    manager.set_project_enabled("manager-first", true).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, editor_cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    let editor_window = editor_cx.update(|window, _| window.window_handle());
    let owner = editor_cx.update(|window, cx| {
        let owner = app.read(cx).extensions.clone();
        owner.update(cx, |owner, cx| {
            let mut update = package("manager-first", "1.1.0");
            update.source = Some("candidate.zip".into());
            owner.manager_packages = vec![update];
            let mut state = owner.worker.state.lock().unwrap();
            state.entries = manager.published_entries();
            state
                .service_states
                .insert("manager-first/analysis".into(), "Ready".into());
            state.status = Some(worker::OperationStatus {
                plugin: Some("manager-first".into()),
                message: "First plugin operation failed".into(),
            });
            drop(state);
            owner.poll(cx);
        });
        app.update(cx, |app, cx| app.toggle_extensions(window, cx));
        owner
    });
    let dialog = editor_cx
        .update(|_, cx| cx.windows())
        .into_iter()
        .find(|window| *window != editor_window)
        .unwrap();
    let form = VisualTestContext::from_window(dialog, editor_cx).into_mut();
    draw(form);
    scenario(form, &owner);
    rust_i18n::set_locale(&previous_locale);
}

/// Refresh layout and event listeners after every state-changing input.
fn draw(form: &mut VisualTestContext) {
    form.run_until_parked();
    form.update(|window, cx| window.draw(cx).clear(cx));
}

/// Use the actual pointer route, including Base's disabled and focus behavior.
fn click(form: &mut VisualTestContext, selector: &'static str) {
    let bounds = form
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    form.simulate_click(bounds.center(), Default::default());
    draw(form);
}

/// Both local tab strips must retain fixed bounds while only README content scrolls.
#[gpui::test]
fn fixed_header_and_available_tabs_in_chinese_light_theme(cx: &mut TestAppContext) {
    with_manager(cx, "zh-CN", false, check_detail_navigation);
}

/// The same behavior is exercised with longer English labels and the other theme palette.
#[gpui::test]
fn fixed_header_and_available_tabs_in_english_dark_theme(cx: &mut TestAppContext) {
    with_manager(cx, "en", true, check_detail_navigation);
}

/// Closing and reopening the native manager restores its default page without recreating plugins.
#[gpui::test]
fn newly_opened_manager_starts_at_overview(cx: &mut TestAppContext) {
    with_manager(cx, "zh-CN", false, |form, owner| {
        click(form, "plugin-detail-tabs-5");
        let app = form.update(|_, cx| owner.read(cx).parent.upgrade().unwrap());
        // Use the dialog's actual Escape handler; this window has no platform should-close callback.
        form.simulate_keystrokes("escape");
        form.run_until_parked();
        assert_eq!(form.cx.update(|cx| cx.windows().len()), 1);
        let main_window = form.cx.update(|cx| cx.windows()[0]);
        form.cx
            .update_window(main_window, |_, window, cx| {
                app.update(cx, |app, cx| app.toggle_extensions(window, cx));
            })
            .unwrap();
        let dialog = form
            .cx
            .update(|cx| cx.windows())
            .into_iter()
            .find(|window| *window != main_window)
            .unwrap();
        let reopened = VisualTestContext::from_window(dialog, &form.cx).into_mut();
        draw(reopened);
        assert!(reopened.debug_bounds("plugin-readme-region").is_some());
        assert!(reopened.debug_bounds("plugin-runtime-log").is_none());
        assert_eq!(
            reopened.update(|_, cx| owner.read(cx).manager_detail_tab),
            management::DetailTab::Overview
        );
    });
}

/// Validate scrolling, disabled pointer targets, compound keyboard navigation and real restart routing.
fn check_detail_navigation(form: &mut VisualTestContext, owner: &Entity<ExtensionPanel>) {
    assert!(form.debug_bounds("plugin-readme-region").is_some());
    assert!(form.debug_bounds("plugin-runtime-log").is_none());
    let header = form.debug_bounds("plugin-manager-header").unwrap();
    let tab = form.debug_bounds("plugin-detail-tabs-0").unwrap();
    let content = form.debug_bounds("plugin-manager-detail-content").unwrap();
    assert!(content.top() >= tab.bottom());
    form.simulate_event(gpui_kit::ScrollWheelEvent {
        position: content.center(),
        delta: gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-600.))),
        touch_phase: gpui_kit::TouchPhase::Moved,
        modifiers: Default::default(),
    });
    draw(form);
    assert!(form.update(|_, cx| owner.read(cx).manager_detail_scroll.offset().y < px(0.)));
    assert_eq!(header, form.debug_bounds("plugin-manager-header").unwrap());
    assert_eq!(tab, form.debug_bounds("plugin-detail-tabs-0").unwrap());
    for selector in [
        "plugin-detail-tabs-1",
        "plugin-detail-tabs-2",
        "plugin-detail-tabs-3",
        "plugin-detail-tabs-4",
    ] {
        click(form, selector);
        assert_eq!(
            form.update(|_, cx| owner.read(cx).manager_detail_tab),
            management::DetailTab::Overview
        );
    }
    click(form, "plugin-detail-tabs-0");
    for (key, expected) in [
        ("right", management::DetailTab::RuntimeLog),
        ("left", management::DetailTab::Overview),
        ("end", management::DetailTab::RuntimeLog),
        ("home", management::DetailTab::Overview),
    ] {
        form.simulate_keystrokes(key);
        draw(form);
        assert_eq!(
            form.update(|_, cx| owner.read(cx).manager_detail_tab),
            expected
        );
    }
    click(form, "plugin-detail-tabs-5");
    assert!(form.debug_bounds("plugin-readme-region").is_none());
    assert!(form.debug_bounds("plugin-runtime-service-status").is_some());
    assert!(
        form.debug_bounds("plugin-runtime-operation-error")
            .is_some()
    );
    click(form, "plugin-row-manager-second");
    assert!(form.debug_bounds("plugin-readme-region").is_some());
    click(form, "plugin-detail-tabs-5");
    assert!(
        form.debug_bounds("plugin-runtime-operation-error")
            .is_none()
    );
    assert!(form.debug_bounds("plugin-runtime-service-status").is_none());
    // Unscoped worker failures are still visible without being attributed to this second plugin.
    form.update(|_, cx| {
        owner.update(cx, |owner, cx| {
            owner.worker.state.lock().unwrap().status = Some(worker::OperationStatus {
                plugin: None,
                message: "Manager could not read its registry".into(),
            });
            owner.poll(cx);
        })
    });
    draw(form);
    assert!(
        form.debug_bounds("plugin-manager-operation-error")
            .is_some()
    );
    assert!(
        form.debug_bounds("plugin-runtime-operation-error")
            .is_none()
    );
    click(form, "plugin-row-manager-first");
    let restart = form.debug_bounds("plugin-restart-action").unwrap();
    let global = form.debug_bounds("plugin-global-scope-region").unwrap();
    assert!(restart.right() <= global.left());
    click(form, "plugin-restart-action");
    assert!(form.update(|_, cx| {
        owner
            .read(cx)
            .worker
            .recorded
            .lock()
            .unwrap()
            .try_iter()
            .any(|work| matches!(work, Work::Restart(id) if id == "manager-first"))
    }));
    form.update(|_, cx| {
        owner.update(cx, |owner, cx| {
            owner.worker.state.lock().unwrap().progress = None;
            owner.poll(cx);
        });
        typography::set_font_size(cx, 20.);
        apply_theme(builtin_theme(cx.theme().is_dark()), cx);
    });
    form.simulate_resize(size(px(850.), px(600.)));
    draw(form);
    let detail = form.debug_bounds("plugin-manager-detail").unwrap();
    for selector in [
        "plugin-update-action-region",
        "plugin-uninstall-action-region",
        "plugin-restart-action",
        "plugin-global-scope-region",
        "plugin-project-scope-region",
    ] {
        let bounds = form.debug_bounds(selector).unwrap();
        assert!(
            bounds.left() >= detail.left() && bounds.right() <= detail.right(),
            "{selector} escaped the detail viewport: {bounds:?} / {detail:?}"
        );
    }
}
