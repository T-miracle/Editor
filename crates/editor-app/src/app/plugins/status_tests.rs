//! Declarative packages and native status-bar input exercise the shared runtime-log viewing boundary.
use super::*;
use gpui_kit::{TestAppContext, VisualTestContext, gpui};
use plugin_runtime::{HostResources, Manager, Package, plugin_protocol::Environment};
use std::{
    cell::RefCell,
    io::{Cursor, Write},
};

/// A resource-only package enters through ZIP validation and the same public installer as real plugins.
fn package(id: &str) -> Package {
    let manifest = serde_json::json!({
        "id": id, "name": id, "version": "1.0.0", "protocol": 7,
        "api": {"base": "^1"}, "contributions": "plugin.toml", "storage_limit": 1024
    });
    let declaration = format!(
        "[plugin]\nid = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\nhost_version = \">=0.1.0\"\n"
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
        ("plugin.toml", declaration.into_bytes()),
        ("README.md", b"# Real status fixture\n".to_vec()),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&zip.finish().unwrap().into_inner()).unwrap()
}

/// Completed public-manager installations populate the recording-worker UI fixture and share its host log sink.
fn with_status(
    cx: &mut TestAppContext,
    count: usize,
    locale: &str,
    dark: bool,
    scenario: impl FnOnce(&mut VisualTestContext, &Entity<EditorApp>, &mut Manager),
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
    let root = workspace.root().join(".runtime-plugin-test");
    let environment = Environment {
        workspace: workspace.root().display().to_string(),
        ..Default::default()
    };
    let mut manager = Manager::open(root.clone(), environment.clone()).unwrap();
    for index in 0..count {
        let package = package(&format!("status-{index:02}"));
        manager.install(&package, Default::default()).unwrap();
    }
    drop(manager);
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, form) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    form.simulate_resize(size(px(1000.), px(760.)));
    form.update(|_, cx| {
        app.update(cx, |app, cx| {
            app.dark_theme = dark;
            apply_theme(builtin_theme(dark), cx);
        });
    });
    let logs = form.update(|_, cx| app.read(cx).extensions.read(cx).runtime_logs());
    let mut manager = Manager::open_with_resources(
        root,
        environment,
        true,
        HostResources {
            logs,
            ..Default::default()
        },
    )
    .unwrap();
    form.update(|_, cx| {
        let panel = app.read(cx).extensions.clone();
        panel.update(cx, |panel, cx| {
            // Test workers record commands without running restore; publish the real install result here.
            // Completing restore removes startup placeholders, matching the production actor's boundary.
            panel.entries = manager.published_entries();
            panel.startup.clear();
            cx.notify();
        });
    });
    draw(form);
    assert_eq!(
        form.update(|_, cx| app.read(cx).extensions.read(cx).entries.len()),
        count
    );
    assert!(form.update(|_, cx| app.read(cx).plugin_indicator(cx).is_none()));
    scenario(form, &app, &mut manager);
    rust_i18n::set_locale(&previous_locale);
}

/// A frame publishes layout, visible-summary acknowledgements, and the resulting reminder change.
fn draw(form: &mut VisualTestContext) {
    form.run_until_parked();
    form.update(|window, cx| window.draw(cx).clear(cx));
    form.run_until_parked();
    form.update(|window, cx| window.draw(cx).clear(cx));
}

/// User-facing assertions activate the real local button instead of a popup helper.
fn click(form: &mut VisualTestContext, selector: &'static str) {
    let bounds = form
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    form.simulate_click(bounds.center(), Default::default());
    draw(form);
}

/// GPUI's debug registry accepts static selectors; these few fixture IDs live for the test process.
fn record_selector(id: u64) -> &'static str {
    Box::leak(format!("plugin-summary-record-{id}").into_boxed_str())
}

/// Host publication invalidates the editor without reaching into the worker's private representation.
fn publish(form: &mut VisualTestContext, app: &Entity<EditorApp>) {
    form.update(|_, cx| app.update(cx, |_, cx| cx.notify()));
    draw(form);
}

