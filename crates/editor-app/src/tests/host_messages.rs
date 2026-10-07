//! Host message acceptance uses the same window, file operations and native input as the editor.

use crate::*;
use gpui_kit::{TestAppContext, VisualTestContext, gpui};
use std::cell::RefCell;

/// Keep the temporary workspace alive while a scenario exercises the actual shell and document routes.
fn with_messages(
    cx: &mut TestAppContext,
    scenario: impl FnOnce(&mut VisualTestContext, Entity<EditorApp>, PathBuf),
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().to_path_buf();
    let workspace = Workspace::open(&root).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, form) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    form.simulate_resize(size(px(1100.), px(800.)));
    draw(form);
    scenario(form, slot.borrow_mut().take().unwrap(), root);
}

/// Flush deferred source notifications and then inspect the resulting native layout.
fn draw(form: &mut VisualTestContext) {
    form.run_until_parked();
    form.update(|window, cx| window.draw(cx).clear(cx));
}

/// Activate the production control by its rendered bounds rather than calling its handler.
fn click(form: &mut VisualTestContext, selector: &'static str) {
    let bounds = form
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    form.simulate_click(bounds.center(), Default::default());
    draw(form);
}

/// GPUI's inspection registry needs static names; a small acceptance fixture keeps these for the process.
fn row(id: u64) -> &'static str {
    Box::leak(format!("host-message-{id}").into_boxed_str())
}

/// Native button clicks occur on key release; a key-down-only shortcut is not a complete activation.
fn activate(form: &mut VisualTestContext, key: &str) {
    let keystroke = gpui_kit::Keystroke::parse(key).unwrap();
    form.simulate_event(gpui_kit::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    form.simulate_event(gpui_kit::KeyUpEvent { keystroke });
    draw(form);
}

/// Controlled receipts use the same publication boundary as real operations, not a test-only queue.
fn publish(form: &mut VisualTestContext, app: &Entity<EditorApp>, start: u64, end: u64) {
    form.update(|_, cx| {
        app.update(cx, |app, cx| {
            for index in start..=end {
                let level = match index % 3 {
                    0 => app::messages::MessageLevel::Error,
                    1 => app::messages::MessageLevel::Info,
                    _ => app::messages::MessageLevel::Warning,
                };
                app.record_host_message(level, format!("宿主消息 Host receipt {index}"), cx);
            }
        })
    });
    draw(form);
}

/// One list grows by twenty, caps all severities together, and preserves deterministic newest-first order.
#[gpui::test]
fn host_messages_progressive_history_keeps_the_latest_five_hundred(cx: &mut TestAppContext) {
    with_messages(cx, |form, app, _| {
        // A tall viewport makes the disclosed rows directly observable without depending on storage internals.
        form.simulate_resize(size(px(1100.), px(40000.)));
        draw(form);
        assert!(form.debug_bounds("host-messages-empty").is_some());
        publish(form, &app, 1, 1);
        assert!(form.debug_bounds(row(1)).is_some());
        assert!(form.debug_bounds("host-messages-more").is_none());
        publish(form, &app, 2, 20);
        assert!(form.debug_bounds(row(1)).is_some());
        assert!(form.debug_bounds("host-messages-more").is_none());
        publish(form, &app, 21, 21);
        assert!(form.debug_bounds(row(1)).is_none());
        assert!(form.debug_bounds(row(2)).is_some());
        assert!(form.debug_bounds("host-messages-more").is_some());
        publish(form, &app, 22, 40);
        assert!(form.debug_bounds(row(20)).is_none());
        assert!(form.debug_bounds(row(21)).is_some());
        publish(form, &app, 41, 41);
        click(form, "host-messages-more");
        assert!(form.debug_bounds(row(1)).is_none());
        assert!(form.debug_bounds(row(2)).is_some());
        click(form, "host-messages-more");
        assert!(form.debug_bounds(row(1)).is_some());
        assert!(form.debug_bounds("host-messages-more").is_none());
        publish(form, &app, 42, 501);
        assert!(
            form.debug_bounds(row(1)).is_none(),
            "oldest receipt is evicted at capacity"
        );
        while form.debug_bounds("host-messages-more").is_some() {
            click(form, "host-messages-more");
        }
        for id in 2..=501 {
            assert!(
                form.debug_bounds(row(id)).is_some(),
                "retained receipt {id} is accessible"
            );
            if id > 2 {
                assert!(
                    form.debug_bounds(row(id)).unwrap().top()
                        < form.debug_bounds(row(id - 1)).unwrap().top()
                );
            }
        }
        publish(form, &app, 502, 502);
        assert!(form.debug_bounds(row(2)).is_none());
        assert!(form.debug_bounds(row(3)).is_some());
        assert!(form.debug_bounds(row(502)).is_some());
        assert!(form.debug_bounds("host-messages-more").is_none());
        form.simulate_resize(size(px(1100.), px(800.)));
        draw(form);
        let panel = form.debug_bounds("host-messages-panel").unwrap();
        let before = form.debug_bounds(row(502)).unwrap().top();
        form.simulate_event(gpui_kit::ScrollWheelEvent {
            position: panel.center(),
            delta: gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-400.))),
            touch_phase: gpui_kit::TouchPhase::Moved,
            modifiers: Default::default(),
        });
        draw(form);
        assert!(
            form.debug_bounds(row(502))
                .is_none_or(|bounds| bounds.top() < before),
            "the native viewport scrolls"
        );
    });
}

