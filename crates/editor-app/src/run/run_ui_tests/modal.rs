//! Real modal callbacks must open/close outside the borrowed editor/window update.
use super::*;

/// The simplified form has a sidebar, three main fields and a footer outside the field scroller.
/// This uses the real modal so a full-height wrapper cannot hide unused space below its footer.
#[gpui::test]
fn simplified_modal_keeps_sidebar_fields_and_footer_in_their_layout_regions(
    cx: &mut TestAppContext,
) {
    let root = tempfile::tempdir().unwrap();
    let (app, cx) = open_editor(cx, root.path());
    cx.update(|window, cx| {
        app.update(cx, |state, cx| {
            state.open_run_config_dialog(window, cx, None)
        })
    });
    use_run_dialog(cx, &app);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let form = cx.debug_bounds("run-config-form").unwrap();
    let save = cx.debug_bounds("run-config-save").unwrap();
    assert!(
        form.bottom() - save.bottom() <= px(28.),
        "save actions belong at the bottom of the card: form={form:?}, save={save:?}"
    );
    let sidebar = cx.debug_bounds("run-config-sidebar").unwrap();
    // Discovery is an icon in the same header as Add, rather than a footer row.
    let discover = cx.debug_bounds("run-config-discover").unwrap();
    let add = cx.debug_bounds("run-config-new").unwrap();
    assert!(discover.right() <= add.left());
    assert_eq!(discover.center().y, add.center().y);
    assert!(discover.top() >= sidebar.top());
    let page = cx.debug_bounds("run-config-page").unwrap();
    assert!(
        sidebar.right() <= page.left() && sidebar.size.width >= px(180.),
        "configuration list and fields occupy separate columns: {sidebar:?}, {page:?}"
    );
    for selector in [
        "run-config-name",
        "run-config-target-picker",
        "run-config-arguments-edit",
        "run-config-startup",
        "run-config-more",
    ] {
        let bounds = cx.debug_bounds(selector).unwrap();
        assert!(bounds.left() >= page.left() && bounds.right() <= page.right());
    }
    assert!(cx.debug_bounds("run-config-directory").is_none());
    assert!(cx.debug_bounds("run-config-environment-edit").is_none());
}

/// The title bar reserves text for the selected name; execution actions use compact icon hit targets.
#[gpui::test]
fn b1_titlebar_actions_are_compact_icons(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (_, cx) = open_editor(cx, root.path());
    for action in ["run-build", "run-start", "run-debug", "run-stop"] {
        let bounds = cx.debug_bounds(action).unwrap();
        assert!(
            bounds.size.width <= bounds.size.height + px(4.),
            "{action} must fit an icon button rather than a text label: {bounds:?}"
        );
    }
    assert!(
        cx.debug_bounds("run-rerun").is_none(),
        "rerun is an explicit menu action, keeping the idle title bar to four icons"
    );
    assert!(
        cx.debug_bounds("run-terminate").is_none(),
        "immediate termination is offered when stopping, not as an extra idle title-bar action"
    );
}

/// Drive the actual button hit region; pointer hover and click use the same native input seam.
fn click_form_control(cx: &mut gpui_kit::VisualTestContext, selector: &'static str) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let position = cx.debug_bounds(selector).unwrap().center();
    cx.simulate_event(gpui::MouseMoveEvent {
        position,
        pressed_button: None,
        modifiers: Default::default(),
    });
    cx.run_until_parked();
    cx.simulate_click(position, Default::default());
    cx.run_until_parked();
    use_live_window(cx);
}