/// Error severity stays above a later warning and loading; viewing confirms reminders but reads only summaries.
#[gpui::test]
fn status_popover_confirms_the_round_and_preserves_older_unread_errors(cx: &mut TestAppContext) {
    with_status(cx, 2, "en", false, |form, app, manager| {
        let logs = manager.runtime_logs();
        logs.append(
            "status-00",
            LogLevel::Error,
            "host.operation",
            "Older failure",
        );
        let warning = logs.append(
            "status-00",
            LogLevel::Warning,
            "language.stderr:status-00/analysis",
            "Latest warning",
        );
        logs.append(
            "status-01",
            LogLevel::Info,
            "plugin.stdout",
            "Normal history",
        );
        form.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.extensions.update(cx, |panel, cx| {
                    panel.startup.insert("status-01".into(), "status-01".into());
                    cx.notify();
                });
                cx.notify();
            })
        });
        draw(form);
        // Even host-reported plugin failures retain their package log destination.
        // Publishing the shared runtime sink must not create a host message receipt.
        assert!(form.debug_bounds("host-messages-empty").is_some());
        assert!(form.debug_bounds("host-message-1").is_none());
        assert!(form.debug_bounds("plugin-error-indicator").is_some());
        assert!(form.debug_bounds("plugin-warning-indicator").is_none());
        assert!(form.debug_bounds("plugin-loading-indicator").is_none());
        click(form, "plugin-error-indicator");
        assert!(form.debug_bounds(record_selector(warning)).is_some());
        assert!(form.debug_bounds("plugin-loading-indicator").is_some());
        assert!(form.debug_bounds("plugin-error-indicator").is_none());
        assert_eq!(logs.unread_severity("status-00"), Some(LogLevel::Error));
        assert!(logs.pending_reminders().is_empty());
        // Confirmed old history cannot reopen the bottom reminder; a genuinely new anomaly can.
        logs.append("status-00", LogLevel::Error, "plugin.stderr", "New failure");
        form.update(|_, cx| app.update(cx, |_, cx| cx.notify()));
        draw(form);
        assert!(form.debug_bounds("plugin-error-indicator").is_some());
        assert!(form.debug_bounds(record_selector(warning)).is_some());
    });
}

/// A newly arriving record remains outside the clicked checkpoint and does not replace an open summary.
#[gpui::test]
fn status_popover_does_not_acknowledge_a_record_arriving_after_the_click(cx: &mut TestAppContext) {
    with_status(cx, 1, "en", false, |form, app, manager| {
        let logs = manager.runtime_logs();
        let first = logs.append(
            "status-00",
            LogLevel::Warning,
            "plugin.stderr",
            "First warning",
        );
        publish(form, app);
        let indicator = form.debug_bounds("plugin-warning-indicator").unwrap();
        // Append after actual activation, before explicitly painting the opened card.
        form.simulate_click(indicator.center(), Default::default());
        let later = logs.append(
            "status-00",
            LogLevel::Error,
            "plugin.stderr",
            "Concurrent new error",
        );
        publish(form, app);
        assert!(form.debug_bounds(record_selector(first)).is_some());
        assert!(form.debug_bounds(record_selector(later)).is_none());
        assert!(form.debug_bounds("plugin-error-indicator").is_some());
        assert_eq!(
            logs.pending_reminders(),
            vec![("status-00".into(), LogLevel::Error)]
        );
        form.simulate_keystrokes("escape");
        draw(form);
        assert!(form.debug_bounds("plugin-status-popup").is_none());
        assert_eq!(logs.unread_severity("status-00"), Some(LogLevel::Error));
        assert!(form.debug_bounds("plugin-error-indicator").is_some());
    });
}

