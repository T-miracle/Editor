//! Observable native interactions for plugin defaults, staged edits and lossless configuration saves.
use super::*;
use crate::run::{RunConfigDraft, RunControls};

/// Click the same hit region used by pointer interaction, then complete its retained repaint.
#[track_caller]
fn click(cx: &mut gpui_kit::VisualTestContext, selector: &'static str) {
    // Optional content can be below the clipped viewport; reach it with the actual native wheel.
    for _ in 0..8 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let bounds = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("missing control: {selector}"));
        let Some(page) = cx.debug_bounds("run-config-page") else {
            break;
        };
        if !selector.starts_with("run-config-")
            || bounds.center().x < page.left()
            || bounds.center().x > page.right()
            || matches!(
                selector,
                "run-config-save" | "run-config-cancel" | "run-config-save-location"
            )
            || (bounds.center().y >= page.top() && bounds.center().y <= page.bottom())
        {
            break;
        }
        let delta = if bounds.center().y < page.top() {
            150.
        } else {
            -150.
        };
        cx.simulate_event(gpui::MouseMoveEvent {
            position: page.center(),
            pressed_button: None,
            modifiers: Default::default(),
        });
        cx.run_until_parked();
        cx.simulate_event(gpui::ScrollWheelEvent {
            position: page.center(),
            delta: gpui::ScrollDelta::Pixels(point(px(0.), px(delta))),
            modifiers: Default::default(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        cx.run_until_parked();
    }
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let bounds = cx.debug_bounds(selector).unwrap();
    cx.simulate_event(gpui::MouseMoveEvent {
        position: bounds.center(),
        pressed_button: None,
        modifiers: Default::default(),
    });
    cx.run_until_parked();
    cx.simulate_click(bounds.center(), Default::default());
    cx.run_until_parked();
    use_live_window(cx);
}

/// All visible edits go through native focus, selection and text input rather than draft setters.
#[track_caller]
fn edit(cx: &mut gpui_kit::VisualTestContext, selector: &'static str, value: &str) {
    click(cx, selector);
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input(value);
    cx.run_until_parked();
}

/// Isolate storage before opening the production modal, retaining the actual save callbacks.
fn open_form(
    cx: &mut gpui_kit::VisualTestContext,
    app: &Entity<EditorApp>,
    root: &std::path::Path,
    configs: &[(&str, &str)],
) {
    cx.update(|window, cx| {
        app.update(cx, |state, cx| {
            let key = state.workspace_key();
            state.run_controls =
                RunControls::load_with_project(&key, Some(root.join("local")), Some(root.into()));
            for (id, name) in configs {
                let mut draft = RunConfigDraft::from_config(None, (*id).into());
                draft.name = (*name).into();
                draft.program = "fixture.exe".into();
                state
                    .run_controls
                    .upsert(draft.to_config().unwrap(), &key)
                    .unwrap();
            }
            state.open_run_config_dialog(window, cx, configs.first().map(|(id, _)| (*id).into()));
        })
    });
    use_run_dialog(cx, app);
}

/// Unconfirmed plugin choices never persist; a saved choice keeps its opaque preparation binding.
#[gpui::test]
fn plugin_defaults_are_staged_until_save_and_keep_their_bindings(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (app, cx) = open_editor(cx, root.path());
    open_form(cx, &app, root.path(), &[]);
    let target = plugin_schema::DiscoveredTarget {
        id: "independent-default".into(),
        provider: "arbitrary-provider".into(),
        target_type: "tool@1".into(),
        label: "Plugin application".into(),
        // Display labels may contain whitespace; saving must never interpret them as paths.
        program: " Provider target ".into(),
        found_in: "project.toml".into(),
        fields: [
            (
                "provider_binding".into(),
                r#"{"target":"application","profile":"debug"}"#.into(),
            ),
            (
                "program_args".into(),
                "--plugin-default\nliteral space".into(),
            ),
        ]
        .into(),
    };
    cx.update(|_, cx| {
        app.update(cx, |state, cx| {
            state.run_controls.reconcile_discovered(&[target.clone()]);
            cx.notify();
        })
    });
    cx.run_until_parked();
    click(cx, "run-config-target-independent-default");
    assert!(
        cx.debug_bounds("run-config-program").is_none(),
        "opaque bindings have no editable program field"
    );
    click(cx, "run-config-startup");
    assert!(
        cx.debug_bounds("run-config-build-remove-0").is_none(),
        "provider preparation cannot be deleted"
    );
    click(cx, "run-config-cancel");
    assert!(cx.update(|_, cx| app.read(cx).run_controls.configurations().is_empty()));
    let key = cx.update(|_, cx| app.read(cx).workspace_key());
    assert!(
        RunControls::load_with_project(
            &key,
            Some(root.path().join("local")),
            Some(root.path().into())
        )
        .configurations()
        .is_empty()
    );
    cx.update(|window, cx| {
        app.update(cx, |state, cx| {
            state.open_run_config_dialog(window, cx, None)
        })
    });
    use_run_dialog(cx, &app);
    click(cx, "run-config-target-independent-default");
    click(cx, "run-config-arguments-edit");
    edit(
        cx,
        "run-config-arguments",
        "literal space\nquote\"value\n中文;&|",
    );
    click(cx, "run-config-detail-done");
    assert!(cx.update(|_, cx| app.read(cx).run_controls.configurations().is_empty()));
    click(cx, "run-config-save");
    let saved = cx.update(|_, cx| app.read(cx).run_controls.selected().unwrap().clone());
    assert!(
        matches!(&saved.target, editor_core::RunTarget::Provided {provider, binding, ..} if provider == &target.provider && binding == &target.fields["provider_binding"])
    );
    assert_eq!(
        saved.literal_arguments(),
        ["literal space", "quote\"value", "中文;&|"]
    );
    assert_eq!(
        saved.build,
        editor_core::configuration_for(&target, saved.id.clone(), saved.name.clone()).build
    );
    assert_eq!(
        cx.update(|_, cx| app
            .read(cx)
            .run_controls
            .target_template(&target.id, &key)
            .unwrap()),
        saved,
        "reselecting a target preserves user edits"
    );
    assert!(cx.update(|_, cx| app.read(cx).run_controls.sessions().is_empty()));
}

/// Field Cancel/Escape restores that field; completing a field never saves the surrounding draft.
#[gpui::test]
fn detail_cancel_restores_literals_and_enter_does_not_save_the_configuration(
    cx: &mut TestAppContext,
) {
    let root = tempfile::tempdir().unwrap();
    let (app, cx) = open_editor(cx, root.path());
    open_form(cx, &app, root.path(), &[("first", "First")]);
    click(cx, "run-config-arguments-edit");
    edit(cx, "run-config-arguments", "discard this");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(cx.debug_bounds("run-config-arguments-edit").is_some());
    click(cx, "run-config-arguments-edit");
    edit(cx, "run-config-arguments", "hello world\n中文;&|");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(
        cx.update(|_, cx| app.read(cx).run_form.is_some()),
        "native multiline Enter cannot save the outer configuration"
    );
    click(cx, "run-config-detail-done");
    assert!(cx.update(|_, cx| app.read(cx).run_form.is_some()));
    assert!(cx.update(|_, cx| {
        app.read(cx)
            .run_controls
            .configuration("first")
            .unwrap()
            .literal_arguments()
            .is_empty()
    }));
    click(cx, "run-config-arguments-edit");
    edit(cx, "run-config-arguments", "discard again");
    click(cx, "run-config-detail-cancel");
    click(cx, "run-config-save");
    assert_eq!(
        cx.update(|_, cx| app
            .read(cx)
            .run_controls
            .configuration("first")
            .unwrap()
            .literal_arguments()),
        ["hello world", "中文;&|"]
    );
}

/// All three navigation decisions retain a concrete draft or explicitly move after saving it.
#[gpui::test]
fn switching_configurations_can_cancel_discard_or_save_pending_edits(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (app, cx) = open_editor(cx, root.path());
    open_form(
        cx,
        &app,
        root.path(),
        &[("first", "First"), ("second", "Second")],
    );
    edit(cx, "run-config-name", "Keep my input");
    click(cx, "run-config-existing-second");
    click(cx, "run-config-switch-cancel");
    click(cx, "run-config-existing-second");
    click(cx, "run-config-switch-discard");
    assert_eq!(
        cx.update(|_, cx| app
            .read(cx)
            .run_form
            .as_ref()
            .unwrap()
            .read(cx)
            .draft()
            .id
            .clone()),
        "second"
    );
    assert_eq!(
        cx.update(|_, cx| app
            .read(cx)
            .run_controls
            .configuration("first")
            .unwrap()
            .name
            .clone()),
        "First"
    );
    click(cx, "run-config-existing-first");
    edit(cx, "run-config-name", "Saved before moving");
    click(cx, "run-config-existing-second");
    click(cx, "run-config-switch-save");
    assert_eq!(
        cx.update(|_, cx| app
            .read(cx)
            .run_controls
            .configuration("first")
            .unwrap()
            .name
            .clone()),
        "Saved before moving"
    );
    assert_eq!(
        cx.update(|_, cx| app
            .read(cx)
            .run_form
            .as_ref()
            .unwrap()
            .read(cx)
            .draft()
            .id
            .clone()),
        "second"
    );
    click(cx, "run-config-cancel");
    let key = cx.update(|_, cx| app.read(cx).workspace_key());
    assert_eq!(
        RunControls::load_with_project(
            &key,
            Some(root.path().join("local")),
            Some(root.path().into())
        )
        .configuration("first")
        .unwrap()
        .name,
        "Saved before moving"
    );
}

/// A malformed collapsed setting reopens its actual editor and retains the invalid native text.
#[gpui::test]
fn saving_a_bad_hidden_setting_opens_the_field_without_persisting_it(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (app, cx) = open_editor(cx, root.path());
    open_form(cx, &app, root.path(), &[("first", "First")]);
    click(cx, "run-config-more");
    click(cx, "run-config-environment-edit");
    edit(cx, "run-config-environment", "MALFORMED");
    click(cx, "run-config-detail-done");
    click(cx, "run-config-more");
    click(cx, "run-config-save");
    assert!(cx.debug_bounds("run-config-environment").is_some());
    assert!(cx.update(|_, cx| {
        app.read(cx)
            .run_form
            .as_ref()
            .unwrap()
            .read(cx)
            .error()
            .is_some()
    }));
    assert!(cx.update(|_, cx| {
        app.read(cx)
            .run_controls
            .configuration("first")
            .unwrap()
            .env
            .is_empty()
    }));
    edit(cx, "run-config-environment", "KEY=literal=value");
    click(cx, "run-config-detail-done");
    click(cx, "run-config-save");
    assert_eq!(
        cx.update(|_, cx| app
            .read(cx)
            .run_controls
            .configuration("first")
            .unwrap()
            .env["KEY"]
            .clone()),
        "literal=value"
    );
}

/// Editing prepared actions uses typed native fields; Cancel does not alter the original row.
#[gpui::test]
fn structured_actions_preserve_literal_argv_and_keep_build_and_prelaunch_separate(
    cx: &mut TestAppContext,
) {
    let root = tempfile::tempdir().unwrap();
    let (app, cx) = open_editor(cx, root.path());
    open_form(cx, &app, root.path(), &[("first", "First")]);
    click(cx, "run-config-startup");
    click(cx, "run-config-build-add-0");
    edit(cx, "run-step-name", "Build fixture");
    edit(cx, "run-step-program", "tool.exe");
    edit(cx, "run-step-arguments", "literal space\n中文=;&|");
    click(cx, "run-config-detail-done");
    assert_eq!(
        cx.update(|_, cx| app
            .read(cx)
            .run_form
            .as_ref()
            .unwrap()
            .read(cx)
            .step_row_count(crate::run::RunField::Build)),
        1,
        "accepting the typed build action adds exactly one draft row"
    );
    click(cx, "run-config-build-edit-0");
    edit(cx, "run-step-program", "discard.exe");
    click(cx, "run-config-detail-cancel");
    click(cx, "run-config-prelaunch-add-0");
    edit(cx, "run-step-name", "Before running");
    edit(cx, "run-step-program", "prepare.exe");
    click(cx, "run-config-detail-done");
    click(cx, "run-config-save");
    assert!(
        cx.update(|_, cx| app.read(cx).run_form.is_none()),
        "valid typed actions must save and close the modal: {:?}",
        cx.update(|_, cx| app
            .read(cx)
            .run_form
            .as_ref()
            .map(|form| form.read(cx).error().map(str::to_owned)))
    );
    let saved = cx.update(|_, cx| {
        app.read(cx)
            .run_controls
            .configuration("first")
            .unwrap()
            .clone()
    });
    assert_eq!(saved.build.len(), 1);
    assert_eq!(saved.build[0].target.executable(), Some("tool.exe"));
    assert_eq!(
        saved.build[0].target.arguments(),
        ["literal space", "中文=;&|"]
    );
    assert_eq!(saved.prelaunch.len(), 1);
    assert_eq!(saved.prelaunch[0].target.executable(), Some("prepare.exe"));
    assert!(cx.update(|_, cx| app.read(cx).run_controls.sessions().is_empty()));
}

/// Choose one of the three native type tabs by its visible equal-width hit region.
fn action_kind(cx: &mut gpui_kit::VisualTestContext, index: usize) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let bounds = cx.debug_bounds("run-step-kind").unwrap();
    let position = point(
        bounds.left() + bounds.size.width * ((index as f32 + 0.5) / 3.),
        bounds.center().y,
    );
    cx.simulate_event(gpui::MouseMoveEvent {
        position,
        pressed_button: None,
        modifiers: Default::default(),
    });
    cx.run_until_parked();
    cx.simulate_click(position, Default::default());
    cx.run_until_parked();
}