/// A real save retains its result, while a fresh app restores native layout choices with an empty history.
#[gpui::test]
fn host_messages_restore_hidden_width_without_restoring_history(cx: &mut TestAppContext) {
    with_messages(cx, |form, app, root| {
        let path = root.join("document.txt");
        std::fs::write(&path, "original").unwrap();
        form.update(|window, cx| app.update(cx, |app, cx| app.open_file(path.clone(), window, cx)));
        draw(form);
        assert!(form.debug_bounds("host-message-info").is_some());
        form.simulate_input("changed");
        draw(form);
        assert!(
            form.debug_bounds(row(2)).is_none(),
            "typing is not a receipt"
        );
        form.update(|_, cx| app.update(cx, |app, cx| app.save_current(cx)));
        draw(form);
        assert!(std::fs::read_to_string(&path).unwrap().contains("changed"));
        assert!(
            form.debug_bounds(row(2)).is_some(),
            "successful native save appears"
        );
        let panel = form.debug_bounds("host-messages-panel").unwrap();
        let edge = point(panel.left() + px(1.), panel.center().y);
        let target = edge - point(px(75.), px(0.));
        form.simulate_mouse_down(edge, MouseButton::Left, Default::default());
        form.simulate_mouse_move(
            edge - point(px(10.), px(0.)),
            MouseButton::Left,
            Default::default(),
        );
        form.simulate_mouse_move(target, MouseButton::Left, Default::default());
        form.simulate_mouse_up(target, MouseButton::Left, Default::default());
        draw(form);
        let width = form.debug_bounds("host-messages-panel").unwrap().size.width;
        assert!(
            width > panel.size.width + px(50.),
            "the actual dock edge resizes messages"
        );
        click(form, "host-messages-hide");
        let workspace = Workspace::open(&root).unwrap();
        let slot = Rc::new(RefCell::new(None));
        let capture = slot.clone();
        let (_, restored) = form.add_window_view(move |window, cx| {
            let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
            *capture.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let restored_app = slot.borrow_mut().take().unwrap();
        restored.simulate_resize(size(px(1100.), px(800.)));
        draw(restored);
        assert!(restored.debug_bounds("host-messages-panel").is_none());
        click(restored, "host-messages-toggle");
        let panel = restored.debug_bounds("host-messages-panel").unwrap();
        assert!((panel.size.width - width).abs() < px(2.));
        assert!(restored.debug_bounds("host-messages-empty").is_some());
        assert!(restored.debug_bounds(row(1)).is_none());
        assert!(restored.debug_bounds("host-messages-more").is_none());
        // Fresh host results still publish after restoration; saved tabs did not create duplicate receipts.
        publish(restored, &restored_app, 1, 1);
        assert!(restored.debug_bounds(row(1)).is_some());
    });
}

/// An older restricted workspace keeps a moved peer when the new host panel joins its right dock.
#[gpui::test]
fn host_messages_extend_legacy_layout_and_support_native_keyboard(cx: &mut TestAppContext) {
    use gpui_base::dock::{DockPlacement, PanelId};
    with_messages(cx, |form, app, root| {
        form.update(|window, cx| {
            app.update(cx, |app, cx| {
                let message = app.messages.clone();
                let explorer = dock::panel_handle(app.explorer_panel.clone());
                app.dock_area.update(cx, |area, cx| {
                    // Build a real pre-feature layout with a right-hand peer instead of hand-writing tree internals.
                    area.remove_panel(message, window, cx);
                    area.add_panel_view(explorer, DockPlacement::Right, Some(px(350.)), window, cx);
                    area.set_dock_size(DockPlacement::Right, px(350.), window, cx);
                });
                app.session_state.workspace_trusted = false;
                app.capture_dock_layout(cx);
                app.persist_session();
            })
        });
        let mut legacy = serde_json::to_value(SessionState::load(&root)).unwrap();
        legacy.as_object_mut().unwrap().remove("messages_visible");
        let legacy: SessionState = serde_json::from_value(legacy).unwrap();
        legacy.save();
        let workspace = Workspace::open(&root).unwrap();
        let slot = Rc::new(RefCell::new(None));
        let capture = slot.clone();
        let (_, restored) = form.add_window_view(move |window, cx| {
            let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
            *capture.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = slot.borrow_mut().take().unwrap();
        restored.simulate_resize(size(px(1100.), px(800.)));
        draw(restored);
        let panel = restored.debug_bounds("host-messages-panel").unwrap();
        let peer_selector = restored.update(|_, cx| {
            let area = app.read(cx).dock_area.read(cx);
            let id = PanelId::from(app.read(cx).explorer_panel.entity_id());
            let node = area
                .layout(DockPlacement::Right)
                .unwrap()
                .find_panel_node(id)
                .unwrap();
            Box::leak(format!("local-dock-content-{}", node.as_u64()).into_boxed_str())
                as &'static str
        });
        let peer = restored.debug_bounds(peer_selector).unwrap();
        assert!(
            peer.bottom() <= panel.top(),
            "the existing peer keeps its independent split"
        );
        assert!((panel.size.width - px(350.)).abs() < px(2.));
        // A restored closed dock still contains a visible panel identity; the bottom action must reopen it once.
        restored.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.dock_area.update(cx, |area, cx| {
                    area.toggle_dock(DockPlacement::Right, window, cx)
                });
            })
        });
        draw(restored);
        assert!(restored.debug_bounds("host-messages-panel").is_none());
        click(restored, "host-messages-toggle");
        assert!(
            restored.debug_bounds("host-messages-panel").is_some(),
            "one bottom activation reopens a closed region"
        );
        assert!(restored.debug_bounds(peer_selector).is_some());
        click(restored, "host-messages-hide");
        assert!(
            restored.debug_bounds(peer_selector).is_some(),
            "hiding messages leaves its peer visible"
        );
        // Keyboard activation uses the local Base button's actual focus handle.
        click(restored, "host-messages-toggle");
        activate(restored, "enter");
        assert!(restored.debug_bounds("host-messages-panel").is_none());
        activate(restored, "space");
        assert!(restored.debug_bounds("host-messages-panel").is_some());
        let previous_locale = rust_i18n::locale().to_string();
        rust_i18n::set_locale("en");
        restored.update(|window, cx| {
            typography::set_font_size(cx, 20.);
            apply_theme(builtin_theme(true), cx);
            window.refresh();
        });
        restored.simulate_scale_factor_change(1.5);
        draw(restored);
        assert!(restored.debug_bounds("host-messages-empty").is_some());
        publish(restored, &app, 1, 1);
        assert!(restored.debug_bounds(row(1)).is_some());
        rust_i18n::set_locale(&previous_locale);
    });
}