/// Layout presence alone is insufficient: an offscreen summary stays unread until a real scroll paints its message.
#[gpui::test]
fn status_popover_marks_only_the_summaries_visible_in_its_scroll_viewport(cx: &mut TestAppContext) {
    with_status(cx, 12, "zh-CN", true, |form, app, manager| {
        let logs = manager.runtime_logs();
        let oldest = logs.append(
            "status-00",
            LogLevel::Error,
            "plugin.stderr",
            "旧的屏幕外异常",
        );
        for index in 1..12 {
            logs.append(
                &format!("status-{index:02}"),
                LogLevel::Warning,
                "language.stderr:analysis",
                format!("警告 {index}：{}", "等待服务恢复。 ".repeat(10)),
            );
        }
        form.simulate_scale_factor_change(1.5);
        publish(form, app);
        click(form, "plugin-error-indicator");
        assert!(logs.pending_reminders().is_empty());
        assert_eq!(logs.unread_severity("status-11"), None);
        assert_eq!(logs.unread_severity("status-00"), Some(LogLevel::Error));
        let header = form.debug_bounds("plugin-status-popup").unwrap();
        let viewport = form.debug_bounds("plugin-status-scroll").unwrap();
        form.simulate_event(ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-99_999.))),
            touch_phase: gpui_kit::TouchPhase::Moved,
            modifiers: Default::default(),
        });
        draw(form);
        let visible = form.debug_bounds(record_selector(oldest)).unwrap();
        assert!(visible.intersects(&viewport));
        assert_eq!(header, form.debug_bounds("plugin-status-popup").unwrap());
        assert_eq!(logs.unread_severity("status-00"), None);
        form.simulate_keystrokes("escape");
        draw(form);
        assert!(
            logs.records("status-00")
                .iter()
                .any(|record| record.id == oldest)
        );
        assert!(form.debug_bounds("plugin-status-popup").is_none());
    });
}

/// A summary resets the native manager's market/search state and affects only its plugin's full-log boundary.
#[gpui::test]
fn status_popover_opens_the_selected_plugins_full_log_and_keeps_other_new_alerts(
    cx: &mut TestAppContext,
) {
    with_status(cx, 2, "en", false, |form, app, manager| {
        let logs = manager.runtime_logs();
        let first = logs.append(
            "status-00",
            LogLevel::Error,
            "plugin.stderr",
            "First failure",
        );
        let second = logs.append(
            "status-01",
            LogLevel::Warning,
            "plugin.stderr",
            "Second warning",
        );
        let editor_window = form.update(|window, _| window.window_handle());
        form.update(|window, cx| app.update(cx, |app, cx| app.toggle_extensions(window, cx)));
        let manager_window = form
            .update(|_, cx| cx.windows())
            .into_iter()
            .find(|window| *window != editor_window)
            .unwrap();
        {
            let dialog = VisualTestContext::from_window(manager_window, form).into_mut();
            draw(dialog);
            click(dialog, "plugin-manager-search");
            dialog.simulate_input("unmatched 插件");
            draw(dialog);
            assert!(dialog.debug_bounds("plugin-row-status-00").is_none());
            click(dialog, "plugin-manager-tabs-1");
        }
        publish(form, app);
        click(form, "plugin-error-indicator");
        let later = logs.append(
            "status-01",
            LogLevel::Warning,
            "plugin.stderr",
            "New second warning",
        );
        publish(form, app);
        // The reason itself, rather than a separate CTA, is part of the native activation target.
        click(
            form,
            Box::leak(format!("plugin-summary-message-{first}").into_boxed_str()),
        );
        assert!(form.update(|_, cx| app.read(cx).plugin_popup.is_none()));
        assert!(form.update(|_, cx| app.read(cx).plugin_popup_snapshot.is_none()));
        {
            let dialog = VisualTestContext::from_window(manager_window, form).into_mut();
            draw(dialog);
            assert!(dialog.debug_bounds("plugin-runtime-log").is_some());
            assert!(dialog.debug_bounds("plugin-readme-region").is_none());
            // Both installed rows return, proving neither a market page nor the old search hides this route.
            assert!(dialog.debug_bounds("plugin-row-status-00").is_some());
            assert!(dialog.debug_bounds("plugin-row-status-01").is_some());
            assert!(
                dialog
                    .debug_bounds(Box::leak(
                        format!("plugin-log-record-{first}").into_boxed_str()
                    ))
                    .is_some()
            );
            assert!(
                dialog
                    .debug_bounds(Box::leak(
                        format!("plugin-log-record-{second}").into_boxed_str()
                    ))
                    .is_none()
            );
        }
        assert_eq!(logs.unread_severity("status-00"), None);
        assert_eq!(logs.unread_severity("status-01"), Some(LogLevel::Warning));
        assert_eq!(
            logs.pending_reminders(),
            vec![("status-01".into(), LogLevel::Warning)]
        );
        assert!(
            logs.records("status-01")
                .iter()
                .any(|record| record.id == later)
        );
    });
}