/// References stay references, and explicit Shell fields preserve the final script argument intact.
#[gpui::test]
fn structured_actions_support_shell_and_build_reference(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (app, cx) = open_editor(cx, root.path());
    open_form(
        cx,
        &app,
        root.path(),
        &[("first", "First"), ("library", "Library")],
    );
    click(cx, "run-config-startup");
    click(cx, "run-config-prelaunch-add-0");
    edit(cx, "run-step-name", "Build library");
    action_kind(cx, 2);
    click(cx, "run-step-reference");
    click(cx, "native-menu-Library");
    click(cx, "run-config-detail-done");
    click(cx, "run-config-prelaunch-add-1");
    action_kind(cx, 1);
    edit(cx, "run-step-name", "Script preparation");
    edit(cx, "run-step-program", "powershell.exe");
    edit(cx, "run-step-arguments", "-NoProfile\n-Command");
    edit(
        cx,
        "run-step-script",
        "Write-Output '中文;&|'\nWrite-Output 'second line'",
    );
    click(cx, "run-config-detail-done");
    click(cx, "run-config-save");
    let saved = cx.update(|_, cx| {
        app.read(cx)
            .run_controls
            .configuration("first")
            .unwrap()
            .clone()
    });
    assert_eq!(
        saved.prelaunch[0].target,
        editor_core::StepTarget::Build {
            config: "Library".into()
        }
    );
    assert_eq!(
        saved.prelaunch[1].target,
        editor_core::StepTarget::Action {
            target: editor_core::RunTarget::Script {
                interpreter: "powershell.exe".into(),
                args: vec!["-NoProfile".into(), "-Command".into()],
                script: "Write-Output '中文;&|'\nWrite-Output 'second line'".into()
            }
        }
    );
}

