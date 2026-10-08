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
            state.logs = manager.runtime_logs();
            state.logs.append(
                "manager-first",
                plugin_runtime::logs::LogLevel::Info,
                "language.status:manager-first/analysis",
                "Ready",
            );
            state.logs.append(
                "manager-first",
                plugin_runtime::logs::LogLevel::Error,
                "host.operation",
                "First plugin operation failed",
            );
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

/// GPUI's test lookup requires static selectors; the two dynamic record names live for this test process.
fn record_selector(id: u64) -> &'static str {
    Box::leak(format!("plugin-log-record-{id}").into_boxed_str())
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

/// Native navigation acknowledges one plugin while preserving its history, other plugins and colors.
#[gpui::test]
fn log_view_shares_read_state_without_clearing_other_plugins(cx: &mut TestAppContext) {
    with_manager(cx, "zh-CN", false, |form, owner| {
        let logs = form.update(|_, cx| owner.read(cx).runtime_logs());
        let second = logs.append(
            "manager-second",
            plugin_runtime::logs::LogLevel::Warning,
            "guest.stderr",
            "另一个插件的警告",
        );
        let first_error = logs.records("manager-first").last().unwrap().id;
        draw(form);
        assert!(form.debug_bounds("plugin-detail-tabs-5-Error").is_some());
        click(form, "plugin-detail-tabs-5");
        draw(form);
        assert!(form.debug_bounds("plugin-detail-tabs-5-Error").is_none());
        assert_eq!(logs.unread_severity("manager-first"), None);
        assert_eq!(
            logs.records("manager-first").last().unwrap().id,
            first_error
        );
        assert_eq!(
            logs.records("manager-first").last().unwrap().level,
            plugin_runtime::logs::LogLevel::Error
        );
        assert_eq!(
            logs.unread_severity("manager-second"),
            Some(plugin_runtime::logs::LogLevel::Warning)
        );
        click(form, "plugin-row-manager-second");
        assert!(form.debug_bounds("plugin-detail-tabs-5-Warning").is_some());
        click(form, "plugin-detail-tabs-5");
        draw(form);
        assert_eq!(logs.unread_severity("manager-second"), None);
        assert!(form.debug_bounds(record_selector(second)).is_some());
        click(form, "plugin-detail-tabs-0");
        logs.append(
            "manager-second",
            plugin_runtime::logs::LogLevel::Info,
            "guest.stdout",
            "normal output",
        );
        draw(form);
        assert!(form.debug_bounds("plugin-detail-tabs-5-Warning").is_none());
        logs.append(
            "manager-second",
            plugin_runtime::logs::LogLevel::Warning,
            "guest.stderr",
            "new warning",
        );
        draw(form);
        assert!(form.debug_bounds("plugin-detail-tabs-5-Warning").is_some());
        logs.append(
            "manager-second",
            plugin_runtime::logs::LogLevel::Error,
            "host.plugin",
            "new failure",
        );
        draw(form);
        assert!(form.debug_bounds("plugin-detail-tabs-5-Error").is_some());
    });
}