/// A retained summary survives uninstall, while its now-invalid route cannot replace another manager selection.
#[gpui::test]
fn status_popover_does_not_route_uninstalled_plugin_history(cx: &mut TestAppContext) {
    with_status(cx, 2, "en", false, |form, app, manager| {
        let editor_window = form.update(|window, _| window.window_handle());
        form.update(|window, cx| app.update(cx, |app, cx| app.toggle_extensions(window, cx)));
        let manager_window = form
            .update(|_, cx| cx.windows())
            .into_iter()
            .find(|window| *window != editor_window)
            .unwrap();
        {
            let dialog = VisualTestContext::from_window(manager_window, form).into_mut();
            draw(dialog);
            click(dialog, "plugin-row-status-01");
        }
        let logs = manager.runtime_logs();
        let record = logs.append(
            "status-00",
            LogLevel::Warning,
            "plugin.stderr",
            "Retained warning",
        );
        publish(form, app);
        click(form, "plugin-warning-indicator");
        // Hold the pointer press on the old enabled row while a real public uninstall is published.
        let action = form.debug_bounds("plugin-summary-open-status-00").unwrap();
        form.simulate_mouse_down(action.center(), MouseButton::Left, Default::default());
        manager.uninstall("status-00", false).unwrap();
        let entries = manager.published_entries();
        form.update(|_, cx| {
            app.read(cx).extensions.clone().update(cx, |panel, cx| {
                panel.entries = entries;
                cx.notify();
            })
        });
        form.simulate_mouse_up(action.center(), MouseButton::Left, Default::default());
        draw(form);
        assert!(form.debug_bounds(record_selector(record)).is_some());
        {
            let dialog = VisualTestContext::from_window(manager_window, form).into_mut();
            draw(dialog);
            assert!(dialog.debug_bounds("plugin-readme-region").is_some());
            assert!(dialog.debug_bounds("plugin-runtime-log").is_none());
        }
        assert!(
            logs.records("status-00")
                .iter()
                .any(|saved| saved.id == record)
        );
    });
}

/// Normal LSP loading remains visible and restores the loading entry after an anomaly round is viewed.
#[gpui::test]
fn status_popover_preserves_language_loading_and_keyboard_dismissal(cx: &mut TestAppContext) {
    use crate::app::language_servers::ServiceLoadState;
    with_status(cx, 1, "zh-CN", true, |form, app, manager| {
        let workspace = form.update(|_, cx| app.read(cx).workspace.root().to_owned());
        let (_fixture, plan) = crate::tests::declared_language_service(&workspace);
        let server = Arc::new(language_navigation::LanguageServer::from_service(plan).unwrap());
        form.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.language_servers
                    .insert("fixture".into(), server.clone());
                app.language_service_states
                    .insert("fixture".into(), ServiceLoadState::Loading);
                cx.notify();
            })
        });
        draw(form);
        click(form, "plugin-loading-indicator");
        assert!(form.debug_bounds("plugin-status-popup").is_some());
        form.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.finish_server_loading("fixture", &server, Ok(()), cx)
            })
        });
        draw(form);
        assert!(form.debug_bounds("plugin-loading-indicator").is_none());
        assert!(form.debug_bounds("plugin-status-popup").is_none());
        assert!(form.update(|_, cx| app.read(cx).plugin_popup_snapshot.is_none()));
        form.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.language_service_states
                    .insert("fixture".into(), ServiceLoadState::Loading);
                cx.notify();
            })
        });
        let logs = manager.runtime_logs();
        logs.append(
            "status-00",
            LogLevel::Warning,
            "plugin.stderr",
            "语言服务仍在准备中",
        );
        publish(form, app);
        assert!(form.debug_bounds("plugin-warning-indicator").is_some());
        assert!(form.debug_bounds("plugin-loading-indicator").is_none());
        click(form, "plugin-warning-indicator");
        assert!(form.debug_bounds("plugin-loading-indicator").is_some());
        assert!(form.update(|window, cx| {
            app.read(cx)
                .plugin_popup_snapshot
                .as_ref()
                .unwrap()
                .state
                .read(cx)
                .focus_handle(cx)
                .contains_focused(window, cx)
        }));
        form.simulate_keystrokes("tab escape");
        draw(form);
        assert!(form.debug_bounds("plugin-status-popup").is_none());
        assert!(form.debug_bounds("plugin-loading-indicator").is_some());
        assert_eq!(logs.unread_severity("status-00"), None);
    });
}