/// Native keyboard activation opens optional fields without replacing retained input values.
#[gpui::test]
fn disclosures_accept_keyboard_activation_without_losing_fields(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (app, cx) = open_editor(cx, root.path());
    cx.update(|window, cx| {
        app.update(cx, |state, cx| {
            state.open_run_config_dialog(window, cx, None)
        })
    });
    use_run_dialog(cx, &app);
    cx.update(|window, cx| {
        let form = app.read(cx).run_form.as_ref().unwrap().clone();
        form.read(cx)
            .field_input(crate::run::RunField::Name)
            .unwrap()
            .update(cx, |input, cx| input.set_value("保留我的配置", window, cx));
    });
    click_form_control(cx, "run-config-more");
    cx.run_until_parked();
    assert!(cx.debug_bounds("run-config-directory").is_some());
    // Buttons consume key down/up activation; character-input simulation would insert a space.
    let keystroke = gpui::Keystroke::parse("space").unwrap();
    cx.simulate_event(gpui::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.simulate_event(gpui::KeyUpEvent { keystroke });
    cx.run_until_parked();
    assert!(cx.debug_bounds("run-config-directory").is_none());
    let keystroke = gpui::Keystroke::parse("enter").unwrap();
    cx.simulate_event(gpui::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.simulate_event(gpui::KeyUpEvent { keystroke });
    cx.run_until_parked();
    assert!(cx.debug_bounds("run-config-directory").is_some());
    assert_eq!(
        cx.update(|_, cx| {
            app.read(cx)
                .run_form
                .as_ref()
                .unwrap()
                .read(cx)
                .field_input(crate::run::RunField::Name)
                .unwrap()
                .read(cx)
                .value()
                .to_string()
        }),
        "保留我的配置"
    );
}

/// Sidebar selection loads the chosen inputs; the scope picker has native keyboard dismissal.
#[gpui::test]
fn sidebar_and_destination_picker_work_inside_the_modal(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (app, cx) = open_editor(cx, root.path());
    cx.update(|window, cx| {
        app.update(cx, |state, cx| {
            let key = state.workspace_key();
            state.run_controls = crate::run::RunControls::load_with_project(
                &key,
                Some(root.path().join("local")),
                Some(root.path().into()),
            );
            for (id, name) in [("first", "第一项"), ("second", "第二项")] {
                let mut draft = crate::run::RunConfigDraft::from_config(None, id.into());
                draft.name = name.into();
                draft.program = "fixture.exe".into();
                state
                    .run_controls
                    .upsert(draft.to_config().unwrap(), &key)
                    .unwrap();
            }
            state.open_run_config_dialog(window, cx, Some("first".into()));
        });
    });
    use_run_dialog(cx, &app);
    click_form_control(cx, "run-config-existing-second");
    assert_eq!(
        cx.update(|_, cx| {
            app.read(cx)
                .run_form
                .as_ref()
                .unwrap()
                .read(cx)
                .field_input(crate::run::RunField::Name)
                .unwrap()
                .read(cx)
                .value()
                .to_string()
        }),
        "第二项"
    );
    click_form_control(cx, "run-config-save-location");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(
        cx.update(|_, cx| app.read(cx).run_form.is_some()),
        "Escape dismisses the picker, retaining the modal"
    );
    click_form_control(cx, "run-config-save-location");
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    assert!(!cx.update(|_, cx| {
        app.read(cx)
            .run_form
            .as_ref()
            .unwrap()
            .read(cx)
            .destination_is_local()
    }));
    assert!(
        cx.update(|_, cx| app.read(cx).run_controls.sessions().is_empty()),
        "editing never launches a program"
    );
}

/// A constrained viewport keeps save/cancel outside the field scroller in both themes and locales.
#[gpui::test]
fn b1_footer_stays_visible_at_narrow_sizes_and_zoom(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (app, cx) = open_editor(cx, root.path());
    cx.simulate_resize(size(px(520.), px(420.)));
    for locale in ["en", "zh-CN"] {
        rust_i18n::set_locale(locale);
        for dark in [false, true] {
            cx.update(|window, cx| {
                apply_theme(builtin_theme(dark), cx);
                crate::ui::typography::set_font_size(cx, 24.);
                crate::ui::theme::sync_font_sizes(cx);
                app.update(cx, |state, cx| {
                    state.open_run_config_dialog(window, cx, None)
                });
            });
            use_run_dialog(cx, &app);
            cx.simulate_resize(size(px(520.), px(420.)));
            cx.update(|window, cx| window.draw(cx).clear(cx));
            let form = cx.debug_bounds("run-config-form").unwrap();
            let page = cx.debug_bounds("run-config-page").unwrap();
            // The compact layout keeps discovery available in the header in both languages/themes.
            let discover = cx.debug_bounds("run-config-discover").unwrap();
            let add = cx.debug_bounds("run-config-new").unwrap();
            assert!(discover.right() <= add.left());
            assert_eq!(discover.center().y, add.center().y);
            for selector in [
                "run-config-save",
                "run-config-cancel",
                "run-config-save-location",
            ] {
                let bounds = cx.debug_bounds(selector).unwrap();
                assert!(
                    bounds.left() >= form.left()
                        && bounds.right() <= form.right()
                        && bounds.bottom() <= form.bottom()
                        && bounds.top() >= page.bottom(),
                    "{locale}, dark={dark}: {selector} stays reachable outside the scroller: {bounds:?}, form={form:?}, page={page:?}"
                );
            }
        }
    }
    rust_i18n::set_locale("en");
}

/// Save and Cancel close the owned native window, preserving the editor and allowing a fresh draft.
#[gpui::test]
fn real_run_configuration_modal_opens_and_closes_after_callbacks(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (app, cx) = open_editor(cx, root.path());
    // Fixture-local storage prevents this interaction check from changing a user configuration.
    cx.update(|window, cx| {
        app.update(cx, |state, cx| {
            let key = state.workspace_key();
            state.run_controls = crate::run::RunControls::load_with_project(
                &key,
                Some(root.path().join("local")),
                Some(root.path().into()),
            );
            state.open_run_config_dialog(window, cx, None);
        })
    });
    use_run_dialog(cx, &app);
    assert_eq!(cx.update(|_, cx| cx.windows().len()), 2);
    assert!(cx.debug_bounds("run-config-form").is_some());
    let owner = app.clone();
    cx.update(move |_, cx| owner.update(cx, |state, cx| state.close_run_form(cx)));
    use_live_window(cx);
    assert!(cx.update(|_, cx| app.read(cx).run_form.is_none()));
    cx.run_until_parked();
    use_live_window(cx);
    assert_eq!(cx.update(|_, cx| cx.windows().len()), 1);
    // A subsequent dialog must retain a fresh form rather than an abandoned deferred callback.
    cx.update(|window, cx| {
        app.update(cx, |state, cx| {
            state.open_run_config_dialog(window, cx, None)
        })
    });
    use_run_dialog(cx, &app);
    let owner = app.clone();
    cx.update(move |window, cx| {
        owner.update(cx, |state, cx| {
            let form = state.run_form.as_ref().unwrap().clone();
            form.update(cx, |form, cx| {
                form.field_input(crate::run::RunField::Name)
                    .unwrap()
                    .update(cx, |input, cx| {
                        input.set_value("Native modal fixture", window, cx)
                    });
                form.field_input(crate::run::RunField::Program)
                    .unwrap()
                    .update(cx, |input, cx| {
                        input.set_value("powershell.exe", window, cx)
                    });
            });
            state.commit_run_form(window, cx);
        })
    });
    cx.run_until_parked();
    use_live_window(cx);
    assert_eq!(cx.update(|_, cx| cx.windows().len()), 1);
    assert_eq!(
        cx.update(|_, cx| app.read(cx).run_controls.selected().unwrap().name.clone()),
        "Native modal fixture"
    );
}

/// Reopening activates one native window without replacing edited fields; native chrome cancels inner edits.
#[gpui::test]
fn native_configuration_window_reuses_its_draft_and_closes_through_shared_chrome(
    cx: &mut TestAppContext,
) {
    let root = tempfile::tempdir().unwrap();
    let (app, cx) = open_editor(cx, root.path());
    cx.update(|window, cx| {
        app.update(cx, |state, cx| {
            state.open_run_config_dialog(window, cx, None)
        })
    });
    use_run_dialog(cx, &app);
    let handle = cx.update(|_, cx| app.read(cx).run_dialog_window.unwrap());
    assert!(cx.debug_bounds("app-dialog-title-bar").is_some());
    click_form_control(cx, "run-config-name");
    cx.simulate_input("Preserve the draft");
    cx.update(|window, cx| {
        app.update(cx, |state, cx| {
            state.open_run_config_dialog(window, cx, None)
        })
    });
    assert_eq!(cx.update(|_, cx| cx.windows().len()), 2);
    assert_eq!(
        cx.update(|_, cx| app.read(cx).run_dialog_window.unwrap().window_id()),
        handle.window_id()
    );
    assert_eq!(
        cx.update(|_, cx| app
            .read(cx)
            .run_form
            .as_ref()
            .unwrap()
            .read(cx)
            .field_input(crate::run::RunField::Name)
            .unwrap()
            .read(cx)
            .value()
            .to_string()),
        "Preserve the draft"
    );
    click_form_control(cx, "run-config-arguments-edit");
    click_form_control(cx, "run-config-arguments");
    cx.simulate_input("discard detail");
    click_form_control(cx, "app-dialog-close");
    assert!(cx.debug_bounds("run-config-arguments-edit").is_some());
    assert_eq!(cx.update(|_, cx| cx.windows().len()), 2);
    click_form_control(cx, "app-dialog-close");
    assert_eq!(cx.update(|_, cx| cx.windows().len()), 1);
    assert!(cx.update(
        |_, cx| app.read(cx).run_form.is_none() && app.read(cx).run_dialog_window.is_none()
    ));
}

/// Platform close is vetoed for a detail edit, then native close receipts release the whole draft.
#[gpui::test]
fn native_configuration_close_request_preserves_inner_cancel_and_cleans_on_closed(
    cx: &mut TestAppContext,
) {
    let root = tempfile::tempdir().unwrap();
    let (app, cx) = open_editor(cx, root.path());
    cx.update(|window, cx| {
        app.update(cx, |state, cx| {
            state.open_run_config_dialog(window, cx, None)
        })
    });
    use_run_dialog(cx, &app);
    click_form_control(cx, "run-config-arguments-edit");
    assert!(!cx.simulate_close());
    cx.run_until_parked();
    assert!(cx.debug_bounds("run-config-arguments-edit").is_some());
    assert!(cx.simulate_close());
    // The platform, rather than its should-close callback, owns actual native window removal.
    cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
    use_live_window(cx);
    assert_eq!(cx.update(|_, cx| cx.windows().len()), 1);
    assert!(cx.update(
        |_, cx| app.read(cx).run_form.is_none() && app.read(cx).run_dialog_window.is_none()
    ));
}

/// A source reveal and a physical panel click must preserve the panel's next keyboard step.
#[gpui::test]
fn debug_panel_keeps_keyboard_focus_after_source_navigation(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("probe.rs"),
        "//! Keyboard focus fixture.\nfn main() {}\n",
    )
    .unwrap();
    let (app, cx) = open_editor(cx, root.path());
    cx.update(|window, cx| {
        app.update(cx, |state, cx| {
            let key = state.workspace_key();
            state.run_controls = crate::run::RunControls::load_with_project(
                &key,
                Some(root.path().join("local")),
                Some(root.path().into()),
            );
            state
                .run_controls
                .note_debug_availability(Ok("independent-debugger".into()));
            state
                .run_controls
                .note_debug_capabilities(editor_core::DebugCapabilities {
                    breakpoints: true,
                    resume_pause: true,
                    step: true,
                    inspect: true,
                });
            state.run_controls.begin_debug_session("native-focus");
            state
                .run_controls
                .note_debug_provider_owner("native-focus", "independent-debugger");
            state
                .run_controls
                .note_debug_provider_session("native-focus", "debug-fixture");
            state.run_controls.note_debug_state(
                "native-focus",
                editor_core::DebugSessionState::Paused {
                    source: "probe.rs".into(),
                    line: 2,
                    reason: None,
                },
            );
            state.debug_panel.open = true;
            let _ = window;
            cx.notify();
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let region = cx.debug_bounds("debug-panel-state").unwrap();
    cx.simulate_event(gpui::MouseMoveEvent {
        position: region.center(),
        pressed_button: None,
        modifiers: Default::default(),
    });
    cx.run_until_parked();
    cx.simulate_click(region.center(), Default::default());
    cx.run_until_parked();
    assert!(
        cx.update(|window, cx| app.read(cx).debug_panel.focus_handle(cx).is_focused(window)),
        "the physical panel click owns focus after deferred source navigation"
    );
    // A new pause can reveal a source after the user already focused inspection.
    cx.update(|window, cx| {
        app.update(cx, |state, cx| {
            assert!(state.open_debug_location("probe.rs", 2, window, cx))
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.update(|window, cx| app.read(cx).debug_panel.focus_handle(cx).is_focused(window)),
        "asynchronous source navigation must not replace the panel focus"
    );
}

/// Long stacks and local-variable lists keep row height and scroll under a real native wheel event.
#[gpui::test]
fn debug_stack_and_locals_scroll_with_a_native_wheel(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (app, cx) = open_editor(cx, root.path());
    cx.update(|_,cx|app.update(cx,|state,cx| {
        state.run_controls.begin_debug_session("wheel");
        state.run_controls.note_debug_provider_session("wheel","wheel-session");
        let report=plugin_runtime::DebugSession::from_value(&serde_json::json!({"session":"wheel-session","state":"paused","pause":1,"source":"probe.rs","line":2})).unwrap();
        state.run_controls.observe_debug_report(&report);
        let request=state.run_controls.begin_debug_request(crate::run::DebugMethod::Frames,None).unwrap();
        let frames=(0..50).map(|id|editor_core::StackFrame {id,name:format!("frame {id}"),source:"probe.rs".into(),line:2}).collect();
        state.run_controls.apply_debug_answer(request,Some(frames),None).unwrap();
        let request=state.run_controls.begin_debug_request(crate::run::DebugMethod::Variables,Some(0)).unwrap();
        let variables=(0..50).map(|id|editor_core::DebugVariable {name:format!("value{id}"),value:id.to_string()}).collect();
        state.run_controls.apply_debug_answer(request,None,Some(variables)).unwrap();
        state.debug_panel.open=true;cx.notify();
    }));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    for (pane, row) in [
        ("debug-panel-frames", "debug-panel-frame-0"),
        ("debug-panel-variables", "run-debug-variable-0-value0"),
    ] {
        let before = cx.debug_bounds(row).unwrap();
        let bounds = cx.debug_bounds(pane).unwrap();
        cx.simulate_event(gpui::MouseMoveEvent {
            position: bounds.center(),
            pressed_button: None,
            modifiers: Default::default(),
        });
        cx.run_until_parked();
        cx.simulate_event(gpui::ScrollWheelEvent {
            position: bounds.center(),
            delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-350.))),
            modifiers: Default::default(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let after = cx.debug_bounds(row).unwrap();
        assert!(
            after.origin.y < before.origin.y,
            "{pane} wheel must move actual rows: before={before:?}, after={after:?}, pane={bounds:?}"
        );
    }
}
