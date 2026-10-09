//! The shared native input keeps composition separate from a confirmed plugin result.
use super::*;
use gpui_kit::{TestAppContext, component::Root, gpui};
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, api},
};

/// IME Enter first commits text; the next Enter confirms the actual bounded UTF-8 value.
#[gpui::test]
#[ignore = "build capability-example with current --plugin-package before running"]
fn native_plugin_input_does_not_confirm_marked_ime_text(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::typography::init(cx);
        crate::apply_theme(crate::builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        directory.path().join("runtime"),
        Environment {
            workspace: directory.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let package = Package::read(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example-0.18.0.zip"),
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    manager
        .invoke_command(
            &package.manifest.id,
            "scope-probe",
            serde_json::to_value(api::Operation::Editor {
                operation: api::EditorOperation::Interaction {
                    operation: Operation::Input {
                        title: "名称 / Name".into(),
                        value: String::new(),
                        placeholder: None,
                        password: false,
                        max_bytes: 64,
                    },
                },
                timeout_ms: 30000,
            })
            .unwrap(),
        )
        .unwrap();
    let request = manager
        .live
        .get_mut(&package.manifest.id)
        .unwrap()
        .take_editor_requests()
        .pop()
        .unwrap();
    assert!(request.begin());
    let mut prompt = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let owner = cx.new(|cx| {
            HostInteraction::new(package.manifest.id.clone(), request.clone(), window, cx)
        });
        prompt = Some(owner.clone());
        Root::new(owner, window, cx)
    });
    let prompt = prompt.unwrap();
    visual.run_until_parked();
    // Use the actual Base Input handler contract used by the Windows IME bridge.
    let input = visual.update(|_, cx| prompt.read(cx).input.clone());
    visual.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx)
        })
    });
    visual.simulate_keystrokes("enter");
    assert!(
        !request.status().is_terminal(),
        "marked composition is not a user-confirmed result"
    );
    visual.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_text_in_range(None, "你好", window, cx)
        })
    });
    visual.simulate_keystrokes("enter");
    assert!(
        matches!(request.status(), api::RequestUpdate::Completed { result: Ok(api::EditorValue::Interaction(Value::Input(text))) } if text == "你好")
    );
    manager.poll();
}

/// Keyboard navigation makes a far-away choice visible while confirmation stays reachable.
#[gpui::test]
#[ignore = "build capability-example with current --plugin-package before running"]
fn native_plugin_quick_pick_scrolls_the_keyboard_target_into_view(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::typography::init(cx);
        crate::apply_theme(crate::builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let mut manager = Manager::open(
        directory.path().join("runtime"),
        Environment {
            workspace: directory.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let package = Package::read(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-api-test/capability-example-0.18.0.zip"),
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    manager
        .invoke_command(
            &package.manifest.id,
            "scope-probe",
            serde_json::to_value(api::Operation::Editor {
                operation: api::EditorOperation::Interaction {
                    operation: Operation::QuickPick {
                        title: "Choose a target".into(),
                        items: (0..512)
                            .map(
                                |index| plugin_runtime::plugin_protocol::interaction::PickItem {
                                    id: index.to_string(),
                                    label: format!("Choice {index:03}"),
                                    description: Some("Keyboard reachable".into()),
                                },
                            )
                            .collect(),
                    },
                },
                timeout_ms: 30000,
            })
            .unwrap(),
        )
        .unwrap();
    let request = manager
        .live
        .get_mut(&package.manifest.id)
        .unwrap()
        .take_editor_requests()
        .pop()
        .unwrap();
    assert!(request.begin());
    let (_, visual) = cx.add_window_view(|window, cx| {
        let owner = cx.new(|cx| {
            HostInteraction::new(package.manifest.id.clone(), request.clone(), window, cx)
        });
        Root::new(owner, window, cx)
    });
    visual.simulate_resize(gpui_kit::size(px(600.), px(720.)));
    visual.run_until_parked();
    visual.simulate_keystrokes("end");
    visual.run_until_parked();
    let last = visual
        .debug_bounds("plugin-pick-511")
        .expect("last choice must be visible");
    assert!(
        last.center().y < px(600.),
        "keyboard choice is outside the visible prompt: {last:?}"
    );
    let confirm = visual.debug_bounds("plugin-interaction-confirm").unwrap();
    assert!(
        confirm.center().y < px(700.),
        "confirmation must remain in view"
    );
    visual.simulate_keystrokes("enter");
    assert!(matches!(request.status(), api::RequestUpdate::Completed {
        result: Ok(api::EditorValue::Interaction(Value::Picked(id)))
    } if id == "511"));
    manager.poll();
}