/// A new alert can recreate the same kind of entry, but Escape must not return focus to its retired predecessor.
#[gpui::test]
fn status_popover_returns_to_editor_after_same_severity_entry_is_recreated(
    cx: &mut TestAppContext,
) {
    with_status(cx, 1, "zh-CN", false, |form, app, manager| {
        let document = form.update(|_, cx| app.read(cx).workspace.root().join("focus.txt"));
        std::fs::write(&document, "").unwrap();
        form.update(|window, cx| app.update(cx, |app, cx| app.open_file(document, window, cx)));
        draw(form);
        let logs = manager.runtime_logs();
        logs.append(
            "status-00",
            LogLevel::Error,
            "plugin.stderr",
            "First failure",
        );
        publish(form, app);
        click(form, "plugin-error-indicator");
        // Let an actual frame remove the confirmed entry before a same-severity arrival rebuilds it.
        assert!(form.debug_bounds("plugin-error-indicator").is_none());
        logs.append(
            "status-00",
            LogLevel::Error,
            "plugin.stderr",
            "Later failure",
        );
        publish(form, app);
        assert!(form.debug_bounds("plugin-error-indicator").is_some());
        form.simulate_keystrokes("escape");
        draw(form);
        assert!(form.debug_bounds("plugin-status-popup").is_none());
        form.simulate_input("继续编辑");
        draw(form);
        assert_eq!(
            form.update(|_, cx| app.read(cx).editor.read(cx).text().to_string()),
            "继续编辑"
        );
        assert_eq!(
            logs.pending_reminders(),
            vec![("status-00".into(), LogLevel::Error)]
        );
    });
}

/// Native Tab traversal must bring each focused card into the painted viewport without a wheel event.
#[gpui::test]
fn status_popover_scrolls_every_keyboard_focused_summary_into_view(cx: &mut TestAppContext) {
    with_status(cx, 12, "zh-CN", true, |form, app, manager| {
        let logs = manager.runtime_logs();
        for index in 0..12 {
            logs.append(
                &format!("status-{index:02}"),
                LogLevel::Warning,
                "plugin.stderr",
                format!("可用键盘查看的警告 {index}"),
            );
        }
        form.simulate_scale_factor_change(1.5);
        // The test platform opens inactive windows; native focus-in observers require activation.
        form.update(|window, _| window.activate_window());
        publish(form, app);
        assert!(form.update(|window, _| window.is_window_active()));
        click(form, "plugin-warning-indicator");
        let mut visited = std::collections::BTreeSet::new();
        // The fixed header has one close button, followed by all 12 summary buttons in the native tab group.
        for _ in 0..13 {
            form.simulate_keystrokes("tab");
            draw(form);
            let focused = form.update(|window, cx| {
                app.read(cx)
                    .plugin_popup_snapshot
                    .as_ref()
                    .unwrap()
                    .row_focus
                    .iter()
                    .find(|(_, row)| row.handle.is_focused(window))
                    .map(|(plugin, _)| plugin.clone())
            });
            if let Some(plugin) = focused {
                // GPUI requires process-lived debug selector strings even for dynamic fixture IDs.
                let selector = Box::leak(format!("plugin-summary-open-{plugin}").into_boxed_str());
                let row = form.debug_bounds(selector).unwrap();
                let viewport = form.debug_bounds("plugin-status-scroll").unwrap();
                assert!(
                    row.intersects(&viewport),
                    "focused {plugin} remains outside the viewport"
                );
                // At 1.5 scaling, f32 edge addition can differ by 0.000061px despite aligned device pixels.
                // This arithmetic-only tolerance still rejects visibly clipped cards.
                let epsilon = px(0.01);
                assert!(
                    row.top() + epsilon >= viewport.top()
                        && row.bottom() <= viewport.bottom() + epsilon,
                    "focused {plugin} is clipped: row={row:?}, viewport={viewport:?}, top_delta={:?}, bottom_delta={:?}",
                    row.top() - viewport.top(),
                    viewport.bottom() - row.bottom()
                );
                visited.insert(plugin);
            }
        }
        assert_eq!(visited.len(), 12);
        assert!(visited.contains("status-00"));
        assert_eq!(logs.unread_severity("status-00"), None);
    });
}

