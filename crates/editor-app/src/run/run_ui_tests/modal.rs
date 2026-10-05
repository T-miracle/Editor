//! Real modal callbacks must open/close outside the borrowed editor/window update.
use super::*;

/// Save and Cancel remove the actual modal layer, rather than leaving an empty modal.
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
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| cx.windows().len()), 1);
    assert!(cx.debug_bounds("run-config-form").is_some());
    let owner = app.clone();
    cx.update(move |_, cx| owner.update(cx, |state, cx| state.close_run_form(cx)));
    assert!(cx.update(|_, cx| app.read(cx).run_form.is_none()));
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| cx.windows().len()), 1);
    // A subsequent dialog must retain a fresh form rather than an abandoned deferred callback.
    cx.update(|window, cx| {
        app.update(cx, |state, cx| {
            state.open_run_config_dialog(window, cx, None)
        })
    });
    cx.run_until_parked();
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
            state.commit_run_form(cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| cx.windows().len()), 1);
    assert_eq!(
        cx.update(|_, cx| app.read(cx).run_controls.selected().unwrap().name.clone()),
        "Native modal fixture"
    );
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