/// Copy remains an unsaved draft; Delete waits for an explicit decision and Cancel preserves data.
#[gpui::test]
fn copying_and_deleting_configurations_keep_their_storage_boundaries(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (app, cx) = open_editor(cx, root.path());
    open_form(cx, &app, root.path(), &[("first", "First")]);
    click(cx, "run-config-arguments-edit");
    edit(cx, "run-config-arguments", "copied literal space");
    click(cx, "run-config-detail-done");
    click(cx, "run-config-actions");
    click(cx, "native-menu-copy");
    assert_eq!(
        cx.update(|_, cx| app.read(cx).run_controls.configurations().len()),
        1
    );
    let copy = cx.update(|_, cx| {
        app.read(cx)
            .run_form
            .as_ref()
            .unwrap()
            .read(cx)
            .draft()
            .id
            .clone()
    });
    assert_ne!(copy, "first");
    click(cx, "run-config-save");
    assert_eq!(
        cx.update(|_, cx| app
            .read(cx)
            .run_controls
            .configuration(&copy)
            .unwrap()
            .literal_arguments()),
        ["copied literal space"]
    );
    cx.update(|window, cx| {
        app.update(cx, |state, cx| {
            state.open_run_config_dialog(window, cx, Some(copy.clone()))
        })
    });
    use_run_dialog(cx, &app);
    click(cx, "run-config-actions");
    click(cx, "native-menu-delete");
    click(cx, "run-config-switch-cancel");
    assert_eq!(
        cx.update(|_, cx| app.read(cx).run_controls.configurations().len()),
        2
    );
    click(cx, "run-config-actions");
    click(cx, "native-menu-delete");
    click(cx, "run-config-switch-discard");
    assert!(cx.update(|_, cx| app.read(cx).run_controls.configuration(&copy).is_none()));
    assert!(cx.update(|_, cx| app.read(cx).run_controls.configuration("first").is_some()));
}