/// Bounded ordinary history may evict an error, but its pending reminder and captured reason survive until viewed.
#[gpui::test]
fn status_popover_retains_an_evicted_errors_reminder_and_snapshot_reason(cx: &mut TestAppContext) {
    with_status(cx, 1, "en", false, |form, app, manager| {
        let logs = manager.runtime_logs();
        let error = logs.append(
            "status-00",
            LogLevel::Error,
            "plugin.stderr",
            "Failure before the ordinary history burst",
        );
        for index in 0..512 {
            logs.append(
                "status-00",
                LogLevel::Info,
                "plugin.stdout",
                format!("history {index}"),
            );
        }
        assert!(
            logs.records("status-00")
                .iter()
                .all(|record| record.id != error)
        );
        // The log-tab badge describes retained unread records, independently of the persistent bottom reminder.
        assert_eq!(logs.unread_severity("status-00"), None);
        assert_eq!(
            logs.pending_reminders(),
            vec![("status-00".into(), LogLevel::Error)]
        );
        publish(form, app);
        click(form, "plugin-error-indicator");
        assert!(form.debug_bounds(record_selector(error)).is_some());
        assert!(form.debug_bounds("plugin-error-indicator").is_none());
        assert!(logs.pending_reminders().is_empty());
        let later = logs.append(
            "status-00",
            LogLevel::Warning,
            "plugin.stderr",
            "Warning after the captured boundary",
        );
        publish(form, app);
        assert!(form.debug_bounds("plugin-warning-indicator").is_some());
        assert!(form.debug_bounds(record_selector(error)).is_some());
        assert!(form.debug_bounds(record_selector(later)).is_none());
        assert_eq!(logs.unread_severity("status-00"), Some(LogLevel::Warning));
        form.simulate_keystrokes("escape");
        draw(form);
        click(form, "plugin-warning-indicator");
        assert!(form.debug_bounds(record_selector(later)).is_some());
        assert!(form.debug_bounds(record_selector(error)).is_none());
        assert!(logs.pending_reminders().is_empty());
        assert_eq!(logs.unread_severity("status-00"), None);
    });
}

/// An installed, interactive summary must wrap long reasons while keeping its time inside the native card.
#[gpui::test]
fn status_popover_wraps_installed_summary_content_and_keeps_its_time_visible(
    cx: &mut TestAppContext,
) {
    with_status(cx, 1, "en", false, |form, app, manager| {
        let record = manager.runtime_logs().append(
            "status-00",
            LogLevel::Error,
            "lsp/status-00/analysis",
            "The selected language tool is unavailable. Please check the configured executable path. 原因应完整显示。 ".repeat(16),
        );
        form.simulate_scale_factor_change(1.5);
        publish(form, app);
        click(form, "plugin-error-indicator");
        let popup = form.debug_bounds("plugin-status-popup").unwrap();
        // These dynamic selectors are retained for GPUI's static-only test debug registry.
        let message = form
            .debug_bounds(Box::leak(
                format!("plugin-summary-message-{record}").into_boxed_str(),
            ))
            .unwrap();
        let time = form
            .debug_bounds(Box::leak(
                format!("plugin-summary-time-{record}").into_boxed_str(),
            ))
            .unwrap();
        assert!(
            message.size.height > px(36.),
            "a long reason must occupy multiple lines: {message:?}"
        );
        assert!(message.size.width < popup.size.width);
        assert!(
            time.left() >= popup.left() && time.right() <= popup.right(),
            "time must remain inside the card: {time:?} / {popup:?}"
        );
        assert!(time.top() >= popup.top() && time.bottom() <= popup.bottom());
        let viewport = form.debug_bounds("plugin-status-scroll").unwrap();
        let visible = message.intersect(&viewport);
        assert!(visible.size.width > px(0.) && visible.size.height > px(0.));
        // Activate painted body text, proving the constrained content still belongs to the installed Base button.
        form.simulate_click(visible.center(), Default::default());
        draw(form);
        assert!(form.update(|_, cx| app.read(cx).plugin_popup.is_none()));
        let editor_window = form.update(|window, _| window.window_handle());
        let manager_window = form
            .update(|_, cx| cx.windows())
            .into_iter()
            .find(|window| *window != editor_window)
            .unwrap();
        let dialog = VisualTestContext::from_window(manager_window, form).into_mut();
        draw(dialog);
        assert!(dialog.debug_bounds("plugin-runtime-log").is_some());
    });
}
