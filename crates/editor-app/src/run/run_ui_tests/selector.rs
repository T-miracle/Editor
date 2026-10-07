//! The title-bar selector anchors to its button and exposes only saved choices plus editing.
use super::*;

/// Empty lists have a disabled placeholder; Enter skips it and opens the native editor.
#[gpui::test]
fn empty_configuration_selector_is_anchored_and_opens_editing(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (app, cx) = open_editor(cx, root.path());
    for dark in [false, true] {
        cx.update(|_, cx| apply_theme(builtin_theme(dark), cx));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let button = cx.debug_bounds("run-config-selector").unwrap();
        // Label and arrow share a center and equal outer insets, independent of theme.
        let label = cx.debug_bounds("run-config-selector-label").unwrap();
        let arrow = cx.debug_bounds("run-config-selector-chevron").unwrap();
        assert_eq!(label.center().y, arrow.center().y);
        let insets = [
            label.left() - button.left(),
            button.right() - arrow.right(),
            label.top() - button.top(),
            button.bottom() - label.bottom(),
        ];
        assert!(
            insets
                .iter()
                .all(|inset| (*inset - insets[0]).abs() <= px(0.5)),
            "equal content insets: {insets:?}"
        );
        // Clicking away from the button's lower edge must not anchor to the pointer.
        cx.simulate_click(button.center(), Default::default());
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.update(|_, cx| {
            let popup = app.read(cx).run_menu.as_ref().unwrap().popup.read(cx);
            assert_eq!(popup.position, point(button.left(), button.bottom()));
            assert_eq!(popup.items.len(), 2);
            assert!(popup.items[0].disabled);
            assert!(!popup.items[0].separator_before);
            assert!(!popup.items[1].disabled);
            assert!(popup.items[1].separator_before);
            assert_eq!(popup.items[0].label, rust_i18n::t!("run.menu_empty"));
            assert_eq!(popup.items[1].label, rust_i18n::t!("run.menu_edit"));
        });
        let menu = cx.debug_bounds("native-popup-menu").unwrap();
        assert!(menu.top() >= button.bottom());
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(cx.update(|_, cx| app.read(cx).run_menu.is_none()));
    }
    let button = cx.debug_bounds("run-config-selector").unwrap();
    cx.simulate_click(button.center(), Default::default());
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_keystrokes("enter");
    use_run_dialog(cx, &app);
    assert!(cx.debug_bounds("run-config-form").is_some());
}

/// Saved entries remain selectable, with exactly one separator before the final edit action.
#[gpui::test]
fn saved_configuration_selector_has_only_two_sections(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let stored = store_plugin_configuration(&storage_key_of(root.path()), "Saved program");
    let (app, cx) = open_editor(cx, root.path());
    let button = cx.debug_bounds("run-config-selector").unwrap();
    cx.simulate_click(button.center(), Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let popup = app.read(cx).run_menu.as_ref().unwrap().popup.read(cx);
        assert_eq!(popup.items.len(), 2);
        assert!(popup.items[0].label.starts_with("Saved program"));
        assert!(!popup.items[0].disabled);
        assert!(!popup.items[0].separator_before);
        assert!(popup.items[1].separator_before);
        assert!(!popup.items[1].disabled);
    });
    let _ = std::fs::remove_file(stored);
}