/// Descending logs retain a captured read boundary and keep new top entries unread while reviewing history.
#[gpui::test]
fn long_runtime_logs_scroll_below_fixed_tabs(cx: &mut TestAppContext) {
    with_manager(cx, "en", true, |form, owner| {
        let logs = form.update(|_, cx| owner.read(cx).runtime_logs());
        let initial_count = logs.records("manager-first").len();
        for index in 0..100 {
            logs.append(
                "manager-first",
                plugin_runtime::logs::LogLevel::Warning,
                "guest.stderr",
                format!(
                    "record {index}: {}",
                    "可读的长日志 readable text ".repeat(12)
                ),
            );
        }
        form.update(|_, cx| {
            typography::set_font_size(cx, 20.);
            apply_theme(builtin_theme(true), cx);
        });
        draw(form);
        click(form, "plugin-detail-tabs-5");
        draw(form);
        // Opening the page acknowledges its captured maximum receipt ID, including old offscreen
        // warnings. Display order must not accidentally shrink that boundary to the oldest row.
        assert_eq!(logs.unread_severity("manager-first"), None);
        let header = form.debug_bounds("plugin-manager-header").unwrap();
        let tabs = form.debug_bounds("plugin-detail-tabs-5").unwrap();
        let viewport = form.debug_bounds("plugin-manager-detail-content").unwrap();
        form.simulate_event(gpui_kit::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-600.))),
            touch_phase: gpui_kit::TouchPhase::Moved,
            modifiers: Default::default(),
        });
        draw(form);
        assert_eq!(header, form.debug_bounds("plugin-manager-header").unwrap());
        assert_eq!(tabs, form.debug_bounds("plugin-detail-tabs-5").unwrap());
        assert!(form.update(|_, cx| owner.read(cx).manager_detail_scroll.offset().y < px(0.)));
        // In descending order, review the oldest records before the new error arrives at the top.
        form.simulate_event(gpui_kit::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-99_999.))),
            touch_phase: gpui_kit::TouchPhase::Moved,
            modifiers: Default::default(),
        });
        draw(form);
        let previous_offset = form.update(|_, cx| owner.read(cx).manager_detail_scroll.offset());
        let late = logs.append(
            "manager-first",
            plugin_runtime::logs::LogLevel::Error,
            "host.plugin",
            "new offscreen error",
        );
        draw(form);
        assert_eq!(logs.records("manager-first").len(), initial_count + 101);
        assert_eq!(
            form.update(|_, cx| owner.read(cx).manager_detail_scroll.offset()),
            previous_offset,
            "an incoming top entry must not move the user's history viewport"
        );
        let late_bounds = form.debug_bounds(record_selector(late)).unwrap();
        assert!(late_bounds.bottom() <= viewport.top());
        assert_eq!(
            logs.unread_severity("manager-first"),
            Some(plugin_runtime::logs::LogLevel::Error)
        );
        assert!(form.debug_bounds("plugin-detail-tabs-5-Error").is_some());
        // Only scrolling back to the new message reads it and removes the unread error indicator.
        form.simulate_event(gpui_kit::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui_kit::ScrollDelta::Pixels(point(px(0.), px(99_999.))),
            touch_phase: gpui_kit::TouchPhase::Moved,
            modifiers: Default::default(),
        });
        draw(form);
        let late_bounds = form.debug_bounds(record_selector(late)).unwrap();
        assert!(late_bounds.intersects(&viewport));
        assert_eq!(logs.unread_severity("manager-first"), None);
        assert!(form.debug_bounds("plugin-detail-tabs-5-Error").is_none());
        assert_eq!(header, form.debug_bounds("plugin-manager-header").unwrap());
        assert_eq!(tabs, form.debug_bounds("plugin-detail-tabs-5").unwrap());
    });
}

/// Latest receipts appear first even when their displayed, second-resolution timestamps match.
#[gpui::test]
fn runtime_logs_show_latest_receipts_at_the_top(cx: &mut TestAppContext) {
    with_manager(cx, "zh-CN", false, |form, owner| {
        let logs = form.update(|_, cx| owner.read(cx).runtime_logs());
        let mut same_second = None;
        // Appending through the manager's shared sink preserves real record identities. Repeating
        // the short pair handles a wall-clock second changing between the first two receipts.
        for _ in 0..4 {
            let earlier = logs.append(
                "manager-second",
                plugin_runtime::logs::LogLevel::Warning,
                "guest.stderr",
                "较早的同秒日志",
            );
            let latest = logs.append(
                "manager-second",
                plugin_runtime::logs::LogLevel::Error,
                "host.plugin",
                "最新的同秒日志",
            );
            let records = logs.records("manager-second");
            let pair = &records[records.len() - 2..];
            if pair[0]
                .time
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                == pair[1]
                    .time
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            {
                same_second = Some((earlier, latest));
                break;
            }
        }
        let (earlier, latest) =
            same_second.expect("two adjacent receipts within one displayed second");
        assert!(latest > earlier);
        click(form, "plugin-row-manager-second");
        click(form, "plugin-detail-tabs-5");
        let earlier_bounds = form.debug_bounds(record_selector(earlier)).unwrap();
        let latest_bounds = form.debug_bounds(record_selector(latest)).unwrap();
        let viewport = form.debug_bounds("plugin-manager-detail-content").unwrap();
        assert!(
            latest_bounds.top() < earlier_bounds.top(),
            "latest receipt must precede its same-second peer"
        );
        assert!(latest_bounds.intersects(&viewport));
        assert_eq!(logs.unread_severity("manager-second"), None);
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