/// A real failed document open is retained, and hiding the panel returns width to the editor.
#[gpui::test]
fn host_messages_record_file_failure_and_reopen_without_losing_history(cx: &mut TestAppContext) {
    with_messages(cx, |form, app, root| {
        let panel = form
            .debug_bounds("host-messages-panel")
            .expect("messages default to the right");
        let editor = form.debug_bounds("editor-panel-content").unwrap();
        assert!(panel.left() >= editor.right());
        form.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_file(root.join("missing.txt"), window, cx)
            })
        });
        draw(form);
        assert!(
            form.debug_bounds("host-message-error").is_some(),
            "actual open failure appears in the history"
        );
        let hide = form.debug_bounds("host-messages-hide").unwrap();
        form.simulate_click(hide.center(), Default::default());
        draw(form);
        assert!(form.debug_bounds("host-messages-panel").is_none());
        assert!(
            form.debug_bounds("editor-panel-content")
                .unwrap()
                .size
                .width
                > editor.size.width
        );
        let button = form.debug_bounds("host-messages-toggle").unwrap();
        form.simulate_click(button.center(), Default::default());
        draw(form);
        assert!(
            SessionState::load(&root).messages_visible,
            "bottom button must persist the reopened choice"
        );
        assert!(
            form.debug_bounds("host-messages-panel").is_some(),
            "the bottom button must reopen the panel at {button:?}"
        );
        assert!(form.debug_bounds("host-message-error").is_some());
    });
}
